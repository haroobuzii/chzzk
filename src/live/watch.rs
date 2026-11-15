//! 시청 연결(WHEP 중계): 방송 중인 라이브 방송만 MediaMTX에 넘긴다. 시청에는 키가 없다.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
    response::Response,
    routing::{patch, post},
    Router,
};

use super::{find_live, is_session_id, relay_offer, relay_session, LiveState, Relay};
use crate::{
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
    let (id, access) = match find_live(&state, &segment).await {
        Ok(found) => found,
        Err(response) => return response,
    };
    if access.status != LiveStatus::Live {
        return failure(
            StatusCode::CONFLICT,
            "not_live",
            "지금 방송 중이 아니에요. 목록에서 다시 확인해 주세요.",
        );
    }
    relay_offer(&state, id, Relay::Whep, &headers, body).await
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
    let id = match find_live(&state, &segment).await {
        Ok((id, _)) => id,
        Err(response) => return response,
    };
    relay_session(&state, id, Relay::Whep, &session, method, &headers, body).await
}
