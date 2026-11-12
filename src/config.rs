//! 환경 변수에서 읽는 서버 설정.

use std::path::PathBuf;

use anyhow::Context;

/// 업로드 최대 크기 기본값: 2 GiB.
const DEFAULT_MAX_UPLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
#[allow(dead_code)] // 보관 폴더와 최대 크기는 등록 기능(다음 티켓)에서 쓴다.
pub struct Config {
    /// PostgreSQL 접속 주소. 반드시 있어야 한다.
    pub database_url: String,
    /// 영상 보관 폴더. 기본 `./data`.
    pub data_dir: PathBuf,
    /// 업로드 최대 크기(바이트). 기본 2 GiB.
    pub max_upload_bytes: u64,
}

impl Config {
    /// `DATABASE_URL`, `DATA_DIR`, `MAX_UPLOAD_BYTES`를 읽는다.
    /// 비어 있거나 없는 값은 기본값을 쓴다(`DATABASE_URL` 제외).
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = read("DATABASE_URL").context("DATABASE_URL 환경 변수가 필요합니다")?;
        let data_dir = read("DATA_DIR").map_or_else(|| PathBuf::from("./data"), PathBuf::from);
        let max_upload_bytes = match read("MAX_UPLOAD_BYTES") {
            None => DEFAULT_MAX_UPLOAD_BYTES,
            Some(value) => value
                .parse()
                .with_context(|| format!("MAX_UPLOAD_BYTES는 바이트 단위 숫자여야 합니다: {value}"))?,
        };
        Ok(Self { database_url, data_dir, max_upload_bytes })
    }
}

/// 환경 변수를 읽는다. 없거나 공백뿐이면 `None`.
fn read(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
