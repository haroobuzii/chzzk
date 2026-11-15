//! 송출 화면 `/live/new`: 제목을 넣고 카메라 미리보기를 본 뒤 "방송 시작"으로 송출한다.
//!
//! 카메라와 WebRTC는 브라우저 JavaScript(`PUBLISHER_JS`)가 다룬다. Rust 쪽은 `document::eval`로
//! 명령을 하나씩 보내고, 결과는 `dioxus.send`로 받는다.
//! 송출 키는 JavaScript 쪽 메모리(`window.chzzkPublisher`)에만 두고 브라우저 저장소에 두지 않는다.

use dioxus::prelude::*;
use serde::Deserialize;

use crate::Route;

/// 송출 화면의 단계.
#[derive(Clone, Debug, PartialEq)]
enum Phase {
    /// 카메라를 켜는 중이거나 미리보기를 보는 중. 아직 방송을 시작하지 않았다.
    Preview,
    /// 시작 API를 부르고 WebRTC로 연결하는 중.
    Connecting,
    /// 송출 중. 방송 번호를 안다.
    Live { id: i64 },
    /// "방송 종료"를 눌러 끝내는 중.
    Ending { id: i64 },
    /// 방송을 끝냈다. 연결과 카메라는 꺼졌다.
    Ended { id: i64 },
}

/// JavaScript 명령의 결과.
#[derive(Deserialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PublisherMessage {
    /// 명령이 끝났다. 시작 명령이면 방송 번호가 담긴다.
    Ok {
        #[serde(default)]
        id: Option<i64>,
    },
    /// 실패했다. 사용자에게 보여 줄 한국어 문구가 담긴다.
    Error { message: String },
}

