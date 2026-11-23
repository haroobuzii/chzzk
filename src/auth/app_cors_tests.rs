//! 앱용 접근 허용(CORS) 테스트.
//!
//! 실제 서버처럼 `Origin` 확인([`guard_origin`])을 씌운 라우터에 앱 웹뷰 출처의 요청을 넣고
//! 응답 헤더와 상태 코드를 본다.

use axum::{
    body::Body,
    http::{header, HeaderName, Method, Request, StatusCode},
    response::Response,
    routing::get,
    Json, Router,
};
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;

use super::guard_origin;
use super::personas::Persona;
use crate::donations::DonationsState;
use crate::live::fake_mediamtx::{FAKE_ANSWER, FAKE_SECRET};
use crate::live::tests::{body_text, session_request, LiveApp, OFFER};
use crate::routes::tests::{app, register_ok};
use crate::test_support::{at, insert_recorded};

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

/// 앱이 GET이 아닌 요청 전에 보내는 사전 요청.
fn preflight(path: &str, method: &str, origin: &str) -> Request<Body> {
    from_app(
        Request::options(path)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, method)
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
            .body(Body::empty())
            .unwrap(),
        origin,
    )
}

fn whep_request(path: &str) -> Request<Body> {
    Request::post(path)
        .header(header::CONTENT_TYPE, "application/sdp")
        .body(Body::from(OFFER))
        .unwrap()
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

fn header_text(response: &Response, name: HeaderName) -> &str {
    response
        .headers()
        .get(name)
        .map_or("", |v| v.to_str().unwrap())
}

/// 방송 목록·단건은 Dioxus 서버 함수라서 화면 라우터와 함께 만들어진다.
/// 여기서는 같은 주소에 세운 자리 라우트로 그 주소의 허용 헤더만 본다.
fn server_function_stand_ins() -> Router {
    Router::new()
        .route("/api/broadcasts/list", get(|| async { Json(json!([])) }))
        .route("/api/broadcasts/info", get(|| async { Json(json!(null)) }))
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

#[sqlx::test]
async fn 앱_출처로_보낸_방송_목록과_단건_GET에_허용_헤더가_붙는다() {
    let router = guard_origin(server_function_stand_ins());

    for path in ["/api/broadcasts/list", "/api/broadcasts/info?id=1"] {
        let request = Request::get(path).body(Body::empty()).unwrap();
        let response = send(&router, from_app(request, WINDOWS_APP)).await;

        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(allowed_origin(&response), Some(WINDOWS_APP), "{path}");
        assert!(!allows_credentials(&response), "{path}");
    }
}

#[sqlx::test]
async fn 앱_출처로_보낸_시청_연결_사전_요청과_POST에_허용_헤더가_붙는다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let id = app.go_live("시청", at(0)).await;
    let router = guard_origin(app.router.clone());
    let whep = format!("/api/live/{id}/whep");

    for origin in [APPLE_APP, WINDOWS_APP] {
        let response = send(&router, preflight(&whep, "POST", origin)).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT, "{origin}");
        assert_eq!(allowed_origin(&response), Some(origin));
        assert!(header_text(&response, header::ACCESS_CONTROL_ALLOW_METHODS).contains("POST"));
        assert!(
            header_text(&response, header::ACCESS_CONTROL_ALLOW_HEADERS).contains("content-type")
        );
        assert!(!allows_credentials(&response));

        let response = send(&router, from_app(whep_request(&whep), origin)).await;
        assert_eq!(response.status(), StatusCode::CREATED, "{origin}");
        assert_eq!(allowed_origin(&response), Some(origin));
        // 앱의 JavaScript가 세션 주소를 읽어야 PATCH·DELETE를 보낼 수 있다.
        assert!(header_text(&response, header::ACCESS_CONTROL_EXPOSE_HEADERS).contains("location"));
        assert!(!allows_credentials(&response));
        assert_eq!(body_text(response).await, FAKE_ANSWER);
    }
}

#[sqlx::test]
async fn 앱_출처로_보낸_시청_세션_PATCH와_DELETE에도_허용_헤더가_붙는다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let id = app.go_live("시청", at(0)).await;
    let router = guard_origin(app.router.clone());
    let session = format!("/api/live/{id}/whep/{FAKE_SECRET}");

    for method in [Method::PATCH, Method::DELETE] {
        let response = send(&router, preflight(&session, method.as_str(), APPLE_APP)).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT, "{method}");
        assert!(
            header_text(&response, header::ACCESS_CONTROL_ALLOW_METHODS).contains(method.as_str())
        );

        let request = from_app(session_request(method.clone(), &session), APPLE_APP);
        let response = send(&router, request).await;
        assert!(
            response.status().is_success(),
            "{method}: {}",
            response.status()
        );
        assert_eq!(allowed_origin(&response), Some(APPLE_APP));
    }
}

#[sqlx::test]
async fn 모르는_출처에는_허용_헤더가_없고_시청_연결도_403이다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let id = app.go_live("시청", at(0)).await;
    let router = guard_origin(app.router.clone().merge(server_function_stand_ins()));
    let whep = format!("/api/live/{id}/whep");

    for origin in [
        "https://evil.example",
        "dioxus://evil.example",
        "http://dioxus.index.html.evil.example",
    ] {
        let request = Request::get("/api/broadcasts/list")
            .body(Body::empty())
            .unwrap();
        let response = send(&router, from_app(request, origin)).await;
        assert_eq!(response.status(), StatusCode::OK, "{origin}");
        assert_eq!(allowed_origin(&response), None, "{origin}");

        let response = send(&router, preflight(&whep, "POST", origin)).await;
        assert_eq!(allowed_origin(&response), None, "{origin}");

        let response = send(&router, from_app(whep_request(&whep), origin)).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{origin}");
        assert_eq!(allowed_origin(&response), None, "{origin}");
    }
    assert!(app.fake.webrtc_requests().is_empty());
}

#[sqlx::test]
async fn 앱_출처로_보낸_로그인_댓글_후원_등록_송출_요청은_로그인했어도_403이다(
    pool: PgPool,
) {
    let user = Persona::user_a(&pool).await;
    let broadcast = insert_recorded(&pool, "녹화", at(0)).await;
    let live = LiveApp::new(pool.clone()).await;
    let ready = live.start_live("준비").await;
    let recorded = app(pool.clone());
    let router = guard_origin(
        super::api(pool.clone())
            .merge(crate::comments::api(pool.clone()))
            .merge(crate::donations::api(DonationsState {
                pool: pool.clone(),
                toss: None,
            }))
            .merge(recorded.router.clone())
            .merge(live.router.clone()),
    );

    let paths = [
        "/api/auth/login".to_owned(),
        "/api/auth/logout".to_owned(),
        format!("/api/broadcasts/{broadcast}/comments"),
        format!("/api/broadcasts/{broadcast}/donations"),
        "/api/broadcasts".to_owned(),
        "/api/live".to_owned(),
        format!("/api/live/{ready}/whip"),
    ];

    for path in &paths {
        for origin in [APPLE_APP, WINDOWS_APP] {
            // 본문은 보지 않는다. `Origin` 확인이 라우트에 닿기 전에 막는다.
            let request = Request::post(path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap();
            let response = send(&router, from_app(user.sign(request), origin)).await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path} {origin}");
            assert_eq!(allowed_origin(&response), None, "{path} {origin}");
        }
        let response = send(&router, preflight(path, "POST", APPLE_APP)).await;
        assert_eq!(allowed_origin(&response), None, "{path}");
    }
    assert!(live.fake.webrtc_requests().is_empty());
}
