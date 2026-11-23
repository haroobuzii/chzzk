//! 상태 확인 주소 `GET /healthz`. 운영자와 자동 배포가 서버가 살아 있는지 본다.
//!
//! - DB에 간단한 질의가 되면 200, 안 되면 503이다.
//! - MediaMTX 관리 API에 닿지 않아도 녹화·로그인은 되므로 200을 유지하고 `mediamtx: "unreachable"`로 알린다.
//! - 로그인 없이 부를 수 있고, 캐시하지 않는다.

use std::time::Duration;

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::json;
use sqlx::PgPool;

use crate::live::mediamtx::MediaMtx;

/// 빌드할 때 `CHZZK_VERSION` 환경 변수로 넣은 커밋 해시. 없으면 `dev`.
pub const BUILD_VERSION: &str = match option_env!("CHZZK_VERSION") {
    Some(version) => version,
    None => "dev",
};

/// 각 확인을 기다리는 최대 시간. 상태 확인 자체가 오래 걸려 자동 배포가 멈추지 않게 한다.
const CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// 상태 확인이 보는 것: DB, MediaMTX, 버전.
#[derive(Clone)]
pub struct HealthState {
    pub pool: PgPool,
    pub mediamtx: MediaMtx,
    pub version: &'static str,
}

/// 상태 확인 라우터.
pub fn api(state: HealthState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .with_state(state)
}

/// `GET /healthz`: `{status, db, mediamtx, version}`.
async fn healthz(State(state): State<HealthState>) -> Response {
    let (db_ok, mediamtx_ok) = tokio::join!(db_ok(&state.pool), mediamtx_ok(&state.mediamtx));
    let status = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let body = json!({
        "status": if db_ok { "ok" } else { "down" },
        "db": if db_ok { "ok" } else { "down" },
        "mediamtx": if mediamtx_ok { "ok" } else { "unreachable" },
        "version": state.version,
    });
    (status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

async fn db_ok(pool: &PgPool) -> bool {
    let query = sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(pool);
    matches!(tokio::time::timeout(CHECK_TIMEOUT, query).await, Ok(Ok(1)))
}

async fn mediamtx_ok(mediamtx: &MediaMtx) -> bool {
    matches!(
        tokio::time::timeout(CHECK_TIMEOUT, mediamtx.is_reachable()).await,
        Ok(true)
    )
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
        response::Response,
    };
    use serde_json::Value;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use super::{api, HealthState};
    use crate::live::fake_mediamtx::FakeMediaMtx;

    async fn get_healthz(state: HealthState) -> Response {
        let request = Request::get("/healthz").body(Body::empty()).unwrap();
        api(state).oneshot(request).await.unwrap()
    }

    async fn body_json(response: Response) -> Value {
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
    }

    #[sqlx::test]
    async fn healthy_server_reports_ok(pool: PgPool) {
        let fake = FakeMediaMtx::start().await;
        let response = get_healthz(HealthState {
            pool,
            mediamtx: fake.client(),
            version: "abc1234",
        })
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["status"], "ok");
        assert_eq!(body["db"], "ok");
        assert_eq!(body["mediamtx"], "ok");
    }
}
