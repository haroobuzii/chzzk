use chrono::{DateTime, FixedOffset, Utc};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(feature = "server")]
mod config;
#[cfg(feature = "server")]
mod db;
mod display;
#[cfg(feature = "server")]
mod live;
mod number;
mod publisher;
mod refresh;
#[cfg(feature = "server")]
mod routes;
#[cfg(feature = "server")]
mod server;
#[cfg(test)]
mod test_support;
#[cfg(feature = "server")]
mod title;
#[cfg(feature = "server")]
mod videos;
mod viewer;

use display::Mark;
use publisher::NewLive;
use refresh::{sleep_ms, REFRESH_MS};
use viewer::LiveDetail;

const MAIN_CSS: Asset = asset!("/assets/main.css");

fn main() {
    // 서버로 빌드할 때: 설정을 읽고 DB에 접속한 뒤 화면과 API를 함께 제공한다.
    #[cfg(feature = "server")]
    dioxus::serve(|| async move { server::router().await });

    // 브라우저(WASM)로 빌드할 때: 화면만 띄운다.
    #[cfg(not(feature = "server"))]
    dioxus::launch(App);
}

/// 방송 하나. 녹화 방송이면 영상 하나를, 라이브 방송이면 방송 상태와 시작·종료 시각을 가진다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
pub struct Broadcast {
    /// 방송 번호. DB가 1부터 매긴다. 녹화·라이브가 같은 번호 줄을 쓴다.
    pub id: i64,
    pub title: String,
    /// 등록 시각. 라이브 방송은 시작 API를 부른 시각이다.
    pub created_at: DateTime<Utc>,
    pub kind: BroadcastKind,
    /// 방송 상태. 라이브 방송만 가진다.
    pub status: Option<LiveStatus>,
    /// 시작 시각. 방송 중·종료인 라이브 방송만 가진다.
    pub started_at: Option<DateTime<Utc>>,
    /// 종료 시각. 종료된 라이브 방송만 가진다.
    pub ended_at: Option<DateTime<Utc>>,
}

/// 방송 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "server",
    derive(sqlx::Type),
    sqlx(type_name = "broadcast_kind", rename_all = "lowercase")
)]
pub enum BroadcastKind {
    /// 녹화 방송: 올린 영상 파일을 다시 본다.
    Recorded,
    /// 라이브 방송: 휴대폰이 지금 보내는 영상을 시청한다.
    Live,
}

/// 라이브 방송의 방송 상태.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "server",
    derive(sqlx::Type),
    sqlx(type_name = "live_status", rename_all = "lowercase")
)]
pub enum LiveStatus {
    /// 준비: 시작했지만 아직 영상이 들어오지 않았다. 화면에 보이지 않는다.
    Ready,
    /// 방송 중.
    Live,
    /// 종료.
    Ended,
    /// 버려짐: 준비에서 끝내 방송 중이 되지 못했다. 화면에 보이지 않는다.
    Abandoned,
}

/// 등록된 방송 목록을 최신순으로 돌려준다.
#[get("/api/broadcasts/list")]
async fn get_broadcasts() -> Result<Vec<Broadcast>, ServerFnError> {
    match db::list_broadcasts(server::pool()).await {
        Ok(broadcasts) => Ok(broadcasts),
        Err(e) => {
            dioxus::logger::tracing::error!("방송 목록 조회 실패: {e}");
            Ok(HttpError::internal_server_error(
                "방송 목록을 불러오지 못했어요. 잠시 뒤 다시 시도해 주세요.",
            )?)
        }
    }
}

/// 방송 번호로 방송 하나를 찾는다. 없으면 `None`.
#[get("/api/broadcasts/info?id")]
async fn get_broadcast(id: i64) -> Result<Option<Broadcast>, ServerFnError> {
    match db::find_broadcast(server::pool(), id).await {
        Ok(broadcast) => Ok(broadcast),
        Err(e) => {
            dioxus::logger::tracing::error!("방송 조회 실패: {e}");
            Ok(HttpError::internal_server_error(
                "방송을 불러오지 못했어요. 잠시 뒤 다시 시도해 주세요.",
            )?)
        }
    }
}

