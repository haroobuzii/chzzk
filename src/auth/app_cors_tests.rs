//! 앱용 접근 허용(CORS) 테스트.
//!
//! 실제 서버처럼 `Origin` 확인([`guard_origin`])을 씌운 라우터에 앱 웹뷰 출처의 요청을 넣고
//! 응답 헤더와 상태 코드를 본다.

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    response::Response,
    Router,
};
use sqlx::PgPool;
use tower::ServiceExt;

use super::guard_origin;
use crate::routes::tests::{app, register_ok};

/// macOS·iOS 앱 웹뷰의 출처.
const APPLE_APP: &str = "dioxus://index.html";
/// Windows 앱 웹뷰의 출처.
const WINDOWS_APP: &str = "http://dioxus.index.html";

async fn send(router: &Router, request: Request<Body>) -> Response {
    router.clone().oneshot(request).await.unwrap()
}

/// 웹뷰가 다른 출처 요청에 붙이는 헤더를 단다.
fn from_app(mut request: Request<Body>, origin: &str) -> Request<Body> {
    let headers = request.headers_mut();
    headers.insert(header::ORIGIN, origin.parse().unwrap());
    headers.insert("sec-fetch-site", "cross-site".parse().unwrap());
    request
}

fn allowed_origin(response: &Response) -> Option<&str> {
    response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .map(|v| v.to_str().unwrap())
}

fn allows_credentials(response: &Response) -> bool {
    response
        .headers()
        .contains_key(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
}

#[sqlx::test]
async fn 앱_출처로_보낸_영상_GET에는_허용_헤더가_붙고_쿠키_허용_헤더는_없다(
    pool: PgPool,
) {
    let app = app(pool);
    let id = register_ok(&app, "녹화").await;
    let router = guard_origin(app.router.clone());

    for origin in [APPLE_APP, WINDOWS_APP] {
        let request = Request::get(format!("/api/broadcasts/{id}/video"))
            .header(header::RANGE, "bytes=0-3")
            .body(Body::empty())
            .unwrap();
        let response = send(&router, from_app(request, origin)).await;

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT, "{origin}");
        assert_eq!(allowed_origin(&response), Some(origin));
        assert!(!allows_credentials(&response), "{origin}");
    }
}
