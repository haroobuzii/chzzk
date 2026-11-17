//! MediaMTX 연결: WHIP/WHEP 요청을 MediaMTX에 넘기고, 관리 API로 지금 영상이 들어오는 경로를 읽는다.
//!
//! 라이브 방송 하나는 MediaMTX 경로 `live/<방송 번호>` 하나다.

use std::collections::HashSet;

use anyhow::{bail, Context};
use axum::http::{HeaderMap, Method};
use serde::Deserialize;

/// 설정된 두 주소로 MediaMTX에 요청을 보낸다.
#[derive(Clone, Debug)]
pub struct MediaMtx {
    /// WHIP/WHEP을 받는 WebRTC HTTP 주소. 예: `http://localhost:8889`
    webrtc_url: String,
    /// 관리 API 주소. 예: `http://localhost:9997`
    api_url: String,
    http: reqwest::Client,
}

/// 방송 번호에 해당하는 MediaMTX 경로.
pub fn path_of(id: i64) -> String {
    format!("live/{id}")
}

/// MediaMTX 경로에서 방송 번호를 읽는다. `live/<번호>` 모양이 아니면 `None`.
pub fn id_of(path: &str) -> Option<i64> {
    let number = path.strip_prefix("live/")?;
    crate::number::parse_number(number)
}

impl MediaMtx {
    pub fn new(webrtc_url: &str, api_url: &str) -> Self {
        Self {
            webrtc_url: webrtc_url.trim_end_matches('/').to_string(),
            api_url: api_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// WebRTC HTTP 주소의 `path`(예: `/live/3/whip`)로 요청을 그대로 넘긴다.
    ///
    /// `headers`에는 넘길 헤더만 담아 온다(Content-Type, If-Match 등). 로그인 쿠키는 넘기지 않는다.
    pub async fn forward(
        &self,
        method: Method,
        path: &str,
        headers: HeaderMap,
        body: Vec<u8>,
    ) -> reqwest::Result<reqwest::Response> {
        self.http
            .request(method, format!("{}{path}", self.webrtc_url))
            .headers(headers)
            .body(body)
            .send()
            .await
    }

    /// 지금 영상이 들어오는(송출자가 연결된) 경로 이름 목록.
    pub async fn online_paths(&self) -> anyhow::Result<HashSet<String>> {
        Ok(self
            .paths()
            .await?
            .into_iter()
            .filter(|path| path.online)
            .map(|path| path.name)
            .collect())
    }

    /// 방송 번호 `id`의 경로에 연결된 송출 세션을 강제로 끊는다. 송출자가 없으면 아무것도 하지 않는다.
    pub async fn kick_publisher(&self, id: i64) -> anyhow::Result<()> {
        let name = path_of(id);
        let sessions: Vec<String> = self
            .paths()
            .await?
            .into_iter()
            .filter(|path| path.name == name)
            .filter_map(|path| path.source)
            .filter(|source| source.kind == "webRTCSession")
            .map(|source| source.id)
            .collect();
        for session in sessions {
            let url = format!("{}/v3/webrtc/sessions/kick/{session}", self.api_url);
            let response = self
                .http
                .post(&url)
                .send()
                .await
                .context("MediaMTX 관리 API에 닿지 못했습니다")?;
            if !response.status().is_success() {
                bail!(
                    "송출 세션 강제 종료에 MediaMTX가 {}로 응답했습니다",
                    response.status()
                );
            }
        }
        Ok(())
    }

    /// 관리 API의 경로 목록 전부(모든 쪽).
    async fn paths(&self) -> anyhow::Result<Vec<ApiPath>> {
        let mut paths = Vec::new();
        let mut page = 0;
        loop {
            let url = format!(
                "{}/v3/paths/list?page={page}&itemsPerPage=100",
                self.api_url
            );
            let response = self
                .http
                .get(&url)
                .send()
                .await
                .context("MediaMTX 관리 API에 닿지 못했습니다")?;
            if !response.status().is_success() {
                bail!("MediaMTX 관리 API가 {}로 응답했습니다", response.status());
            }
            let bytes = response.bytes().await?;
            let list: PathList =
                serde_json::from_slice(&bytes).context("MediaMTX 경로 목록을 읽지 못했습니다")?;
            paths.extend(list.items);
            page += 1;
            if page >= list.page_count {
                return Ok(paths);
            }
        }
    }
}

/// `GET /v3/paths/list` 응답 중 쓰는 칸.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathList {
    #[serde(default)]
    page_count: u64,
    #[serde(default)]
    items: Vec<ApiPath>,
}

#[derive(Deserialize)]
struct ApiPath {
    name: String,
    /// 송출자가 연결되어 영상이 들어오고 있으면 true.
    #[serde(default)]
    online: bool,
    /// 영상을 보내는 쪽. 송출자가 없으면 null.
    #[serde(default)]
    source: Option<ApiSource>,
}

#[derive(Deserialize)]
struct ApiSource {
    /// 예: `webRTCSession`
    #[serde(rename = "type")]
    kind: String,
    /// 세션 번호. 강제 종료 API에 쓴다.
    id: String,
}