#[derive(Routable, Clone, PartialEq)]
enum Route {
    #[layout(Shell)]
    #[route("/")]
    BroadcastList {},
    #[route("/new")]
    NewBroadcast {},
    #[route("/live/new")]
    NewLive {},
    /// 방송 번호는 글자로 받아 직접 읽는다. 번호가 아닌 주소도 "방송을 찾을 수 없습니다"로 보여 주기 위해서다.
    #[route("/broadcasts/:segment")]
    BroadcastDetail { segment: String },
    #[route("/:..rest")]
    PageNotFound { rest: Vec<String> },
}

#[component]
fn App() -> Element {
    rsx! {
        document::Title { "녹화방송" }
        document::Meta { name: "viewport", content: "width=device-width, initial-scale=1" }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        Router::<Route> {}
    }
}

/// 모든 화면에 공통인 틀: 맨 위 머리글과 그 아래 본문.
#[component]
fn Shell() -> Element {
    rsx! {
        header { class: "site-header",
            Link { class: "brand", to: Route::BroadcastList {}, "녹화방송" }
            nav { class: "header-actions",
                Link { class: "button live-button", to: Route::NewLive {}, "라이브 시작" }
                Link { class: "button", to: Route::NewBroadcast {}, "녹화 올리기" }
            }
        }
        main { Outlet::<Route> {} }
    }
}

/// 첫 화면: 방송 중인 방송을 모은 "지금 라이브" 칸과 전체 방송 목록. 5초마다 다시 불러온다.
#[component]
fn BroadcastList() -> Element {
    let broadcasts = use_server_future(get_broadcasts)?;
    // 처음 목록은 서버가 그려 주고, 그 뒤로는 5초마다 새로 받은 목록으로 바꾼다.
    let mut latest = use_signal(|| None::<Vec<Broadcast>>);
    use_effect(move || {
        spawn(async move {
            loop {
                sleep_ms(REFRESH_MS).await;
                // 실패하면 지금 목록을 그대로 두고 다음에 다시 묻는다.
                if let Ok(list) = get_broadcasts().await {
                    if latest.peek().as_ref() != Some(&list) {
                        latest.set(Some(list));
                    }
                }
            }
        });
    });

    let initial = broadcasts.read();
    let list = match (latest.read().clone(), &*initial) {
        (Some(list), _) => list,
        (None, Some(Ok(list))) => list.clone(),
        (None, Some(Err(e))) => {
            return rsx! {
                p { class: "error", role: "alert", "{error_message(e)}" }
            }
        }
        (None, None) => {
            return rsx! {
                p { "불러오는 중…" }
            }
        }
    };

    if list.is_empty() {
        return rsx! {
            section { class: "empty",
                p { "아직 방송이 없습니다" }
                Link { class: "button", to: Route::NewBroadcast {}, "녹화 올리기" }
            }
        };
    }
    let live_now: Vec<Broadcast> = list
        .iter()
        .filter(|b| b.mark() == Mark::Live)
        .cloned()
        .collect();
    rsx! {
        // 방송 중인 방송이 없으면 이 칸은 보이지 않는다.
        if !live_now.is_empty() {
            section { class: "live-now",
                h2 { class: "section-title", "지금 라이브" }
                ul { class: "broadcast-grid",
                    for broadcast in live_now {
                        li { key: "{broadcast.id}", BroadcastCard { broadcast } }
                    }
                }
            }
        }
        section {
            if list.iter().any(|b| b.mark() == Mark::Live) {
                h2 { class: "section-title", "전체 방송" }
            }
            ul { class: "broadcast-grid",
                for broadcast in list {
                    li { key: "{broadcast.id}", BroadcastCard { broadcast } }
                }
            }
        }
    }
}

