//! 시청 연결(WHEP 중계): 방송 중인 라이브 방송만 MediaMTX에 넘긴다. 시청에는 키가 없다.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::Response,
    routing::{patch, post},
    Router,
};

use super::{
    finish, forward, is_session_id, mediamtx, mediamtx_failed, relay_headers,
    session_from_location, store, LiveState,
};
use crate::{
    number::parse_number,
    routes::{failure, not_found},
    LiveStatus,
};

/// 시청 연결 라우트.
pub fn routes() -> Router<LiveState> {
    Router::new()
        .route("/api/live/{number}/whep", post(watch))
        .route(
            "/api/live/{number}/whep/{session}",
            patch(watch_session).delete(watch_session),
        )
}

/// `POST /api/live/{번호}/whep`: 방송 중이면 SDP offer를 MediaMTX에 넘기고 answer를 돌려준다.
///
/// 준비·종료·버려짐은 409, 없는 번호와 녹화 방송은 404다.
/// `Location`(세션 주소)은 우리 서버의 `/api/live/{번호}/whep/{세션}`으로 바꾼다.
async fn watch(
    State(state): State<LiveState>,
    Path(segment): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (id, status) = match find(&state, &segment).await {
        Ok(found) => found,
        Err(response) => return response,
    };
    if status != LiveStatus::Live {
        return failure(
            StatusCode::CONFLICT,
            "not_live",
            "지금 방송 중이 아니에요. 목록에서 다시 확인해 주세요.",
        );
    }

    let path = format!("/{}/whep", mediamtx::path_of(id));
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
        let location = format!("/api/live/{id}/whep/{secret}");
        if let Ok(value) = HeaderValue::from_str(&location) {
            response.headers_mut().insert(header::LOCATION, value);
        }
    }
    finish(response, upstream).await
}

/// `PATCH`·`DELETE /api/live/{번호}/whep/{세션}`: 시청 세션 요청을 MediaMTX에 넘긴다.
///
/// PATCH는 ICE 후보 추가, DELETE는 연결 끊기다. 방송이 끝난 뒤에도 연결을 끊을 수 있게 상태와 상관없이 넘긴다.
async fn watch_session(
    State(state): State<LiveState>,
    Path((segment, session)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !is_session_id(&session) {
        return not_found();
    }
    let id = match find(&state, &segment).await {
        Ok((id, _)) => id,
        Err(response) => return response,
    };

    let path = format!("/{}/whep/{session}", mediamtx::path_of(id));
    let pass = [header::CONTENT_TYPE, header::IF_MATCH];
    let upstream = match forward(&state, method, &path, &headers, &pass, body).await {
        Ok(upstream) => upstream,
        Err(e) => return mediamtx_failed(&e.to_string()),
    };
    let response = relay_headers(&upstream, &[header::CONTENT_TYPE, header::ETAG]);
    finish(response, upstream).await
}

/// 방송 번호로 라이브 방송의 상태를 찾는다. 번호가 아니거나 라이브 방송이 없으면 404다.
async fn find(state: &LiveState, segment: &str) -> Result<(i64, LiveStatus), Response> {
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
    Ok((id, access.status))
}