#[component]
pub fn NewLive() -> Element {
    let mut phase = use_signal(|| Phase::Preview);
    let mut error = use_signal(|| None::<String>);
    let mut title = use_signal(String::new);

    // 화면에 들어오면 카메라 미리보기를 켠다.
    use_effect(move || {
        spawn(async move {
            if let Err(message) = run("await chzzkPublisher.preview();").await {
                error.set(Some(message));
            }
        });
    });
    // 화면을 떠나면 연결을 닫고 카메라를 끈다.
    use_drop(|| {
        document::eval(&format!("{PUBLISHER_JS}\nchzzkPublisher.stop();"));
    });

    let start = move |event: FormEvent| {
        event.prevent_default();
        if phase() != Phase::Preview {
            return;
        }
        phase.set(Phase::Connecting);
        error.set(None);
        spawn(async move {
            let title_json = serde_json::to_string(&title()).unwrap_or_default();
            match run_for_id(&format!("return await chzzkPublisher.start({title_json});")).await {
                Ok(id) => phase.set(Phase::Live { id }),
                Err(message) => {
                    error.set(Some(message));
                    phase.set(Phase::Preview);
                }
            }
        });
    };

    let end = move |_| {
        let Phase::Live { id } = phase() else {
            return;
        };
        phase.set(Phase::Ending { id });
        spawn(async move {
            // 종료 API가 실패해도 연결은 닫히므로 상태 감시기가 끝낸다. 화면은 끝난 것으로 보여 준다.
            let _ = run("await chzzkPublisher.end();").await;
            phase.set(Phase::Ended { id });
        });
    };

    let connecting = phase() == Phase::Connecting;
    rsx! {
        h1 { "라이브 시작" }
        div { class: "publisher",
            div { class: "publisher-preview",
                // 미리보기는 음소거한다. 내 목소리가 스피커로 다시 나와 울리지 않게 하기 위해서다.
                video {
                    id: "publisher-video",
                    class: "player",
                    autoplay: true,
                    muted: true,
                    playsinline: true,
                }
                if let Phase::Live { .. } = phase() {
                    span { class: "badge badge-live", "LIVE" }
                }
            }
            match phase() {
                Phase::Live { id } => rsx! {
                    p { class: "publisher-status", role: "status", "방송 중입니다" }
                    Link { class: "back-link", to: Route::BroadcastDetail { segment: id.to_string() }, "내 방송 보기 →" }
                    button { class: "button end-button", r#type: "button", onclick: end, "방송 종료" }
                },
                Phase::Ending { .. } => rsx! {
                    button { class: "button end-button", r#type: "button", disabled: true, "끝내는 중…" }
                },
                Phase::Ended { id } => rsx! {
                    p { class: "publisher-status", role: "status", "방송을 종료했습니다" }
                    Link { class: "back-link", to: Route::BroadcastDetail { segment: id.to_string() }, "내 방송 보기 →" }
                },
                _ => rsx! {
                    form { class: "register-form", onsubmit: start,
                        label {
                            span { "제목" }
                            input {
                                name: "title",
                                r#type: "text",
                                autocomplete: "off",
                                disabled: connecting,
                                value: "{title}",
                                oninput: move |event| title.set(event.value()),
                            }
                        }
                        button { class: "button live-button", r#type: "submit", disabled: connecting,
                            if connecting { "연결하는 중…" } else { "방송 시작" }
                        }
                    }
                },
            }
            if let Some(message) = error() {
                p { class: "error", role: "alert", "{message}" }
            }
        }
    }
}

/// `body`를 송출 스크립트와 함께 실행하고 결과를 기다린다.
async fn run(body: &str) -> Result<Option<i64>, String> {
    let script = format!(
        r#"{PUBLISHER_JS}
try {{
  const result = await (async () => {{ {body} }})();
  dioxus.send({{ kind: "ok", id: typeof result === "number" ? result : null }});
}} catch (e) {{
  dioxus.send({{ kind: "error", message: chzzkPublisher.describe(e) }});
}}"#
    );
    let mut eval = document::eval(&script);
    let message: PublisherMessage = eval.recv().await.map_err(|_| {
        "송출 화면에 문제가 생겼어요. 새로고침한 뒤 다시 시도해 주세요.".to_string()
    })?;
    match message {
        PublisherMessage::Ok { id } => Ok(id),
        PublisherMessage::Error { message } => Err(message),
    }
}

async fn run_for_id(body: &str) -> Result<i64, String> {
    run(body)
        .await?
        .ok_or_else(|| "방송 번호를 받지 못했어요. 다시 시도해 주세요.".to_string())
}

/// 송출 스크립트. `window.chzzkPublisher`를 한 번만 만들고, 이후 명령은 같은 객체를 쓴다.
///
/// - `preview()`: 후면 카메라 1280×720과 마이크를 켜고 미리보기에 띄운다.
/// - `start(title)`: 시작 API → RTCPeerConnection(sendonly) → ICE 수집 완료 → WHIP 송출 → 연결 확인. 방송 번호를 돌려준다.
/// - `stop()`: 연결을 닫고 카메라·마이크를 끈다.
/// - `end()`: 종료 API를 부른 뒤 `stop()`한다. 화면을 떠날 때(pagehide)는 keepalive 요청으로 종료를 시도한다.
/// - `describe(error)`: 에러를 사용자에게 보여 줄 한국어 문구로 바꾼다.
///
/// 상태(`stream`, `pc`, `video`, `id`, `key`, `session`)는 객체 안에 둔다. 송출 키는 여기에만 있다.
const PUBLISHER_JS: &str = r#"
window.chzzkPublisher ??= (() => {
  const state = { stream: null, pc: null, video: null, id: null, key: null, session: null, facing: "environment" };

  class PublisherError extends Error {}
  const fail = (message) => { throw new PublisherError(message); };

  function videoElement() {
    return document.getElementById("publisher-video");
  }

  async function openCamera() {
    if (!window.isSecureContext || !navigator.mediaDevices?.getUserMedia) {
      fail("카메라를 쓰려면 HTTPS 주소로 접속해야 해요. 터널의 https:// 주소로 다시 들어와 주세요.");
    }
    return navigator.mediaDevices.getUserMedia({
      video: { facingMode: { ideal: state.facing }, width: { ideal: 1280 }, height: { ideal: 720 } },
      audio: true,
    });
  }

  async function preview() {
    if (!state.stream) state.stream = await openCamera();
    const video = videoElement();
    if (video) {
      video.muted = true;
      video.srcObject = state.stream;
      await video.play().catch(() => {});
    }
  }

  async function createLive(title) {
    const res = await fetch("/api/live", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title }),
    }).catch(() => fail("서버에 연결하지 못했어요. 잠시 뒤 다시 시도해 주세요."));
    const body = await res.json().catch(() => ({}));
    if (res.status !== 201) fail(body.message || "방송을 시작하지 못했어요. 다시 시도해 주세요.");
    state.id = body.id;
    state.key = body.key;
  }

  function waitIceGathering(pc) {
    return new Promise((resolve) => {
      if (pc.iceGatheringState === "complete") return resolve();
      const done = () => pc.iceGatheringState === "complete" && resolve();
      pc.addEventListener("icegatheringstatechange", done);
      // 후보 수집이 오래 걸려도 모은 만큼으로 보낸다.
      setTimeout(resolve, 3000);
    });
  }

  function waitConnected(pc) {
    return new Promise((resolve, reject) => {
      const check = () => {
        if (pc.connectionState === "connected") resolve();
        if (pc.connectionState === "failed" || pc.connectionState === "closed") reject(new PublisherError(CONNECT_FAILED));
      };
      pc.addEventListener("connectionstatechange", check);
      check();
      setTimeout(() => reject(new PublisherError(CONNECT_FAILED)), 15000);
    });
  }

  const CONNECT_FAILED = "송출 서버와 연결하지 못했어요. 휴대폰과 컴퓨터가 같은 와이파이에 있는지 확인하고 다시 시도해 주세요.";

  async function publish() {
    const pc = new RTCPeerConnection();
    state.pc = pc;
    const [videoTrack] = state.stream.getVideoTracks();
    const [audioTrack] = state.stream.getAudioTracks();
    const videoTransceiver = pc.addTransceiver(videoTrack, { direction: "sendonly", streams: [state.stream] });
    if (audioTrack) pc.addTransceiver(audioTrack, { direction: "sendonly", streams: [state.stream] });
    // MediaMTX는 다시 인코딩하지 않으므로, 어느 브라우저든 볼 수 있는 H264를 먼저 고른다.
    const codecs = RTCRtpSender.getCapabilities?.("video")?.codecs ?? [];
    if (codecs.length && videoTransceiver.setCodecPreferences) {
      videoTransceiver.setCodecPreferences([
        ...codecs.filter((c) => c.mimeType === "video/H264"),
        ...codecs.filter((c) => c.mimeType !== "video/H264"),
      ]);
    }
    await pc.setLocalDescription(await pc.createOffer());
    await waitIceGathering(pc);

    const res = await fetch(`/api/live/${state.id}/whip`, {
      method: "POST",
      headers: { "Content-Type": "application/sdp", Authorization: `Bearer ${state.key}` },
      body: pc.localDescription.sdp,
    }).catch(() => fail(CONNECT_FAILED));
    if (res.status !== 201) {
      const body = await res.json().catch(() => ({}));
      fail(body.message || CONNECT_FAILED);
    }
    state.session = res.headers.get("Location");
    await pc.setRemoteDescription({ type: "answer", sdp: await res.text() });
    await waitConnected(pc);
  }

  async function start(title) {
    await preview();
    await createLive(title);
    try {
      await publish();
    } catch (e) {
      closeConnection();
      throw e;
    }
    return state.id;
  }

  function closeConnection() {
    state.pc?.close();
    state.pc = null;
    state.session = null;
  }

  function stop() {
    closeConnection();
    state.stream?.getTracks().forEach((t) => t.stop());
    state.stream = null;
    state.id = null;
    state.key = null;
  }

  // 종료 API를 부른다. 실패해도 상태 감시기가 경로가 사라진 것을 보고 끝내므로 결과는 보지 않는다.
  function requestEnd(keepalive) {
    if (state.id == null || !state.key) return Promise.resolve();
    return fetch(`/api/live/${state.id}/end`, {
      method: "POST",
      headers: { Authorization: `Bearer ${state.key}` },
      keepalive,
    }).catch(() => {});
  }

  // "방송 종료": 종료 API를 부른 뒤 연결을 닫고 카메라·마이크를 끈다.
  async function end() {
    await requestEnd(false);
    stop();
  }

  // 송출 화면을 떠날 때(탭 닫기, 다른 사이트로 이동)도 종료를 시도한다. 페이지가 사라져도 요청이 가도록 keepalive를 쓴다.
  window.addEventListener("pagehide", () => {
    requestEnd(true);
  });

  function describe(e) {
    if (e instanceof PublisherError) return e.message;
    if (e?.name === "NotAllowedError" || e?.name === "SecurityError") {
      return "카메라·마이크 권한이 없어 방송을 시작할 수 없어요. 브라우저 주소창의 사이트 설정에서 카메라와 마이크를 허용한 뒤 새로고침해 주세요.";
    }
    if (e?.name === "NotFoundError" || e?.name === "OverconstrainedError") {
      return "쓸 수 있는 카메라나 마이크를 찾지 못했어요.";
    }
    if (e?.name === "NotReadableError") {
      return "카메라를 다른 앱이 쓰고 있어요. 다른 앱을 닫고 다시 시도해 주세요.";
    }
    return "송출을 준비하지 못했어요. 새로고침한 뒤 다시 시도해 주세요.";
  }

  return { state, preview, start, stop, end, describe };
})();
"#;
