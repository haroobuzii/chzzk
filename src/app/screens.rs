//! 앱 화면: 방송 목록과 방송 상세. 목록 칸·카드·방송 정보와 라이브 시청(`viewer::LivePlayer`)은 웹 화면의 조각을 그대로 쓴다.
//!
//! 앱에는 로그인·댓글·후원·등록·송출이 없다. 방송 상세의 "웹에서 열기"가 기본 브라우저에서 같은 방송의 웹 페이지를 연다(ADR 0009).

use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use dioxus::prelude::*;

use super::deploy_server::{ConnectionError, DeployServer};
use crate::broadcast_view::{use_live_status, BroadcastInfo, BroadcastSections, LiveEndedNotice};
use crate::display::Mark;
use crate::refresh::{sleep_ms, REFRESH_MS};
use crate::viewer::LivePlayer;
use crate::{Broadcast, BroadcastKind, MAIN_CSS};

/// 빌드할 때 따로 정하지 않으면 앱이 부르는 배포 서버.
const DEFAULT_SERVER_URL: &str = "https://chzzk.haroobuzii.com";

/// 앱이 부를 서버 주소. 빌드할 때 `CHZZK_APP_SERVER_URL`로 바꿀 수 있다(개발 땐 내 컴퓨터의 서버).
fn server_url() -> &'static str {
    match option_env!("CHZZK_APP_SERVER_URL") {
        Some(url) if !url.is_empty() => url,
        _ => DEFAULT_SERVER_URL,
    }
}

/// 앱 창을 띄운다.
pub fn launch() {
    let window = WindowBuilder::new()
        .with_title("녹화방송")
        .with_inner_size(LogicalSize::new(1100.0, 760.0));
    dioxus::LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(window))
        .launch(App);
}

// 웹 화면의 `Route`와 같은 이유로 매크로가 만든 코드의 "unreachable call" 경고를 이 안에서만 끈다.
#[allow(unreachable_code)]
mod route {
    use super::*;

    #[derive(Routable, Clone, PartialEq)]
    pub enum AppRoute {
        #[layout(AppShell)]
        #[route("/")]
        AppBroadcastList {},
        #[route("/broadcasts/:id")]
        AppBroadcastDetail { id: i64 },
    }
}
use route::AppRoute;

#[component]
fn App() -> Element {
    use_context_provider(|| DeployServer::new(server_url()));
    rsx! {
        document::Title { "녹화방송" }
        document::Link { rel: "stylesheet", href: MAIN_CSS }
        Router::<AppRoute> {}
    }
}

/// 모든 앱 화면에 공통인 틀: 머리글과 본문, 보기 전용 안내.
#[component]
fn AppShell() -> Element {
    rsx! {
        header { class: "site-header",
            Link { class: "brand", to: AppRoute::AppBroadcastList {}, "녹화방송" }
        }
        main {
            Outlet::<AppRoute> {}
            p { class: "hint app-note",
                "이 앱은 방송을 보기만 해요. 로그인·댓글·후원은 방송 화면의 \"웹에서 열기\"로 웹에서 할 수 있어요."
            }
        }
    }
}

/// 첫 화면: "지금 라이브" 칸과 전체 방송 목록. 5초마다 다시 불러온다.
#[component]
fn AppBroadcastList() -> Element {
    let server = use_context::<DeployServer>();
    let mut shown = use_signal(|| None::<Result<Vec<Broadcast>, ConnectionError>>);
    let mut polling = use_future(move || {
        let server = server.clone();
        async move {
            loop {
                let result = server.broadcasts().await;
                // 목록을 한 번 보여 준 뒤에는 다시 불러오기에 실패해도 지금 목록을 그대로 두고 다음에 다시 묻는다.
                let keep_current = result.is_err() && matches!(*shown.peek(), Some(Ok(_)));
                if !keep_current && shown.peek().as_ref() != Some(&result) {
                    shown.set(Some(result));
                }
                sleep_ms(REFRESH_MS).await;
            }
        }
    });

    let retry = move |_| {
        shown.set(None);
        polling.restart();
    };
    match shown() {
        None => rsx! {
            p { "불러오는 중…" }
        },
        Some(Err(error)) => rsx! {
            ConnectionFailed { error, on_retry: retry }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            section { class: "empty",
                p { "아직 방송이 없습니다" }
            }
        },
        Some(Ok(list)) => rsx! {
            BroadcastSections {
                list,
                detail_of: |id: i64| -> NavigationTarget { AppRoute::AppBroadcastDetail { id }.into() },
            }
        },
    }
}

