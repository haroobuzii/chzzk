//! 테스트용 가짜 MediaMTX. 임의 포트에 띄운 작은 axum 서버다.
//!
//! 하는 일은 셋이다.
//! - 받은 WHIP/WHEP·관리 API 요청을 모두 기록한다.
//! - 정해진 SDP answer와 `Location`을 돌려준다.
//! - 경로 목록 API(`/v3/paths/list`)에서 "지금 영상이 들어오는 경로"를 테스트가 정한 대로 돌려준다.
//!
//! 쓰는 법:
//! ```ignore
//! let fake = FakeMediaMtx::start().await;
//! let mediamtx = fake.client();          // 우리 서버가 쓸 MediaMTX 연결
//! fake.set_online(&["live/1"]);          // live/1에 영상이 들어오는 것처럼
//! fake.set_unreachable(true);            // MediaMTX가 꺼진 것처럼(모든 요청에 500)
//! assert!(fake.requests().is_empty());   // 넘어간 요청이 없는지
//! ```

use std::sync::{Arc, Mutex};

use axum::{
    body::Bytes,
    extract::State,
    http::{header, HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    Router,
};
use serde_json::json;

use super::mediamtx::MediaMtx;

/// 가짜가 돌려주는 SDP answer.
pub const FAKE_ANSWER: &str = "v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\ns=fake-answer\r\n";

/// 가짜가 만드는 세션 비밀값. `Location`의 마지막 조각이다.
pub const FAKE_SECRET: &str = "5f3c1a2e-0000-4000-8000-00000000abcd";

/// 가짜가 만드는 세션 번호(`ID` 헤더, 경로 목록의 `source.id`, 강제 종료 API의 번호).
pub const FAKE_SESSION_ID: &str = "9a1b2c3d-0000-4000-8000-000000001234";

/// 가짜가 받은 요청 하나.
#[derive(Clone, Debug)]
pub struct CapturedRequest {
    pub method: Method,
    /// 쿼리를 뺀 경로. 예: `/live/1/whip`
    pub path: String,
    pub headers: HeaderMap,
    pub body: String,
}

#[derive(Default)]
struct Inner {
    requests: Vec<CapturedRequest>,
    online: Vec<String>,
    unreachable: bool,
}

#[derive(Clone)]
pub struct FakeMediaMtx {
    /// 가짜 서버 주소. 예: `http://127.0.0.1:54321`
    pub url: String,
    inner: Arc<Mutex<Inner>>,
}

impl FakeMediaMtx {
    /// 임의 포트에 가짜를 띄운다. 테스트가 끝나면 런타임과 함께 꺼진다.
    pub async fn start() -> Self {
        let inner = Arc::new(Mutex::new(Inner::default()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().fallback(handle).with_state(inner.clone());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, inner }
    }

    /// 이 가짜를 가리키는 MediaMTX 연결. WebRTC 주소와 관리 API 주소가 모두 이 가짜다.
    pub fn client(&self) -> MediaMtx {
        MediaMtx::new(&self.url, &self.url)
    }

    /// 지금까지 받은 요청 전부.
    pub fn requests(&self) -> Vec<CapturedRequest> {
        self.inner.lock().unwrap().requests.clone()
    }

    /// 관리 API가 아닌 WHIP/WHEP 요청만.
    pub fn webrtc_requests(&self) -> Vec<CapturedRequest> {
        self.requests()
            .into_iter()
            .filter(|r| !r.path.starts_with("/v3/"))
            .collect()
    }

    /// 지금 영상이 들어오는 경로를 정한다. 예: `&["live/1"]`
    pub fn set_online(&self, paths: &[&str]) {
        self.inner.lock().unwrap().online = paths.iter().map(|p| p.to_string()).collect();
    }

    /// true면 모든 요청에 500으로 답한다. MediaMTX가 고장 난 것처럼 보이게 한다.
    pub fn set_unreachable(&self, unreachable: bool) {
        self.inner.lock().unwrap().unreachable = unreachable;
    }
}

async fn handle(
    State(inner): State<Arc<Mutex<Inner>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = uri.path().to_string();
    let mut inner = inner.lock().unwrap();
    inner.requests.push(CapturedRequest {
        method: method.clone(),
        path: path.clone(),
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    });
    if inner.unreachable {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "가짜 MediaMTX 고장");
    }

    if path.starts_with("/v3/paths/list") && method == Method::GET {
        return paths_list(&inner.online);
    }
    if let Some(id) = path.strip_prefix("/v3/webrtc/sessions/kick/") {
        return if id == FAKE_SESSION_ID {
            ok()
        } else {
            error(StatusCode::NOT_FOUND, "session not found")
        };
    }

    // WHIP/WHEP: `/{경로}/whip` 또는 `/{경로}/whip/{비밀값}`
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let endpoint = parts.iter().rposition(|p| *p == "whip" || *p == "whep");
    let Some(endpoint) = endpoint else {
        return error(StatusCode::NOT_FOUND, "not found");
    };
    let has_session = endpoint + 1 < parts.len();
    match (method, has_session) {
        (Method::POST, false) => {
            let location = format!("/{}/{}", parts[..=endpoint].join("/"), FAKE_SECRET);
            (
                StatusCode::CREATED,
                [
                    (header::CONTENT_TYPE, "application/sdp".to_string()),
                    (header::LOCATION, location),
                    (header::ETAG, "*".to_string()),
                    (
                        header::HeaderName::from_static("id"),
                        FAKE_SESSION_ID.to_string(),
                    ),
                ],
                FAKE_ANSWER,
            )
                .into_response()
        }
        (Method::PATCH, true) => StatusCode::NO_CONTENT.into_response(),
        (Method::DELETE, true) => ok(),
        _ => error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed"),
    }
}

fn paths_list(online: &[String]) -> Response {
    let items: Vec<_> = online
        .iter()
        .map(|name| {
            json!({
                "name": name,
                "confName": "all_others",
                "ready": true,
                "available": true,
                "online": true,
                "source": { "type": "webRTCSession", "id": FAKE_SESSION_ID },
                "readers": [],
            })
        })
        // 송출자가 없는 경로도 하나 섞는다. online이 false인 경로는 영상이 들어오는 경로가 아니다.
        .chain(std::iter::once(json!({
            "name": "live/999999",
            "confName": "all_others",
            "ready": false,
            "available": false,
            "online": false,
            "source": null,
            "readers": [],
        })))
        .collect();
    axum::Json(json!({ "itemCount": items.len(), "pageCount": 1, "items": items })).into_response()
}

fn ok() -> Response {
    axum::Json(json!({ "status": "ok" })).into_response()
}

fn error(status: StatusCode, message: &str) -> Response {
    (
        status,
        axum::Json(json!({ "status": "error", "error": message })),
    )
        .into_response()
}
