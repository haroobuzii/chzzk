//! 현재 사용자 확인: 요청의 쿠키에서 세션을 찾아 "비로그인 / 사용자"를 알려 준다.
//!
//! 모든 권한 검사는 이 추출기를 거친다. 핸들러 인자에 `CurrentUser`를 받고,
//! 로그인이 필요하면 [`CurrentUser::require`](401), 소유자만 되면 [`User::require_owner`](403)를 부른다.
//!
//! ```ignore
//! async fn rename(current: CurrentUser, ...) -> Response {
//!     let user = match current.require() { Ok(user) => user, Err(response) => return response };
//!     if let Err(response) = user.require_owner(broadcast.owner_id) { return response; }
//!     ...
//! }
//! ```

use axum::{
    extract::{FromRef, FromRequestParts},
    http::{header, request::Parts, HeaderMap, StatusCode},
    response::Response,
};
use sqlx::PgPool;

use super::{store, SESSION_COOKIE};
use crate::routes::failure;

/// 로그인한 사용자 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    /// 사용자 번호.
    pub id: i64,
    /// 아이디. 가입할 때 쓴 대소문자 그대로다.
    pub username: String,
}

// `require_owner`, `require`, `login_required`는 방송 관리·댓글 라우트(02~05 티켓)가 쓴다.
// 그 전까지는 서버 빌드에서 쓰는 곳이 없어 경고가 나므로 끈다. 쓰기 시작하면 이 `allow`들을 지운다.
#[allow(dead_code)]
impl User {
    /// 이 사용자가 `owner_id`의 소유자인지 본다. 아니거나 소유자 없는 방송이면 403 응답.
    pub fn require_owner(&self, owner_id: Option<i64>) -> Result<(), Response> {
        if owner_id == Some(self.id) {
            Ok(())
        } else {
            Err(forbidden())
        }
    }
}

/// 요청을 보낸 사람: 로그인한 사용자이거나 비로그인(`None`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentUser(pub Option<User>);

impl CurrentUser {
    /// 로그인한 사용자. 비로그인이면 `None`.
    pub fn user(&self) -> Option<&User> {
        self.0.as_ref()
    }

    /// 로그인한 사용자를 돌려준다. 비로그인이면 401 응답.
    pub fn require(self) -> Result<User, Response> {
        self.0.ok_or_else(login_required)
    }
}

/// 401: 로그인이 필요한데 로그인하지 않았다.
pub fn login_required() -> Response {
    failure(
        StatusCode::UNAUTHORIZED,
        "login_required",
        "로그인이 필요해요.",
    )
}

/// 403: 로그인했지만 이 일을 할 권한이 없다.
pub fn forbidden() -> Response {
    failure(StatusCode::FORBIDDEN, "forbidden", "권한이 없어요.")
}

impl<S> FromRequestParts<S> for CurrentUser
where
    PgPool: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Some(token) = session_token(&parts.headers) else {
            return Ok(Self(None));
        };
        let pool = PgPool::from_ref(state);
        match store::find_session_user(&pool, &token, chrono::Utc::now()).await {
            Ok(user) => Ok(Self(user)),
            Err(e) => {
                dioxus::logger::tracing::error!("로그인 세션 조회 실패: {e}");
                Err(failure(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "server_error",
                    "로그인 상태를 확인하지 못했어요. 잠시 뒤 다시 시도해 주세요.",
                ))
            }
        }
    }
}

/// `Cookie` 헤더들에서 세션 쿠키 값을 찾는다.
pub(super) fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, value)| *name == SESSION_COOKIE && !value.is_empty())
        .map(|(_, value)| value.to_string())
}