/// 방송 상세: 녹화 방송도 라이브 방송도 앱 안에서 본다. 웹뷰에 WebRTC가 없으면 라이브는 웹에서 시청하도록 안내한다.
#[component]
fn AppBroadcastDetail(id: i64) -> Element {
    let server = use_context::<DeployServer>();
    let mut loaded = use_resource(use_reactive!(|id| {
        let server = server.clone();
        async move { server.broadcast(id).await }
    }));

    let view = match loaded() {
        None => rsx! {
            p { "불러오는 중…" }
        },
        Some(Err(error)) => rsx! {
            ConnectionFailed { error, on_retry: move |_| loaded.restart() }
        },
        Some(Ok(None)) => rsx! {
            section { class: "empty",
                p { "방송을 찾을 수 없습니다" }
                Link { class: "button", to: AppRoute::AppBroadcastList {}, "목록으로" }
            }
        },
        Some(Ok(Some(broadcast))) => rsx! {
            BroadcastView { key: "{broadcast.id}", broadcast }
        },
    };
    view
}

/// 불러온 방송 하나의 상세. 방송 중인 라이브는 5초마다 상태를 확인해 종료·제목 변경·삭제를 반영한다.
#[component]
fn BroadcastView(broadcast: Broadcast) -> Element {
    let server = use_context::<DeployServer>();
    let fetch = {
        let server = server.clone();
        move |id: i64| {
            let server = server.clone();
            async move { server.broadcast(id).await }
        }
    };
    let (current, deleted) = use_live_status(broadcast, fetch);
    if deleted() {
        return rsx! {
            section { class: "empty",
                p { "방송을 찾을 수 없습니다" }
                Link { class: "button", to: AppRoute::AppBroadcastList {}, "목록으로" }
            }
        };
    }
    let broadcast = current();
    let web_page = server.web_page_url(broadcast.id);
    rsx! {
        article { class: "detail",
            if broadcast.kind == BroadcastKind::Recorded {
                // 원본 MP4를 Range 요청으로 받으므로 끝까지 내려받지 않고도 건너뛸 수 있다.
                video {
                    class: "player",
                    src: server.video_url(broadcast.id),
                    controls: true,
                    playsinline: true,
                    preload: "metadata",
                }
            } else if broadcast.mark() == Mark::Live {
                // 웹과 같은 시청 스크립트로 배포 서버의 WHEP에 바로 연결한다.
                // 방송이 끝나 이 부분이 사라지면 LivePlayer가 연결을 닫는다.
                LivePlayer { whep_url: server.whep_url(broadcast.id), web_page: web_page.clone() }
            } else {
                LiveEndedNotice {}
            }
            BroadcastInfo { broadcast: broadcast.clone() }
            div { class: "owner-actions",
                // 앱 웹뷰는 바깥 주소로 가지 않고, 기본 브라우저에서 연다.
                a { class: "button", href: "{web_page}", target: "_blank", "웹에서 열기" }
            }
            Link { class: "back-link", to: AppRoute::AppBroadcastList {}, "← 목록으로" }
        }
    }
}

/// 서버에 연결할 수 없을 때의 안내와 "다시 시도".
#[component]
fn ConnectionFailed(error: ConnectionError, on_retry: EventHandler<MouseEvent>) -> Element {
    rsx! {
        section { class: "empty",
            p { class: "error", role: "alert", "{error}" }
            button { class: "button", r#type: "button", onclick: on_retry, "다시 시도" }
        }
    }
}
