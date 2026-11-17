//! 라이브 방송 시청 화면: WHEP으로 받은 영상을 띄우고, 5초마다 상태를 확인해 제목 변경과 종료, 삭제를 반영한다.
//!
//! WebRTC는 브라우저 JavaScript(`VIEWER_JS`)가 다룬다. Rust 쪽은 `document::eval`로 명령을 보내고,
//! 결과는 `dioxus.send`로 받는다.

use dioxus::prelude::*;
use serde::Deserialize;

use crate::comments::CommentSection;
use crate::display::{broadcast_duration, Mark};
use crate::login::call_api;
use crate::manage::OwnerActions;
use crate::refresh::{sleep_ms, REFRESH_MS};
use crate::{format_kst, get_broadcast, Broadcast, NotFound, Route};

/// 라이브 방송 상세: 방송 중이면 시청 화면, 종료면 종료 안내.
/// 소유자에게는 방송 중일 때 "방송 종료" 버튼이 보인다.
#[component]
pub fn LiveDetail(broadcast: Broadcast) -> Element {
    let mut current = use_signal(|| broadcast.clone());
    // 보고 있던 방송이 지워졌는지.
    let mut deleted = use_signal(|| false);

    // 5초마다 상태(제목 포함)를 확인한다. 종료되거나 지워지면 더 묻지 않는다.
    use_effect(move || {
        let id = current.peek().id;
        spawn(async move {
            while current.peek().mark() == Mark::Live {
                sleep_ms(REFRESH_MS).await;
                // 확인에 실패하면 지금 화면을 그대로 두고 다음에 다시 묻는다.
                match get_broadcast(id).await {
                    Ok(Some(latest)) => {
                        if latest != *current.peek() {
                            current.set(latest);
                        }
                    }
                    Ok(None) => {
                        deleted.set(true);
                        break;
                    }
                    Err(_) => {}
                }
            }
        });
    });

    // 지워지면 LivePlayer가 사라지며 연결을 닫는다.
    if deleted() {
        return rsx! { NotFound {} };
    }
    let broadcast = current();
    let shown_time = broadcast.shown_time();
    // 종료가 끝나면 바로 다시 불러와 종료 안내로 바꾼다(5초 확인을 기다리지 않는다).
    let refresh = move |_| {
        let id = current.peek().id;
        spawn(async move {
            if let Ok(Some(latest)) = get_broadcast(id).await {
                current.set(latest);
            }
        });
    };
    rsx! {
        article { class: "detail",
            if broadcast.mark() == Mark::Live {
                // 방송이 끝나 이 부분이 사라지면 LivePlayer가 연결을 닫는다.
                LivePlayer { id: broadcast.id }
            } else {
                div { class: "live-ended", role: "status",
                    p { class: "live-ended-title", "방송이 종료되었습니다" }
                    p { "종료된 라이브 방송은 다시 볼 수 없습니다." }
                }
            }
            h1 { class: "detail-title",
                if broadcast.mark() == Mark::Live {
                    span { class: "inline-badge {Mark::Live.class()}", "{Mark::Live.label()}" }
                }
                "{broadcast.title}"
            }
            p { class: "detail-owner", "{broadcast.owner_label()}" }
            p { class: "card-time", "시작 {format_kst(&shown_time)}" }
            if let Some(ended_at) = broadcast.ended_at {
                p { class: "card-time", "방송한 시간 {broadcast_duration(shown_time, ended_at)}" }
            }
            OwnerActions {
                broadcast: broadcast.clone(),
                on_renamed: move |title| current.write().title = title,
                // 소유자에게만 보이는 "방송 종료". 송출 화면을 닫았어도 여기서 끌 수 있다.
                if broadcast.mark() == Mark::Live {
                    EndLiveButton { id: broadcast.id, on_ended: refresh }
                }
            }
            Link { class: "back-link", to: Route::BroadcastList {}, "← 목록으로" }
        }
        // 방송 중이면 댓글을 3초마다 다시 불러온다. 종료된 방송에도 댓글을 쓸 수 있다.
        CommentSection { broadcast_id: broadcast.id, live: broadcast.mark() == Mark::Live }
    }
}

