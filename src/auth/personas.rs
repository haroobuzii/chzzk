//! 테스트 손님: 쿠키 없는 비로그인 손님과, 진짜 가입 API로 쿠키를 받은 사용자 A·B.
//!
//! 세션은 DB에 있으므로, 여기서 받은 쿠키는 같은 DB를 쓰는 어느 라우터(방송, 라이브, 댓글)에도 통한다.
//! 02~05 티켓의 테스트도 이 도우미로 비로그인·소유자·다른 사용자를 만든다.
//!
//! ```ignore
//! let owner = Persona::user_a(&pool).await;
//! let other = Persona::user_b(&pool).await;
//! let request = owner.sign(Request::delete("/api/broadcasts/1").body(Body::empty()).unwrap());
//! ```

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use sqlx::PgPool;
use tower::ServiceExt;

/// 테스트 손님들이 함께 쓰는 비밀번호.
pub const PASSWORD: &str = "password1234";

/// 요청을 보내는 사람 하나. 비로그인이면 쿠키가 없다.
#[derive(Clone, Debug)]
pub struct Persona {
    /// 로그인했으면 아이디.
    pub username: Option<String>,
    /// `Cookie` 헤더에 넣을 값(`chzzk_session=…`).
    cookie: Option<String>,
}

impl Persona {
    /// 쿠키 없는 비로그인 손님.
    pub fn anonymous() -> Self {
        Self {
            username: None,
            cookie: None,
        }
    }

    /// 사용자 A(아이디 `user_a`). 진짜 가입 API로 쿠키를 받는다.
    pub async fn user_a(pool: &PgPool) -> Self {
        Self::sign_up(pool, "user_a").await
    }

    /// 사용자 B(아이디 `user_b`). A와 다른 사용자다.
    pub async fn user_b(pool: &PgPool) -> Self {
        Self::sign_up(pool, "user_b").await
    }

    /// `username`과 [`PASSWORD`]로 가입하고, 받은 세션 쿠키를 가진 손님을 돌려준다.
    pub async fn sign_up(pool: &PgPool, username: &str) -> Self {
        Self::post_credentials(pool, "/api/auth/signup", username, StatusCode::CREATED).await
    }

    /// 이미 가입한 `username`과 [`PASSWORD`]로 로그인해 새 세션 쿠키를 받는다.
    pub async fn log_in(pool: &PgPool, username: &str) -> Self {
        Self::post_credentials(pool, "/api/auth/login", username, StatusCode::OK).await
    }

    async fn post_credentials(
        pool: &PgPool,
        path: &str,
        username: &str,
        expected: StatusCode,
    ) -> Self {
        let request = Request::post(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({ "username": username, "password": PASSWORD }).to_string(),
            ))
            .unwrap();
        let response = super::api(pool.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let set_cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            status,
            expected,
            "{path} 응답: {}",
            String::from_utf8_lossy(&body)
        );
        let set_cookie = set_cookie.expect("세션 쿠키가 와야 한다");
        let cookie = set_cookie
            .split(';')
            .next()
            .expect("쿠키 이름=값이 맨 앞에 있다")
            .trim()
            .to_string();
        Self {
            username: Some(username.to_string()),
            cookie: Some(cookie),
        }
    }

    /// `Cookie` 헤더에 넣을 값. 비로그인이면 `None`.
    pub fn cookie(&self) -> Option<&str> {
        self.cookie.as_deref()
    }

    /// 요청에 이 손님의 쿠키를 붙인다. 비로그인이면 그대로 둔다.
    pub fn sign(&self, mut request: Request<Body>) -> Request<Body> {
        if let Some(cookie) = &self.cookie {
            request
                .headers_mut()
                .append(header::COOKIE, cookie.parse().expect("쿠키는 ASCII다"));
        }
        request
    }
}
