//! 앱 웹뷰에서 오는 요청의 접근 허용(CORS, ADR 0009).
//!
//! 앱 웹뷰는 출처가 배포 서버와 다르다(`dioxus://index.html`, `http://dioxus.index.html`).
//! 그 출처에는 공개 읽기(방송 목록·단건·영상 GET)와 시청 연결(WHEP)만 연다.
//! 쿠키 허용 헤더(`Access-Control-Allow-Credentials`)는 붙이지 않으므로, 로그인 쿠키는 앱에서 쓸 수 없다.
//! 나머지 경로는 지금처럼 같은 출처만 받는다([`super::guard_origin`]).

use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// 앱 웹뷰의 출처. macOS·iOS는 `dioxus://`, Windows(WebView2)는 `http(s)://dioxus.index.html`이다.
const APP_ORIGINS: [&str; 3] = [
    "dioxus://index.html",
    "http://dioxus.index.html",
    "https://dioxus.index.html",
];

/// 앱이 읽어야 하는 응답 헤더. WHEP 세션 주소(`Location`)와 영상 일부 전송(`Content-Range`) 등이다.
const EXPOSED_HEADERS: &str = "location, etag, link, content-range, accept-ranges, content-length";
/// 앱이 보낼 수 있는 요청 헤더. WHEP는 `Content-Type: application/sdp`, 영상은 `Range`를 쓴다.
const ALLOWED_HEADERS: &str = "content-type, if-match, range";

/// 앱 출처에 연 경로라면 그 경로에서 쓸 수 있는 방법들.
fn app_methods(path: &str) -> Option<&'static [Method]> {
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match segments.as_slice() {
        // 방송 목록·단건(Dioxus 서버 함수의 GET 주소)
        ["api", "broadcasts", "list" | "info"] => Some(&[Method::GET]),
        // 녹화 영상(Range 포함)
        ["api", "broadcasts", _, "video"] => Some(&[Method::GET, Method::HEAD]),
        // 시청 연결과 그 세션
        ["api", "live", _, "whep"] => Some(&[Method::POST]),
        ["api", "live", _, "whep", _] => Some(&[Method::PATCH, Method::DELETE]),
        _ => None,
    }
}

/// 요청이 앱 출처에서 왔다면 그 출처.
fn app_origin(headers: &HeaderMap) -> Option<HeaderValue> {
    let origin = headers.get(header::ORIGIN)?;
    let text = origin.to_str().ok()?;
    APP_ORIGINS
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(text))
        .then(|| origin.clone())
}

/// 앱 출처에서 앱에 연 경로·방법으로 온 요청인지. `Origin` 확인은 이런 요청을 다른 출처로 보지 않는다.
pub(super) fn allows(request: &Request) -> bool {
    app_origin(request.headers()).is_some()
        && app_methods(request.uri().path())
            .is_some_and(|methods| methods.contains(request.method()))
}

/// 앱 출처 요청에 허용 헤더를 붙이고, 사전 요청(OPTIONS)에는 바로 답한다.
pub(super) async fn allow_app(request: Request, next: Next) -> Response {
    let (Some(origin), Some(methods)) = (
        app_origin(request.headers()),
        app_methods(request.uri().path()),
    ) else {
        return next.run(request).await;
    };

    if request.method() == Method::OPTIONS {
        let requested = request
            .headers()
            .get(header::ACCESS_CONTROL_REQUEST_METHOD)
            .and_then(|v| Method::from_bytes(v.as_bytes()).ok());
        if requested.is_some_and(|m| methods.contains(&m)) {
            return preflight(origin, methods);
        }
        return next.run(request).await;
    }

    if !methods.contains(request.method()) {
        return next.run(request).await;
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static(EXPOSED_HEADERS),
    );
    headers.append(header::VARY, HeaderValue::from_static("origin"));
    response
}

/// 사전 요청의 답: 204와 허용 출처·방법·헤더.
fn preflight(origin: HeaderValue, methods: &[Method]) -> Response {
    let allowed_methods = methods
        .iter()
        .map(Method::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let mut response = (StatusCode::NO_CONTENT, Body::empty()).into_response();
    let headers = response.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_str(&allowed_methods).expect("방법 이름은 ASCII다"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static(ALLOWED_HEADERS),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("600"),
    );
    headers.append(header::VARY, HeaderValue::from_static("origin"));
    response
}