/// 목록의 카드 하나: 썸네일 자리와 표시(녹화·LIVE·종료), 제목, 시각.
/// 시각은 녹화는 등록 시각, 라이브는 시작 시각이다.
#[component]
fn BroadcastCard(broadcast: Broadcast) -> Element {
    let mark = broadcast.mark();
    rsx! {
        article { class: "card",
            Link { class: "card-link", to: Route::BroadcastDetail { segment: broadcast.id.to_string() },
                // 썸네일은 만들지 않으므로 회색 빈 칸을 두고, 왼쪽 위에 표시를 붙인다.
                div { class: "thumbnail",
                    span { class: "badge {mark.class()}", "{mark.label()}" }
                }
                h2 { class: "card-title", title: "{broadcast.title}", "{broadcast.title}" }
                p { class: "card-time", "{format_kst(&broadcast.shown_time())}" }
            }
        }
    }
}

/// 상세 화면: 영상 재생기와 제목, 등록 시각.
#[component]
fn BroadcastDetail(segment: String) -> Element {
    let broadcast = use_server_future(use_reactive!(|segment| async move {
        match number::parse_number(&segment) {
            Some(id) => get_broadcast(id).await,
            // 번호가 아닌 주소는 DB를 보지 않는다.
            None => Ok(None),
        }
    }))?;

    let view = match &*broadcast.read() {
        // 라이브 방송은 시청 화면 또는 종료 안내. 번호가 바뀌면 새로 그린다.
        Some(Ok(Some(broadcast))) if broadcast.kind == BroadcastKind::Live => rsx! {
            LiveDetail { key: "{broadcast.id}", broadcast: broadcast.clone() }
        },
        Some(Ok(Some(broadcast))) => rsx! {
            article { class: "detail",
                // 원본 MP4를 Range 요청으로 받으므로 끝까지 내려받지 않고도 건너뛸 수 있다.
                // playsinline: iOS Safari에서 전체 화면으로 강제로 바뀌지 않고 화면 안에서 재생한다.
                video {
                    class: "player",
                    src: "/api/broadcasts/{broadcast.id}/video",
                    controls: true,
                    playsinline: true,
                    preload: "metadata",
                }
                h1 { class: "detail-title", "{broadcast.title}" }
                p { class: "card-time", "{format_kst(&broadcast.created_at)}" }
                Link { class: "back-link", to: Route::BroadcastList {}, "← 목록으로" }
            }
        },
        Some(Ok(None)) => rsx! { NotFound {} },
        Some(Err(e)) => rsx! {
            p { class: "error", role: "alert", "{error_message(e)}" }
        },
        None => rsx! {
            p { "불러오는 중…" }
        },
    };
    view
}

/// 정해지지 않은 주소.
#[component]
fn PageNotFound(rest: Vec<String>) -> Element {
    let _ = rest;
    rsx! { NotFound {} }
}

/// "방송을 찾을 수 없습니다" 안내. 서버에서 그릴 때는 응답 상태를 404로 바꾼다.
#[component]
fn NotFound() -> Element {
    #[cfg(feature = "server")]
    dioxus::fullstack::FullstackContext::commit_http_status(
        dioxus::fullstack::http::StatusCode::NOT_FOUND,
        None,
    );

    rsx! {
        section { class: "empty",
            p { "방송을 찾을 수 없습니다" }
            Link { class: "button", to: Route::BroadcastList {}, "목록으로" }
        }
    }
}

