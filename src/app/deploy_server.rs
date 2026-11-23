//! 앱의 서버 연결 모듈: 배포 서버의 공개 API를 Rust HTTP로 부르고, 앱이 쓸 주소를 만든다.
//!
//! 목록·단건은 웹뷰의 `fetch`가 아니라 여기서 부른다. 웹뷰의 출처가 배포 서버와 달라도 CORS가 필요 없다.

use std::fmt;
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::Broadcast;

/// 서버가 응답하지 않을 때 기다리는 최대 시간. 넘으면 연결 실패로 본다.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// 배포 서버 하나. 주소는 `https://chzzk.haroobuzii.com`처럼 끝에 `/`가 없는 출처다.
#[derive(Clone)]
pub struct DeployServer {
    base: String,
    http: reqwest::Client,
}

impl DeployServer {
    pub fn new(base: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("기본 설정의 HTTP 클라이언트는 만들 수 있다");
        Self {
            base: base.trim_end_matches('/').to_string(),
            http,
        }
    }

    /// 방송 목록(최신순).
    pub async fn broadcasts(&self) -> Result<Vec<Broadcast>, ConnectionError> {
        self.get_json("/api/broadcasts/list").await
    }

    /// 방송 번호로 방송 하나. 없으면 `None`.
    pub async fn broadcast(&self, id: i64) -> Result<Option<Broadcast>, ConnectionError> {
        self.get_json(&format!("/api/broadcasts/info?id={id}"))
            .await
    }

    /// 녹화 영상 주소. `<video src>`에 그대로 넣는다(Range 지원이라 중간으로 건너뛸 수 있다).
    pub fn video_url(&self, id: i64) -> String {
        format!("{}/api/broadcasts/{id}/video", self.base)
    }

    /// 라이브 방송 시청 연결(WHEP) 주소.
    pub fn whep_url(&self, id: i64) -> String {
        format!("{}/api/live/{id}/whep", self.base)
    }

    /// "웹에서 열기"로 기본 브라우저에서 열 방송 페이지 주소.
    pub fn web_page_url(&self, id: i64) -> String {
        format!("{}/broadcasts/{id}", self.base)
    }

    /// `path`를 GET으로 불러 JSON을 읽는다. 2xx가 아니면 본문과 상관없이 연결 실패다.
    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ConnectionError> {
        let response = self
            .http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .map_err(|_| ConnectionError)?;
        if !response.status().is_success() {
            return Err(ConnectionError);
        }
        let body = response.bytes().await.map_err(|_| ConnectionError)?;
        serde_json::from_slice(&body).map_err(|_| ConnectionError)
    }
}

/// 서버에 연결할 수 없다. 꺼졌거나, 닿지 않거나, 오류로 답했거나, 알 수 없는 답을 보냈다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectionError;

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("서버에 연결할 수 없어요. 인터넷 연결을 확인한 뒤 다시 시도해 주세요.")
    }
}
