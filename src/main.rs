use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(feature = "server")]
mod config;
#[cfg(feature = "server")]
mod db;
#[cfg(feature = "server")]
mod server;

const MAIN_CSS: Asset = asset!("/assets/main.css");

fn main() {
    // 서버로 빌드할 때: 설정을 읽고 DB에 접속한 뒤 화면과 API를 함께 제공한다.
    #[cfg(feature = "server")]
    dioxus::serve(|| async move { server::router().await });

    // 브라우저(WASM)로 빌드할 때: 화면만 띄운다.
    #[cfg(not(feature = "server"))]
    dioxus::launch(App);
}

/// 방송 하나. 제목 하나와 영상 하나를 가진다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
pub struct Broadcast {
    /// 방송 번호. DB가 1부터 매긴다.
    pub id: i64,
    pub title: String,
    /// 등록 시각.
    pub created_at: DateTime<Utc>,
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

#[derive(Routable, Clone, PartialEq)]
enum Route {
    #[layout(Shell)]
    #[route("/")]
    BroadcastList {},
    #[route("/new")]
    NewBroadcast {},
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
            Link { class: "button", to: Route::NewBroadcast {}, "방송 등록" }
        }
        main { Outlet::<Route> {} }
    }
}

/// 첫 화면: 방송 목록.
#[component]
fn BroadcastList() -> Element {
    let broadcasts = use_server_future(get_broadcasts)?;

    let view = match &*broadcasts.read() {
        Some(Ok(list)) if list.is_empty() => rsx! {
            section { class: "empty",
                p { "아직 방송이 없습니다" }
                Link { class: "button", to: Route::NewBroadcast {}, "방송 등록" }
            }
        },
        Some(Ok(list)) => rsx! {
            ul { class: "broadcast-list",
                for broadcast in list {
                    li { key: "{broadcast.id}", "{broadcast.title}" }
                }
            }
        },
        Some(Err(e)) => rsx! {
            p { class: "error", role: "alert", "{error_message(e)}" }
        },
        None => rsx! {
            p { "불러오는 중…" }
        },
    };
    view
}

/// 방송 등록 화면. 등록 기능은 다음 단계에서 붙인다.
#[component]
fn NewBroadcast() -> Element {
    rsx! {
        h1 { "방송 등록" }
        p { "등록 화면은 준비 중입니다." }
    }
}

/// 서버 함수 에러를 사용자에게 보여 줄 문구로 바꾼다.
fn error_message(error: &ServerFnError) -> String {
    match error {
        ServerFnError::ServerError { message, .. } => message.clone(),
        _ => "서버에 연결하지 못했어요. 잠시 뒤 다시 시도해 주세요.".into(),
    }
}
