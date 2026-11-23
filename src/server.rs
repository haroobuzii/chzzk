//! 서버가 켜질 때 한 번 준비하는 것들: 환경 변수 읽기, DB 접속, 라우터 만들기.

use std::sync::OnceLock;

use anyhow::anyhow;
use dioxus::server::axum::Router;
use sqlx::PgPool;

use crate::config::Config;
use crate::live::{mediamtx::MediaMtx, LiveState};

static CONFIG: OnceLock<Config> = OnceLock::new();
static POOL: OnceLock<PgPool> = OnceLock::new();
static MEDIAMTX: OnceLock<MediaMtx> = OnceLock::new();

/// 서버 라우터를 만든다. 처음 불릴 때 환경 변수를 읽고 DB에 접속한다.
///
/// 개발 중에는 코드를 고칠 때마다 다시 불리므로, DB 접속은 한 번만 한다.
pub async fn router() -> anyhow::Result<Router> {
    if POOL.get().is_none() {
        // 로컬에서는 .env 파일에서 환경 변수를 받는다.
        let _ = dotenvy::dotenv();

        let config = Config::from_env()?;
        if config.toss.is_none() {
            log::warn!("TOSS_CLIENT_KEY·TOSS_SECRET_KEY가 없어 후원 기능을 끈 채로 켭니다.");
        }
        let pool = crate::db::connect(&config.database_url)
            .await
            .map_err(|e| anyhow!("DB에 접속하지 못했습니다: {e}"))?;
        let mediamtx = MediaMtx::with_path_prefix(
            &config.mediamtx_webrtc_url,
            &config.mediamtx_api_url,
            &config.live_path_prefix,
        );
        // 상태 감시기는 한 번만 띄운다. 켜지자마자 한 번 맞추므로, 꺼진 동안 바뀐 상태도 바로 맞춰진다.
        crate::live::status_sync::spawn(pool.clone(), mediamtx.clone());
        // 만료 정리도 한 번만 띄운다. 켜지자마자 한 번 정리하므로, 꺼진 동안 지난 주문서도 바로 만료된다.
        crate::donations::expiry::spawn(pool.clone());
        let _ = CONFIG.set(config);
        let _ = POOL.set(pool);
        let _ = MEDIAMTX.set(mediamtx);
    }

    let state = crate::routes::AppState::new(pool().clone(), config());
    let live = LiveState {
        pool: pool().clone(),
        mediamtx: MEDIAMTX.get().expect("위에서 정했다").clone(),
    };
    let manage = crate::manage::ManageState {
        pool: pool().clone(),
        videos: crate::videos::VideoStore::new(&config().data_dir),
        mediamtx: MEDIAMTX.get().expect("위에서 정했다").clone(),
    };
    let donations = crate::donations::DonationsState {
        pool: pool().clone(),
        toss: config().toss.clone(),
    };
    let health = crate::health::HealthState {
        pool: pool().clone(),
        mediamtx: MEDIAMTX.get().expect("위에서 정했다").clone(),
        version: crate::health::BUILD_VERSION,
    };
    // Dioxus가 만든 화면·서버 함수 라우터에 로그인 API, 등록 API, 라이브 API, 댓글 API, 방송 관리 API,
    // 후원 API, 상태 확인 라우트를 덧붙인다.
    // 다른 출처에서 온 상태 변경 요청은 어느 라우트든 막는다.
    Ok(crate::auth::guard_origin(
        dioxus::server::router(crate::App)
            .merge(crate::auth::api(pool().clone()))
            .merge(crate::routes::api(state))
            .merge(crate::live::api(live))
            .merge(crate::comments::api(pool().clone()))
            .merge(crate::manage::api(manage))
            .merge(crate::donations::api(donations))
            .merge(crate::health::api(health)),
    ))
}

pub fn pool() -> &'static PgPool {
    POOL.get().expect("router()가 먼저 DB에 접속해야 합니다")
}

/// 서버 설정. 등록과 영상 전송에서 보관 폴더와 최대 크기를 읽을 때 쓴다.
pub fn config() -> &'static Config {
    CONFIG.get().expect("router()가 먼저 설정을 읽어야 합니다")
}
