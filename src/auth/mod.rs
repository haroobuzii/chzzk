//! 계정과 로그인: 회원가입, 로그인·로그아웃, 현재 사용자 확인(ADR 0006).
//!
//! 로그인 상태는 DB의 세션과 `HttpOnly` 쿠키로 유지한다. 쿠키에는 세션 값 원문이, DB에는 그 해시가 있다.

mod current_user;
mod origin;
pub mod rules;
mod store;

#[cfg(test)]
pub mod personas;
#[cfg(test)]
mod tests;

use axum::{
    extract::{rejection::JsonRejection, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

// 401·403 도우미는 방송 관리·댓글 라우트(02~05 티켓)가 쓴다. 그 전까지는 서버 빌드에서 쓰는 곳이 없다.
#[allow(unused_imports)]
pub use current_user::{forbidden, login_required, CurrentUser, User};
pub use origin::guard_origin;

use crate::routes::{bad_request, failure};
use rules::{validate_password, validate_username};

/// 세션 쿠키 이름.
pub const SESSION_COOKIE: &str = "chzzk_session";

/// 계정 API 라우터.
pub fn api(pool: PgPool) -> Router {
    Router::new()
        .route("/api/auth/signup", post(signup))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", get(me))
        .with_state(pool)
}

/// 가입·로그인 요청 본문.
#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

/// `POST /api/auth/signup`: 사용자를 만들고 바로 로그인시킨다.
///
/// 성공하면 201과 세션 쿠키, `{username}`. 규칙 위반은 400, 이미 쓰는 아이디(대소문자 무시)는 409다.
async fn signup(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    body: Result<Json<Credentials>, JsonRejection>,
) -> Response {
    let Ok(Json(credentials)) = body else {
        return invalid_body();
    };
    if let Err(e) = validate_username(&credentials.username)
        .and_then(|()| validate_password(&credentials.password))
    {
        return bad_request(e.code(), e.message());
    }

    let hash = match store::hash_password(credentials.password).await {
        Ok(hash) => hash,
        Err(e) => return server_error("회원가입", e),
    };
    let user = match store::create_user(&pool, &credentials.username, &hash).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return failure(
                StatusCode::CONFLICT,
                "username_taken",
                "이미 쓰는 아이디예요.",
            )
        }
        Err(e) => return server_error("회원가입", e.into()),
    };
    start_session(&pool, &headers, user, StatusCode::CREATED).await
}

/// `POST /api/auth/login`: 아이디와 비밀번호가 맞으면 200과 세션 쿠키, `{username}`.
///
/// 없는 아이디와 틀린 비밀번호는 둘 다 401이고 같은 메시지다.
async fn login(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    body: Result<Json<Credentials>, JsonRejection>,
) -> Response {
    let Ok(Json(credentials)) = body else {
        return invalid_body();
    };
    match store::check_credentials(&pool, &credentials.username, &credentials.password).await {
        Ok(Some(user)) => start_session(&pool, &headers, user, StatusCode::OK).await,
        Ok(None) => failure(
            StatusCode::UNAUTHORIZED,
            "bad_credentials",
            "아이디 또는 비밀번호가 맞지 않아요.",
        ),
        Err(e) => server_error("로그인", e),
    }
}

/// `POST /api/auth/logout`: 세션을 지우고 쿠키를 만료시킨다. 로그인하지 않았어도 204다.
async fn logout(State(pool): State<PgPool>, headers: HeaderMap) -> Response {
    if let Some(token) = current_user::session_token(&headers) {
        if let Err(e) = store::delete_session(&pool, &token).await {
            return server_error("로그아웃", e.into());
        }
    }
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, session_cookie("", 0, &headers))],
    )
        .into_response()
}

/// `GET /api/auth/me`: 로그인했으면 `{username}`, 아니면 `{username: null}`.
async fn me(current: CurrentUser) -> Response {
    let username = current.user().map(|user| user.username.clone());
    Json(json!({ "username": username })).into_response()
}

/// 새 세션을 만들고 쿠키와 `{username}`을 담은 응답을 만든다.
async fn start_session(
    pool: &PgPool,
    headers: &HeaderMap,
    user: User,
    status: StatusCode,
) -> Response {
    let token = match store::create_session(pool, user.id, chrono::Utc::now()).await {
        Ok(token) => token,
        Err(e) => return server_error("로그인 세션 만들기", e.into()),
    };
    let max_age = store::SESSION_DAYS * 24 * 60 * 60;
    (
        status,
        [(header::SET_COOKIE, session_cookie(&token, max_age, headers))],
        Json(json!({ "username": user.username })),
    )
        .into_response()
}

/// 세션 쿠키 `Set-Cookie` 값. 요청이 HTTPS로 들어왔으면(터널은 `X-Forwarded-Proto: https`를 보낸다) `Secure`를 붙인다.
fn session_cookie(token: &str, max_age: i64, headers: &HeaderMap) -> HeaderValue {
    let secure = if origin::is_https(headers) {
        "; Secure"
    } else {
        ""
    };
    format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}{secure}")
        .parse()
        .expect("세션 값은 16진수라 헤더에 쓸 수 있다")
}

fn invalid_body() -> Response {
    bad_request("invalid_body", "아이디와 비밀번호를 보내 주세요.")
}

fn server_error(what: &str, e: anyhow::Error) -> Response {
    dioxus::logger::tracing::error!("{what} 실패: {e}");
    failure(
        StatusCode::INTERNAL_SERVER_ERROR,
        "server_error",
        "요청을 처리하지 못했어요. 잠시 뒤 다시 시도해 주세요.",
    )
}
