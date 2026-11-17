//! 상태 변경 요청의 `Origin` 확인(CSRF 막기).
//!
//! 쿠키의 `SameSite=Lax`에 더해, POST·PUT·PATCH·DELETE 요청이 요청 주소와 같은 출처에서 왔는지 본다.
//! 다르면 쿠키가 있어도 403이다. 출처 헤더가 없는 요청(테스트, curl)은 쿠키 규칙만 따른다.

use axum::{
    extract::Request,
    http::{header, HeaderMap, Method},
    middleware::{self, Next},
    response::Response,
    Router,
};

use super::forbidden;

/// 라우터 전체에 `Origin` 확인을 씌운다. 서버는 모든 라우트를 합친 뒤 한 번 부른다.
pub fn guard_origin(router: Router) -> Router {
    router.layer(middleware::from_fn(check_origin))
}

async fn check_origin(request: Request, next: Next) -> Response {
    let changes_state = matches!(
        *request.method(),
        Method::POST | Method::PATCH | Method::DELETE | Method::PUT
    );
    if changes_state && !origin_allowed(request.headers()) {
        return forbidden();
    }
    next.run(request).await
}

/// 같은 출처 요청인지 본다.
///
/// 1. 브라우저가 `Sec-Fetch-Site`를 붙였으면 그 판단을 따른다. `same-origin`만 받는다.
///    이 헤더는 웹 페이지가 지어낼 수 없다. 개발 서버(`dx serve`)는 요청을 넘기면서 `Host`를
///    내부 주소(127.0.0.1:임의 포트)로 바꾸므로, 그 뒤에서는 `Origin`과 `Host`를 비교할 수 없다.
/// 2. 없으면(오래된 브라우저) `Origin`이 있을 때 요청 주소(`Host`와 HTTPS 여부)와 같은지 본다.
/// 3. 둘 다 없는 요청(테스트, curl)은 통과시키고 쿠키 규칙만 따른다.
fn origin_allowed(headers: &HeaderMap) -> bool {
    if let Some(site) = headers.get("sec-fetch-site") {
        return site.as_bytes() == b"same-origin";
    }
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let scheme = if is_https(headers) { "https" } else { "http" };
    origin
        .to_str()
        .is_ok_and(|origin| origin.eq_ignore_ascii_case(&format!("{scheme}://{host}")))
}

/// 요청이 HTTPS로 들어왔는지. 터널은 `X-Forwarded-Proto: https`를 보낸다.
pub(super) fn is_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|proto| proto.eq_ignore_ascii_case("https"))
}
