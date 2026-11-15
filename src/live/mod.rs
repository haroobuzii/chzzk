//! 라이브 방송: 시작, 송출 연결(WHIP 중계), 상태 감시기.
//!
//! 휴대폰은 MediaMTX에 직접 닿지 않고 우리 서버를 거친다. 우리 서버가 송출 키를 확인한 뒤에만
//! 요청을 MediaMTX에 넘긴다(ADR 0004, 0005).

pub mod key;
pub mod mediamtx;
pub mod store;
pub mod watcher;

#[cfg(test)]
pub mod fake_mediamtx;
#[cfg(test)]
pub(crate) mod tests;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::json;
use sqlx::PgPool;

use crate::{
    number::parse_number,
    routes::{bad_request, failure, not_found},
    title::validate_title,
    LiveStatus,
};
use mediamtx::MediaMtx;
use store::LiveAccess;

/// 라이브 라우트들이 함께 쓰는 것: DB와 MediaMTX 연결.
#[derive(Clone)]
pub struct LiveState {
    pub pool: PgPool,
    pub mediamtx: MediaMtx,
}

/// 라이브 방송 API 라우터.
pub fn api(state: LiveState) -> Router {
    Router::new()
        .route("/api/live", post(start))
        .route("/api/live/{number}/whip", post(publish))
        .route(
            "/api/live/{number}/whip/{session}",
            axum::routing::patch(publish_session).delete(publish_session),
        )
        .route("/api/live/{number}/end", post(end))
        .with_state(state)
}

/// `POST /api/live/{번호}/end`: 송출 키를 확인한 뒤 방송을 끝낸다.
///
/// 방송 중이면 종료, 준비면 버려짐이 되고 204다. 이미 끝난 방송도 204이고 그대로 둔다.
/// 그다음 MediaMTX의 송출 세션도 끊는다. 실패해도 무시한다(경로가 남아 있어도 상태는 되돌아가지 않는다).
async fn end(
    State(state): State<LiveState>,
    Path(segment): Path<String>,
    headers: HeaderMap,
) -> Response {
    let id = match authorize(&state, &segment, &headers).await {
        Ok((id, _)) => id,
        Err(response) => return response,
    };
    if let Err(e) = store::end(&state.pool, id, chrono::Utc::now()).await {
        dioxus::logger::tracing::error!("라이브 방송 종료 실패: {e}");
        return failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "방송을 끝내지 못했어요. 잠시 뒤 다시 시도해 주세요.",
        );
    }
    if let Err(e) = state.mediamtx.kick_publisher(id).await {
        dioxus::logger::tracing::warn!("송출 세션 끊기 실패(무시): {e:#}");
    }
    StatusCode::NO_CONTENT.into_response()
}

/// `POST /api/live`: JSON `{title}`로 라이브 방송을 준비 상태로 만든다.
///
/// 성공하면 201과 `{id, key}`를 준다. 송출 키 원문은 이 응답에만 담기고 DB에는 해시만 남는다.
async fn start(State(state): State<LiveState>, body: Bytes) -> Response {
    let Ok(input) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return bad_request("bad_request", "요청을 읽지 못했어요. 다시 시도해 주세요.");
    };
    let raw_title = input["title"].as_str().unwrap_or("");
    let title = match validate_title(raw_title) {
        Ok(title) => title,
        Err(e) => return bad_request(e.code(), e.message()),
    };

    let key = key::generate();
    match store::create(&state.pool, &title, &key::hash(&key)).await {
        Ok(id) => (StatusCode::CREATED, Json(json!({ "id": id, "key": key }))).into_response(),
        Err(e) => {
            dioxus::logger::tracing::error!("라이브 방송 시작 실패: {e}");
            failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "방송을 시작하지 못했어요. 잠시 뒤 다시 시도해 주세요.",
            )
        }
    }
}

/// `POST /api/live/{번호}/whip`: 송출 키를 확인한 뒤 SDP offer를 MediaMTX에 넘기고 answer를 돌려준다.
///
/// `Location`(세션 주소)은 우리 서버의 `/api/live/{번호}/whip/{세션}`으로 바꾼다.
async fn publish(
    State(state): State<LiveState>,
    Path(segment): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (id, access) = match authorize(&state, &segment, &headers).await {
        Ok(found) => found,
        Err(response) => return response,
    };
    if access.status != LiveStatus::Ready {
        return failure(
            StatusCode::CONFLICT,
            "not_ready",
            "이미 시작했거나 끝난 방송이에요. 새 방송을 시작해 주세요.",
        );
    }

    let path = format!("/{}/whip", mediamtx::path_of(id));
    let forwarded = forward(
        &state,
        Method::POST,
        &path,
        &headers,
        &[header::CONTENT_TYPE],
        body,
    )
    .await;
    let upstream = match forwarded {
        Ok(upstream) if upstream.status() == reqwest::StatusCode::CREATED => upstream,
        Ok(upstream) => return mediamtx_failed(&format!("{} 응답", upstream.status())),
        Err(e) => return mediamtx_failed(&e.to_string()),
    };

    let mut response = relay_headers(&upstream, &[header::CONTENT_TYPE, header::ETAG]);
    if let Some(secret) = upstream
        .headers()
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .and_then(session_from_location)
    {
        let location = format!("/api/live/{id}/whip/{secret}");
        if let Ok(value) = HeaderValue::from_str(&location) {
            response.headers_mut().insert(header::LOCATION, value);
        }
    }
    finish(response, upstream).await
}

