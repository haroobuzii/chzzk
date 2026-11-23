//! MediaMTX 연결: WHIP/WHEP 요청을 MediaMTX에 넘기고, 관리 API로 지금 영상이 들어오는 경로를 읽는다.
//!
//! 라이브 방송 하나는 MediaMTX 경로 `<앞부분>/<방송 번호>` 하나다. 앞부분은 기본 `live`이고,
//! `LIVE_PATH_PREFIX`로 바꿀 수 있다. MediaMTX 하나를 여러 작업 폴더가 함께 쓸 때 경로가 겹치지 않게 하기 위해서다.

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
    /// 경로 앞부분. 기본 `live`.
    path_prefix: String,
    http: reqwest::Client,
}

/// 경로 앞부분 기본값.
pub const DEFAULT_PATH_PREFIX: &str = "live";

/// 경로 앞부분으로 쓸 수 있는지 본다. 영문 소문자, 숫자, `-`, `_`만 받는다.
pub fn is_valid_path_prefix(prefix: &str) -> bool {
    !prefix.is_empty()
        && prefix
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

impl MediaMtx {
    /// 기본 경로 앞부분(`live`)을 쓰는 연결.
    pub fn new(webrtc_url: &str, api_url: &str) -> Self {
        Self::with_path_prefix(webrtc_url, api_url, DEFAULT_PATH_PREFIX)
    }

    /// 경로 앞부분을 정한 연결. 앞부분은 `is_valid_path_prefix`를 통과한 값이어야 한다.
    pub fn with_path_prefix(webrtc_url: &str, api_url: &str, path_prefix: &str) -> Self {
        Self {
            webrtc_url: webrtc_url.trim_end_matches('/').to_string(),
            api_url: api_url.trim_end_matches('/').to_string(),
            path_prefix: path_prefix.to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// 방송 번호에 해당하는 MediaMTX 경로. 예: `live/3`
    pub fn path_of(&self, id: i64) -> String {
        format!("{}/{id}", self.path_prefix)
    }

    /// MediaMTX 경로에서 방송 번호를 읽는다. 이 연결의 앞부분으로 시작하지 않으면 `None`.
    /// 그래서 다른 작업 폴더가 쓰는 경로(예: `live-t33/2`)는 내 방송으로 읽지 않는다.
    pub fn id_of(&self, path: &str) -> Option<i64> {
        let number = path.strip_prefix(&self.path_prefix)?.strip_prefix('/')?;
        crate::number::parse_number(number)
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
        let name = self.path_of(id);
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

    /// 관리 API에 닿아 정상 응답을 받는지. 상태 확인(`/healthz`)이 쓴다.
    pub async fn is_reachable(&self) -> bool {
        let url = format!("{}/v3/paths/list?itemsPerPage=1", self.api_url);
        matches!(self.http.get(&url).send().await, Ok(response) if response.status().is_success())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn with(prefix: &str) -> MediaMtx {
        MediaMtx::with_path_prefix("http://x", "http://y", prefix)
    }

    #[test]
    fn 기본_앞부분은_live다() {
        let mediamtx = MediaMtx::new("http://x", "http://y");
        assert_eq!(mediamtx.path_of(3), "live/3");
        assert_eq!(mediamtx.id_of("live/3"), Some(3));
    }

    #[test]
    fn 다른_앞부분의_경로는_내_방송으로_읽지_않는다() {
        let mine = with("live-t33");
        assert_eq!(mine.path_of(2), "live-t33/2");
        assert_eq!(mine.id_of("live-t33/2"), Some(2));
        assert_eq!(mine.id_of("live/2"), None);
        assert_eq!(with("live").id_of("live-t33/2"), None);
        assert_eq!(with("live").id_of("live/abc"), None);
    }

    #[test]
    fn 앞부분은_소문자_숫자_하이픈_밑줄만_받는다() {
        for ok in ["live", "live-t33", "live_2"] {
            assert!(is_valid_path_prefix(ok), "{ok}");
        }
        for bad in ["", "Live", "live/x", "live t", "../x", "라이브"] {
            assert!(!is_valid_path_prefix(bad), "{bad:?}");
        }
    }
}