/// 소유자의 "방송 종료" 버튼. 송출 화면을 닫았어도 여기서 끌 수 있다.
#[component]
fn EndLiveButton(id: i64, on_ended: EventHandler<()>) -> Element {
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let end = move |_| async move {
        pending.set(true);
        error.set(None);
        match call_api("POST", &format!("/api/live/{id}/end"), None).await {
            Ok((204, _)) => on_ended.call(()),
            Ok((_, body)) => error.set(Some(
                body["message"]
                    .as_str()
                    .unwrap_or("방송을 끝내지 못했어요. 다시 시도해 주세요.")
                    .to_string(),
            )),
            Err(message) => error.set(Some(message)),
        }
        pending.set(false);
    };

    rsx! {
        if pending() {
            button { class: "button end-button", r#type: "button", disabled: true, "끝내는 중…" }
        } else {
            button { class: "button end-button", r#type: "button", onclick: end, "방송 종료" }
        }
        if let Some(message) = error() {
            p { class: "error", role: "alert", "{message}" }
        }
    }
}

/// 시청 스크립트가 Rust 쪽으로 보내는 소식.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ViewerMessage {
    /// 연결했다(또는 소리를 켰다).
    Ok,
    /// 실패했다. 사용자에게 보여 줄 한국어 문구가 담긴다.
    Error { message: String },
}

/// 실시간 영상. 화면에 들어오면 WHEP으로 연결하고, 화면에서 사라지면 연결을 닫는다.
#[component]
fn LivePlayer(id: i64) -> Element {
    let mut error = use_signal(|| None::<String>);
    let mut muted = use_signal(|| true);

    use_effect(move || {
        spawn(async move {
            if let Err(message) = run(&format!("await chzzkViewer.start({id});")).await {
                error.set(Some(message));
            }
        });
    });
    use_drop(|| {
        document::eval(&format!("{VIEWER_JS}\nchzzkViewer.stop();"));
    });

    let unmute = move |_| {
        spawn(async move {
            match run("await chzzkViewer.unmute();").await {
                Ok(()) => muted.set(false),
                Err(message) => error.set(Some(message)),
            }
        });
    };

    rsx! {
        div { class: "live-player-wrap",
            // 소리 있는 자동 재생은 브라우저가 막으므로 음소거로 시작하고 "소리 켜기"로 켠다.
            video {
                id: "live-player",
                class: "player",
                autoplay: true,
                muted: true,
                playsinline: true,
            }
            if muted() {
                button { class: "button unmute-button", r#type: "button", onclick: unmute, "소리 켜기" }
            }
        }
        if let Some(message) = error() {
            p { class: "error", role: "alert", "{message}" }
        }
    }
}

/// `body`를 시청 스크립트 뒤에 붙여 실행하고 결과를 받는다.
async fn run(body: &str) -> Result<(), String> {
    let script = format!(
        "{VIEWER_JS}
try {{
  {body}
  dioxus.send({{ kind: \"ok\" }});
}} catch (e) {{
  dioxus.send({{ kind: \"error\", message: chzzkViewer.describe(e) }});
}}"
    );
    let mut eval = document::eval(&script);
    let message: ViewerMessage = eval.recv().await.map_err(|_| {
        "시청 화면에 문제가 생겼어요. 새로고침한 뒤 다시 시도해 주세요.".to_string()
    })?;
    match message {
        ViewerMessage::Ok => Ok(()),
        ViewerMessage::Error { message } => Err(message),
    }
}

/// 시청 연결을 다루는 브라우저 JavaScript. 여러 번 실행해도 한 번만 만든다.
const VIEWER_JS: &str = r#"
window.chzzkViewer ??= (() => {
  class ViewerError extends Error {}
  const CONNECT_FAILED = "영상을 불러오지 못했어요. 잠시 뒤 새로고침해 주세요.";
  let current = null;

  const video = () => document.getElementById("live-player");

  // ICE 후보 수집이 끝나기를 기다린다. 오래 걸리면 모은 후보만으로 보낸다.
  function gathered(pc) {
    if (pc.iceGatheringState === "complete") return Promise.resolve();
    return new Promise((resolve) => {
      const timer = setTimeout(resolve, 3000);
      pc.addEventListener("icegatheringstatechange", () => {
        if (pc.iceGatheringState === "complete") {
          clearTimeout(timer);
          resolve();
        }
      });
    });
  }

  // 세션을 먼저 지운 뒤 연결을 닫는다. 먼저 닫으면 MediaMTX가 세션을 지워 DELETE가 404가 된다.
  function hangUp(session) {
    if (session.location) {
      fetch(session.location, { method: "DELETE", keepalive: true }).catch(() => {});
    }
    session.pc.close();
  }

  async function start(id) {
    stop();
    if (!window.RTCPeerConnection) {
      throw new ViewerError("이 브라우저는 실시간 시청을 지원하지 않아요. 최신 Chrome이나 Safari로 열어 주세요.");
    }
    const session = { pc: new RTCPeerConnection(), location: null, closed: false };
    current = session;
    const pc = session.pc;
    pc.addTransceiver("video", { direction: "recvonly" });
    pc.addTransceiver("audio", { direction: "recvonly" });
    const stream = new MediaStream();
    pc.ontrack = (event) => {
      stream.addTrack(event.track);
      const player = video();
      if (player && player.srcObject !== stream) {
        player.muted = true;
        player.srcObject = stream;
        player.play().catch(() => {});
      }
    };

    await pc.setLocalDescription(await pc.createOffer());
    await gathered(pc);
    if (session.closed) return;
    const response = await fetch(`/api/live/${id}/whep`, {
      method: "POST",
      headers: { "Content-Type": "application/sdp" },
      body: pc.localDescription.sdp,
    });
    if (response.status !== 201) {
      let message = null;
      try { message = (await response.json()).message; } catch (_) {}
      throw new ViewerError(message || CONNECT_FAILED);
    }
    session.location = response.headers.get("Location");
    const answer = await response.text();
    // 기다리는 사이 화면을 떠났으면 바로 끊는다.
    if (session.closed) {
      hangUp(session);
      return;
    }
    await pc.setRemoteDescription({ type: "answer", sdp: answer });
  }

  function stop() {
    const session = current;
    current = null;
    if (!session) return;
    session.closed = true;
    hangUp(session);
    const player = video();
    if (player) player.srcObject = null;
  }

  async function unmute() {
    const player = video();
    if (!player) return;
    player.muted = false;
    try {
      await player.play();
    } catch (_) {
      player.muted = true;
      throw new ViewerError("소리를 켜지 못했어요. 영상을 한 번 누른 뒤 다시 시도해 주세요.");
    }
  }

  function describe(error) {
    return error instanceof ViewerError ? error.message : CONNECT_FAILED;
  }

  return { start, stop, unmute, describe };
})();
"#;