/// `PATCH`·`DELETE /api/live/{번호}/whip/{세션}`: 송출 키를 확인한 뒤 송출 세션 요청을 MediaMTX에 넘긴다.
///
/// PATCH는 ICE 후보 추가, DELETE는 연결 끊기다. 방송 상태와 상관없이 넘긴다.
async fn publish_session(
    State(state): State<LiveState>,
    Path((segment, session)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !is_session_id(&session) {
        return not_found();
    }
    let id = match authorize(&state, &segment, &headers).await {
        Ok((id, _)) => id,
        Err(response) => return response,
    };

    let path = format!("/{}/whip/{session}", mediamtx::path_of(id));
    let pass = [header::CONTENT_TYPE, header::IF_MATCH];
    let upstream = match forward(&state, method, &path, &headers, &pass, body).await {
        Ok(upstream) => upstream,
        Err(e) => return mediamtx_failed(&e.to_string()),
    };
    let response = relay_headers(&upstream, &[header::CONTENT_TYPE, header::ETAG]);
    finish(response, upstream).await
}

/// 방송 번호와 송출 키를 확인한다.
///
/// 번호가 아니거나 라이브 방송이 없으면 404, 키가 없거나 틀리면 401이다.
async fn authorize(
    state: &LiveState,
    segment: &str,
    headers: &HeaderMap,
) -> Result<(i64, LiveAccess), Response> {
    let id = parse_number(segment).ok_or_else(not_found)?;
    let access = store::find_access(&state.pool, id)
        .await
        .map_err(|e| {
            dioxus::logger::tracing::error!("라이브 방송 조회 실패: {e}");
            failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "방송을 불러오지 못했어요. 잠시 뒤 다시 시도해 주세요.",
            )
        })?
        .ok_or_else(not_found)?;

    let given = bearer(headers).unwrap_or("");
    if given.is_empty() || !key::matches(given, &access.stream_key_hash) {
        return Err(failure(
            StatusCode::UNAUTHORIZED,
            "bad_stream_key",
            "송출 키가 맞지 않아요. 이 방송을 시작한 화면에서만 송출할 수 있어요.",
        ));
    }
    Ok((id, access))
}

/// `Authorization: Bearer <송출 키>`에서 키를 꺼낸다.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

/// MediaMTX의 세션 비밀값 모양(UUID)인지 본다. 그 밖의 값은 MediaMTX의 다른 주소로 새지 않게 막는다.
fn is_session_id(session: &str) -> bool {
    !session.is_empty()
        && session.len() <= 64
        && session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// MediaMTX가 준 `Location`(예: `/live/3/whip/<비밀값>?쿼리`)에서 세션 비밀값을 꺼낸다.
fn session_from_location(location: &str) -> Option<&str> {
    let without_query = location.split('?').next()?;
    let secret = without_query.rsplit('/').next()?;
    is_session_id(secret).then_some(secret)
}

/// 요청 헤더 중 `pass`에 있는 것만 담아 MediaMTX에 넘긴다. 송출 키는 넘기지 않는다.
async fn forward(
    state: &LiveState,
    method: Method,
    path: &str,
    headers: &HeaderMap,
    pass: &[HeaderName],
    body: Bytes,
) -> reqwest::Result<reqwest::Response> {
    let mut sent = HeaderMap::new();
    for name in pass {
        if let Some(value) = headers.get(name) {
            sent.insert(name.clone(), value.clone());
        }
    }
    state
        .mediamtx
        .forward(method, path, sent, body.to_vec())
        .await
}

/// MediaMTX 응답의 상태 코드와 `names` 헤더만 옮긴 응답(본문은 `finish`가 채운다).
fn relay_headers(upstream: &reqwest::Response, names: &[HeaderName]) -> Response {
    let mut response = Response::new(axum::body::Body::empty());
    *response.status_mut() = upstream.status();
    for name in names {
        if let Some(value) = upstream.headers().get(name) {
            response.headers_mut().insert(name.clone(), value.clone());
        }
    }
    response
}

/// MediaMTX 응답 본문을 읽어 `response`에 담는다.
async fn finish(mut response: Response, upstream: reqwest::Response) -> Response {
    match upstream.bytes().await {
        Ok(bytes) => {
            *response.body_mut() = axum::body::Body::from(bytes);
            response
        }
        Err(e) => mediamtx_failed(&e.to_string()),
    }
}

/// MediaMTX에 닿지 못했거나 MediaMTX가 거절한 경우.
fn mediamtx_failed(reason: &str) -> Response {
    dioxus::logger::tracing::warn!("MediaMTX 요청 실패: {reason}");
    failure(
        StatusCode::BAD_GATEWAY,
        "mediamtx_failed",
        "송출 서버에 연결하지 못했어요. 잠시 뒤 다시 시도해 주세요.",
    )
}
