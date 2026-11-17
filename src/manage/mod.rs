//! 방송 관리: 소유자가 방송 제목을 고치고(`PATCH /api/broadcasts/{번호}`), 방송을 지운다(`DELETE`).
//!
//! 권한 검사는 [`CurrentUser`]를 거친다. 비로그인은 401, 소유자가 아니거나 소유자 없는 방송은 403,
//! 없거나 보이지 않는 방송은 404다. 방송 관리 버튼은 상세 화면의 [`OwnerActions`]다.

#[cfg(feature = "server")]
mod store;
#[cfg(test)]
mod tests;
mod ui;

pub use ui::OwnerActions;

#[cfg(feature = "server")]
pub use server::{api, ManageState};

#[cfg(feature = "server")]
mod server {
    use axum::{
        body::Bytes,
        extract::{FromRef, Path, State},
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::patch,
        Json, Router,
    };
    use serde_json::json;
    use sqlx::PgPool;

    use super::store::{self, Managed};
    use crate::{
        auth::CurrentUser,
        live::{self, mediamtx::MediaMtx},
        number::parse_number,
        routes::{bad_request, failure, not_found},
        title::validate_title,
        videos::VideoStore,
        LiveStatus,
    };

    /// 방송 관리 라우트가 함께 쓰는 것: DB, 영상 보관소, MediaMTX 연결.
    #[derive(Clone)]
    pub struct ManageState {
        pub pool: PgPool,
        pub videos: VideoStore,
        pub mediamtx: MediaMtx,
    }

    /// 현재 사용자 확인([`CurrentUser`])이 세션을 찾을 DB.
    impl FromRef<ManageState> for PgPool {
        fn from_ref(state: &ManageState) -> Self {
            state.pool.clone()
        }
    }

    /// 방송 관리 API 라우터.
    pub fn api(state: ManageState) -> Router {
        Router::new()
            .route("/api/broadcasts/{number}", patch(rename).delete(remove))
            .with_state(state)
    }

    /// `PATCH /api/broadcasts/{번호}`의 본문. 제목이 없으면 빈 제목으로 보고 제목 검사가 거절한다.
    #[derive(serde::Deserialize)]
    struct Rename {
        #[serde(default)]
        title: String,
    }

    /// `PATCH /api/broadcasts/{번호}`: JSON `{title}`로 제목을 고친다. 방송 중인 라이브 방송도 된다.
    ///
    /// 성공하면 200과 `{id, title}`(앞뒤 공백을 지운 제목)을 준다. 제목 규칙은 등록과 같다(위반은 400).
    async fn rename(
        State(state): State<ManageState>,
        current: CurrentUser,
        Path(segment): Path<String>,
        body: Bytes,
    ) -> Response {
        let (id, _) = match find_owned(&state, current, &segment).await {
            Ok(found) => found,
            Err(response) => return response,
        };
        let Ok(input) = serde_json::from_slice::<Rename>(&body) else {
            return bad_request("bad_request", "요청을 읽지 못했어요. 다시 시도해 주세요.");
        };
        let title = match validate_title(&input.title) {
            Ok(title) => title,
            Err(e) => return bad_request(e.code(), e.message()),
        };
        match store::rename(&state.pool, id, &title).await {
            Ok(true) => Json(json!({ "id": id, "title": title })).into_response(),
            // 확인한 뒤 그사이 지워졌다.
            Ok(false) => not_found(),
            Err(e) => server_error("제목 수정", e, "제목을 고치지 못했어요."),
        }
    }

    /// `DELETE /api/broadcasts/{번호}`: 방송을 지우고 204를 준다.
    ///
    /// 1. 방송 중인 라이브 방송이면 종료로 바꾸고 MediaMTX 송출 세션을 끊는다. 끊기 실패는 무시한다
    ///    (종료 API와 같다. 방송 행이 사라지면 송출 연결 요청도 404가 된다).
    /// 2. 방송 행을 지운다. 댓글은 DB가 연쇄로 지운다.
    /// 3. 녹화 방송이면 영상 파일을 지운다. 행을 먼저 지워야, 파일만 사라지고 방송이 남는 일이 없다.
    async fn remove(
        State(state): State<ManageState>,
        current: CurrentUser,
        Path(segment): Path<String>,
    ) -> Response {
        let (id, broadcast) = match find_owned(&state, current, &segment).await {
            Ok(found) => found,
            Err(response) => return response,
        };
        if broadcast.status == Some(LiveStatus::Live) {
            if let Err(e) = live::store::end(&state.pool, id, chrono::Utc::now()).await {
                return server_error("라이브 방송 종료", e, "방송을 지우지 못했어요.");
            }
            if let Err(e) = state.mediamtx.kick_publisher(id).await {
                dioxus::logger::tracing::warn!("송출 세션 끊기 실패(무시): {e:#}");
            }
        }
        match store::delete(&state.pool, id).await {
            Ok(Some(video_key)) => {
                if let Some(key) = video_key {
                    state.videos.remove(&key);
                }
                StatusCode::NO_CONTENT.into_response()
            }
            Ok(None) => not_found(),
            Err(e) => server_error("방송 삭제", e, "방송을 지우지 못했어요."),
        }
    }

    /// 로그인 → 방송 찾기 → 소유자 확인을 차례로 한다. 막히면 401·404·403 응답.
    async fn find_owned(
        state: &ManageState,
        current: CurrentUser,
        segment: &str,
    ) -> Result<(i64, Managed), Response> {
        let user = current.require()?;
        let Some(id) = parse_number(segment) else {
            return Err(not_found());
        };
        let broadcast = match store::find(&state.pool, id).await {
            Ok(Some(broadcast)) => broadcast,
            Ok(None) => return Err(not_found()),
            Err(e) => return Err(server_error("방송 조회", e, "방송을 불러오지 못했어요.")),
        };
        user.require_owner(broadcast.owner_id)?;
        Ok((id, broadcast))
    }

    fn server_error(action: &str, e: sqlx::Error, message: &str) -> Response {
        dioxus::logger::tracing::error!("{action} 실패: {e}");
        failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            &format!("{message} 잠시 뒤 다시 시도해 주세요."),
        )
    }
}
