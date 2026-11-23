//! 환경 변수에서 읽는 서버 설정.

use std::path::PathBuf;

use anyhow::Context;

use crate::donations::toss::TossConfig;

/// 업로드 최대 크기 기본값: 2 GiB.
const DEFAULT_MAX_UPLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Config {
    /// PostgreSQL 접속 주소. 반드시 있어야 한다.
    pub database_url: String,
    /// 영상 보관 폴더. 기본 `./data`.
    pub data_dir: PathBuf,
    /// 업로드 최대 크기(바이트). 기본 2 GiB.
    pub max_upload_bytes: u64,
    /// MediaMTX의 WebRTC HTTP(WHIP/WHEP) 주소. 기본 `http://localhost:8889`.
    pub mediamtx_webrtc_url: String,
    /// MediaMTX의 관리 API 주소. 기본 `http://localhost:9997`.
    pub mediamtx_api_url: String,
    /// MediaMTX 경로 앞부분. 기본 `live`. 작업 폴더마다 다르게 두면 MediaMTX를 함께 써도 방송이 섞이지 않는다.
    pub live_path_prefix: String,
    /// 토스 테스트 키와 API 설정. 키가 둘 다 없으면 `None`이고 후원 기능이 꺼진다.
    pub toss: Option<TossConfig>,
    /// iOS 앱 TestFlight 공개 링크. 없으면 다운로드 페이지에 "준비 중"으로 나온다.
    pub testflight_url: Option<String>,
}

impl Config {
    /// `DATABASE_URL`, `DATA_DIR`, `MAX_UPLOAD_BYTES`, `MEDIAMTX_WEBRTC_URL`, `MEDIAMTX_API_URL`,
    /// `LIVE_PATH_PREFIX`, `TOSS_CLIENT_KEY`, `TOSS_SECRET_KEY`, `TOSS_API_URL`, `TOSS_TEST_FAIL_CODE`,
    /// `TESTFLIGHT_URL`을 읽는다.
    /// 비어 있거나 없는 값은 기본값을 쓴다(`DATABASE_URL` 제외).
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = read("DATABASE_URL").context("DATABASE_URL 환경 변수가 필요합니다")?;
        let data_dir = read("DATA_DIR").map_or_else(|| PathBuf::from("./data"), PathBuf::from);
        let max_upload_bytes = match read("MAX_UPLOAD_BYTES") {
            None => DEFAULT_MAX_UPLOAD_BYTES,
            Some(value) => value.parse().with_context(|| {
                format!("MAX_UPLOAD_BYTES는 바이트 단위 숫자여야 합니다: {value}")
            })?,
        };
        let live_path_prefix = read("LIVE_PATH_PREFIX")
            .unwrap_or_else(|| crate::live::mediamtx::DEFAULT_PATH_PREFIX.into());
        if !crate::live::mediamtx::is_valid_path_prefix(&live_path_prefix) {
            anyhow::bail!(
                "LIVE_PATH_PREFIX는 영문 소문자, 숫자, -, _만 쓸 수 있습니다: {live_path_prefix}"
            );
        }
        let toss = TossConfig::from_values(
            read("TOSS_CLIENT_KEY"),
            read("TOSS_SECRET_KEY"),
            read("TOSS_API_URL"),
            read("TOSS_TEST_FAIL_CODE"),
        )
        .map_err(anyhow::Error::msg)?;
        Ok(Self {
            database_url,
            data_dir,
            max_upload_bytes,
            mediamtx_webrtc_url: read("MEDIAMTX_WEBRTC_URL")
                .unwrap_or_else(|| "http://localhost:8889".into()),
            mediamtx_api_url: read("MEDIAMTX_API_URL")
                .unwrap_or_else(|| "http://localhost:9997".into()),
            live_path_prefix,
            toss,
            testflight_url: read("TESTFLIGHT_URL"),
        })
    }
}

/// 환경 변수를 읽는다. 없거나 공백뿐이면 `None`.
fn read(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
