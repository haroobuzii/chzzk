//! 서버가 켜질 때 한 번 준비하는 것들: 환경 변수 읽기, DB 접속, 라우터 만들기.

use std::sync::OnceLock;

use anyhow::anyhow;
use dioxus::server::axum::Router;
use sqlx::PgPool;

use crate::config::Config;

static CONFIG: OnceLock<Config> = OnceLock::new();
static POOL: OnceLock<PgPool> = OnceLock::new();

/// 서버 라우터를 만든다. 처음 불릴 때 환경 변수를 읽고 DB에 접속한다.
///
/// 개발 중에는 코드를 고칠 때마다 다시 불리므로, DB 접속은 한 번만 한다.
pub async fn router() -> anyhow::Result<Router> {
    if POOL.get().is_none() {
        // 로컬에서는 .env 파일에서 환경 변수를 받는다.
        let _ = dotenvy::dotenv();

        let config = Config::from_env()?;
        let pool = crate::db::connect(&config.database_url)
            .await
            .map_err(|e| anyhow!("DB에 접속하지 못했습니다: {e}"))?;
        let _ = CONFIG.set(config);
        let _ = POOL.set(pool);
    }

    Ok(dioxus::server::router(crate::App))
}

pub fn pool() -> &'static PgPool {
    POOL.get().expect("router()가 먼저 DB에 접속해야 합니다")
}

/// 서버 설정. 등록과 영상 전송에서 보관 폴더와 최대 크기를 읽을 때 쓴다.
#[allow(dead_code)] // 등록 기능(다음 티켓)에서 쓴다.
pub fn config() -> &'static Config {
    CONFIG.get().expect("router()가 먼저 설정을 읽어야 합니다")
}