/// 녹화 올리기 화면: 제목과 MP4 파일을 받아 등록 API로 보낸다.
#[component]
fn NewBroadcast() -> Element {
    let mut pending = use_signal(|| false);
    let mut progress = use_signal(|| 0u8);
    let mut error = use_signal(|| None::<String>);
    let navigator = use_navigator();

    let submit = move |event: FormEvent| {
        event.prevent_default();
        if pending() {
            return;
        }
        pending.set(true);
        progress.set(0);
        error.set(None);
        spawn(async move {
            match upload(move |percent| progress.set(percent)).await {
                Ok(id) => {
                    navigator.push(Route::BroadcastDetail {
                        segment: id.to_string(),
                    });
                }
                Err(message) => error.set(Some(message)),
            }
            pending.set(false);
        });
    };

    rsx! {
        h1 { "녹화 올리기" }
        form { id: "register-form", class: "register-form", onsubmit: submit,
            label {
                span { "제목" }
                input { name: "title", r#type: "text", autocomplete: "off", disabled: pending() }
            }
            label {
                span { "영상 파일 (MP4)" }
                input { name: "video", r#type: "file", accept: "video/mp4,.mp4", disabled: pending() }
            }
            button { class: "button", r#type: "submit", disabled: pending(),
                if pending() { "등록하는 중…" } else { "등록" }
            }
            if pending() {
                div { class: "upload-progress",
                    progress { max: 100, value: progress() }
                    span { role: "status", "{progress()}%" }
                }
            }
            if let Some(message) = error() {
                p { class: "error", role: "alert", "{message}" }
            }
        }
    }
}

/// 등록 화면의 입력을 multipart로 등록 API에 보낸다. 성공하면 새 방송 번호를 돌려준다.
///
/// 업로드는 브라우저의 XMLHttpRequest가 보낸다. 큰 파일을 WASM 메모리로 옮기지 않기 위해서다.
/// 보내는 동안 `on_progress`로 진행률(0~100)을 알린다.
async fn upload(mut on_progress: impl FnMut(u8)) -> Result<i64, String> {
    let mut script = document::eval(UPLOAD_SCRIPT);
    loop {
        let message: UploadMessage = script.recv().await.map_err(|_| {
            "등록 화면에 문제가 생겼어요. 새로고침한 뒤 다시 시도해 주세요.".to_string()
        })?;
        match message {
            UploadMessage::Progress { percent } => on_progress(percent.min(100)),
            UploadMessage::Done { status, body } => return upload_result(status, &body),
        }
    }
}

/// 업로드 스크립트가 Rust 쪽으로 보내는 소식.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum UploadMessage {
    /// 지금까지 보낸 비율(%).
    Progress { percent: u8 },
    /// 업로드가 끝났다. `status`가 0이면 서버에 닿지 못했다.
    Done { status: u16, body: String },
}

/// 등록 API 응답을 방송 번호나 사용자에게 보여 줄 문구로 바꾼다.
fn upload_result(status: u16, body: &str) -> Result<i64, String> {
    let json: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    if status == 201 {
        if let Some(id) = json["id"].as_i64() {
            return Ok(id);
        }
    }
    if status == 0 {
        return Err("서버에 연결하지 못했어요. 잠시 뒤 다시 시도해 주세요.".into());
    }
    Err(json["message"]
        .as_str()
        .unwrap_or("등록하지 못했어요. 잠시 뒤 다시 시도해 주세요.")
        .to_string())
}

/// 등록 화면의 입력을 읽어 XMLHttpRequest로 보내고, 결과를 `dioxus.send`로 알린다.
const UPLOAD_SCRIPT: &str = r#"
const form = document.getElementById("register-form");
const data = new FormData();
data.append("title", form.querySelector("[name=title]").value);
const file = form.querySelector("[name=video]").files[0];
if (file) data.append("video", file, file.name);

const xhr = new XMLHttpRequest();
xhr.open("POST", "/api/broadcasts");
xhr.upload.onprogress = (event) => {
  if (event.lengthComputable) {
    dioxus.send({ kind: "progress", percent: Math.floor((event.loaded / event.total) * 100) });
  }
};
xhr.onload = () => dioxus.send({ kind: "done", status: xhr.status, body: xhr.responseText });
xhr.onerror = () => dioxus.send({ kind: "done", status: 0, body: "" });
xhr.send(data);
"#;

/// 시각을 한국 시간(UTC+9) `YYYY-MM-DD HH:MM`으로 쓴다.
fn format_kst(time: &DateTime<Utc>) -> String {
    let kst = FixedOffset::east_opt(9 * 3600).expect("UTC+9는 올바른 시간대다");
    time.with_timezone(&kst)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 등록_시각은_한국_시간으로_쓴다() {
        let time = DateTime::parse_from_rfc3339("2026-11-12T15:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(format_kst(&time), "2026-11-13 00:30");
    }
}

/// 서버 함수 에러를 사용자에게 보여 줄 문구로 바꾼다.
fn error_message(error: &ServerFnError) -> String {
    match error {
        ServerFnError::ServerError { message, .. } => message.clone(),
        _ => "서버에 연결하지 못했어요. 잠시 뒤 다시 시도해 주세요.".into(),
    }
}
