//! 송출 화면 `/live/new`: 제목을 넣고 카메라 미리보기를 본 뒤 "방송 시작"으로 송출한다.
//!
//! 카메라와 WebRTC는 브라우저 JavaScript(`PUBLISHER_JS`)가 다룬다. Rust 쪽은 `document::eval`로
//! 명령을 하나씩 보내고, 결과는 `dioxus.send`로 받는다.
//! 송출과 종료 요청에는 따로 열쇠를 싣지 않는다. 같은 출처 요청이라 로그인 쿠키가 실리고, 서버가 소유자인지 확인한다.

use dioxus::prelude::*;
use serde::Deserialize;

use crate::{display::Mark, login::RequireLogin, Route};

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
    /// 실패했다.
    Error(Failure),
}

/// JavaScript가 알려 준 실패. 브라우저에서 생긴 실패는 `code`로, 서버가 거절한 이유는 `message`로 온다.
#[derive(Deserialize, Debug)]
struct Failure {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

impl Failure {
    /// 사용자에게 보여 줄 한국어 안내.
    fn message(self) -> String {
        if let Some(message) = self.message {
            return message;
        }
        match self.code.as_deref() {
            Some("insecure") => "카메라를 쓰려면 HTTPS 주소로 접속해야 해요. 터널의 https:// 주소로 다시 들어와 주세요.",
            Some("permission_denied") => "카메라·마이크 권한이 없어 방송을 시작할 수 없어요. 주소창 옆의 사이트 설정(자물쇠 아이콘)에서 카메라와 마이크를 허용한 뒤 새로고침해 주세요.",
            Some("no_device") => "쓸 수 있는 카메라나 마이크를 찾지 못했어요.",
            Some("device_busy") => "카메라를 다른 앱이 쓰고 있어요. 다른 앱을 닫고 다시 시도해 주세요.",
            Some("no_other_camera") => "바꿀 다른 카메라가 없어요.",
            Some("server_unreachable") => "서버에 연결하지 못했어요. 잠시 뒤 다시 시도해 주세요.",
            Some("connect_failed") => "송출 서버와 연결하지 못했어요. 휴대폰과 컴퓨터가 같은 와이파이에 있는지 확인하고 다시 시도해 주세요.",
            _ => "송출을 준비하지 못했어요. 새로고침한 뒤 다시 시도해 주세요.",
        }
        .to_string()
    }
}

/// 송출하는 동안 JavaScript가 스스로 알려 오는 일.
#[derive(Deserialize, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PublisherEvent {
    /// 송출 연결이 끊겼다. JavaScript가 이미 카메라를 끄고 종료 API를 불렀다.
    ConnectionLost,
    /// 화면 꺼짐 막기를 (다시) 걸었는지.
    WakeLock { held: bool },
}

const CONNECTION_LOST: &str = "송출 연결이 끊겨 방송이 종료되었어요. 새 방송을 시작해 주세요.";

impl PublisherEvent {
    /// 연결이 끊긴 일을 화면 단계에 반영한다. 바뀔 단계와 띄울 안내를 돌려준다.
    /// 송출 중이 아니면(이미 끝냈거나 끝내는 중이면) 아무것도 바꾸지 않는다.
    fn apply(&self, phase: &Phase) -> Option<(Phase, &'static str)> {
        match (self, phase) {
            (PublisherEvent::ConnectionLost, Phase::Live { id }) => {
                Some((Phase::Ended { id: *id }, CONNECTION_LOST))
            }
            _ => None,
        }
    }
}

/// 라이브 시작 화면. 로그인한 사용자만 쓸 수 있다.
#[component]
pub fn NewLive() -> Element {
    rsx! {
        RequireLogin { Publisher {} }
    }
}

/// 송출 화면 본문: 카메라 미리보기, 제목, 방송 시작·종료.
#[component]
fn Publisher() -> Element {
    let mut phase = use_signal(|| Phase::Preview);
    let mut error = use_signal(|| None::<String>);
    let mut title = use_signal(String::new);
    // 송출 중에 화면 꺼짐 막기를 걸고 있는지. 못 걸었거나 풀린 뒤 다시 못 걸면 "화면을 켜 두세요" 안내를 띄운다.
    let mut wake_lock = use_signal(|| true);
    // 시청자에게 알려 줄 상세 화면의 전체 주소.
    let mut detail_url = use_signal(String::new);
    let mut copied = use_signal(|| false);
    let mut switching = use_signal(|| false);

    // 화면에 들어오면 카메라 미리보기를 켠다.
    use_effect(move || {
        spawn(async move {
            if let Err(message) = run("await chzzkPublisher.preview();").await {
                error.set(Some(message));
            }
        });
    });
    // 송출하는 동안 JavaScript가 알려 오는 일(연결 끊김, 화면 꺼짐 막기)을 받는다.
    use_effect(move || {
        spawn(async move {
            let mut events = document::eval(&format!(
                "{PUBLISHER_JS}\nchzzkPublisher.listen((event) => dioxus.send(event));\nawait new Promise(() => {{}});"
            ));
            while let Ok(event) = events.recv::<PublisherEvent>().await {
                if let PublisherEvent::WakeLock { held } = event {
                    wake_lock.set(held);
                    continue;
                }
                let current = phase.peek().clone();
                if let Some((next, notice)) = event.apply(&current) {
                    phase.set(next);
                    error.set(Some(notice.to_string()));
                }
            }
        });
    });
    // 화면을 떠나면(앱 안의 다른 화면으로 가도) 방송을 끝내고, 연결을 닫고 카메라를 끈다.
    use_drop(|| {
        document::eval(&format!("{PUBLISHER_JS}\nchzzkPublisher.leave();"));
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
                Ok(id) => {
                    let origin = ask::<String>("location.origin").await.unwrap_or_default();
                    let path = Route::BroadcastDetail {
                        segment: id.to_string(),
                    };
                    detail_url.set(format!("{origin}{path}"));
                    copied.set(false);
                    phase.set(Phase::Live { id });
                }
                Err(message) => {
                    error.set(Some(message));
                    phase.set(Phase::Preview);
                }
            }
        });
    };

    let switch_camera = move |_| {
        if switching() {
            return;
        }
        switching.set(true);
        spawn(async move {
            match run("await chzzkPublisher.switchCamera();").await {
                Ok(_) => error.set(None),
                Err(message) => error.set(Some(message)),
            }
            switching.set(false);
        });
    };

    let copy = move |_| async move {
        let script = document::eval(COPY_SCRIPT);
        if script.send(detail_url()).is_ok() && script.join::<bool>().await.is_ok() {
            copied.set(true);
        } else {
            error.set(Some(
                "복사하지 못했어요. 주소를 직접 선택해서 복사해 주세요.".into(),
            ));
        }
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
                    span { class: "badge {Mark::Live.class()}", "{Mark::Live.label()}" }
                }
            }
            // 끝내는 중이거나 끝난 뒤에는 카메라가 꺼져 있으므로 전환 버튼을 감춘다.
            if matches!(phase(), Phase::Preview | Phase::Connecting | Phase::Live { .. }) {
                button {
                    class: "button secondary-button",
                    r#type: "button",
                    disabled: connecting || switching(),
                    onclick: switch_camera,
                    if switching() { "카메라 바꾸는 중…" } else { "카메라 전환" }
                }
            }
            match phase() {
                Phase::Live { .. } => rsx! {
                    p { class: "publisher-status", role: "status", "방송 중입니다" }
                    if !wake_lock() {
                        p { class: "notice", "송출 중에는 화면을 켜 두세요" }
                    }
                    // 새 탭으로 연다. 이 화면을 떠나면 송출이 멈추기 때문이다.
                    div { class: "share-link",
                        span { "시청 주소" }
                        a {
                            href: "{detail_url}",
                            target: "_blank",
                            rel: "noopener",
                            "{detail_url}"
                        }
                        button { class: "button secondary-button", r#type: "button", onclick: copy,
                            if copied() { "복사됨" } else { "복사" }
                        }
                    }
                    button { class: "button end-button", r#type: "button", onclick: end, "방송 종료" }
                },
                Phase::Ending { .. } => rsx! {
                    button { class: "button end-button", r#type: "button", disabled: true, "끝내는 중…" }
                },
                Phase::Ended { id } => rsx! {
                    p { class: "publisher-status", role: "status", "방송을 종료했습니다" }
                    Link { class: "back-link", to: Route::BroadcastDetail { segment: id.to_string() }, "시청 화면 열기 →" }
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
  dioxus.send({{ kind: "error", ...chzzkPublisher.describe(e) }});
}}"#
    );
    let mut eval = document::eval(&script);
    let message: PublisherMessage = eval.recv().await.map_err(|_| {
        "송출 화면에 문제가 생겼어요. 새로고침한 뒤 다시 시도해 주세요.".to_string()
    })?;
    match message {
        PublisherMessage::Ok { id } => Ok(id),
        PublisherMessage::Error(failure) => Err(failure.message()),
    }
}

/// 송출 스크립트를 띄운 상태에서 JavaScript 식 하나의 값을 읽는다.
async fn ask<T: serde::de::DeserializeOwned>(expression: &str) -> Option<T> {
    document::eval(&format!("{PUBLISHER_JS}\nreturn {expression};"))
        .join::<T>()
        .await
        .ok()
}

/// 받은 글자를 클립보드에 복사한다. 최신 클립보드 API가 막힌 브라우저(HTTP 주소 등)에서는
/// 숨긴 입력칸을 선택해 복사하는 예전 방식으로 한 번 더 시도한다.
const COPY_SCRIPT: &str = r#"
const text = await dioxus.recv();
try {
  await navigator.clipboard.writeText(text);
  return true;
} catch (_) {
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  const ok = document.execCommand("copy");
  area.remove();
  if (!ok) throw new Error("copy failed");
  return true;
}
"#;

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
/// - `end()`: 종료 API를 부른 뒤 `stop()`한다.
/// - `leave()`: 송출 화면을 떠날 때(앱 안의 이동) keepalive 요청으로 종료를 시도하고 `stop()`한다.
///   탭을 닫거나 다른 사이트로 갈 때(pagehide)도 같은 요청을 보낸다.
/// - `switchCamera()`: 전면·후면 카메라를 바꾼다. 송출 중이면 보내는 영상 트랙만 바꿔 끼워 연결은 그대로 둔다.
/// - `listen(callback)`: 송출 중에 생기는 일을 `callback`으로 알린다.
///   `{ kind: "connection_lost" }`(연결이 끊겨 종료 API를 부르고 카메라를 껐다),
///   `{ kind: "wake_lock", held }`(화면 꺼짐 막기를 걸었는지. 걸 때마다, 다시 걸 때마다 알린다).
/// - `describe(error)`: 에러를 `{ code }`(브라우저에서 생긴 실패) 또는 `{ message }`(서버가 준 문구)로 바꾼다.
///   코드를 한국어 안내로 바꾸는 일은 Rust 쪽 `Failure::message`가 한다.
///
/// 상태(`stream`, `pc`, `id`, `session`, `listener` 등)는 객체 안에 둔다.
/// 송출·종료 요청은 같은 출처 요청이라 로그인 쿠키가 실리고, 서버가 소유자인지 확인한다.
const PUBLISHER_JS: &str = r#"
window.chzzkPublisher ??= (() => {
  const state = {
    stream: null, pc: null, video: null, id: null, session: null, facing: "environment",
    videoSender: null, wakeLock: null, wantWakeLock: false, listener: null,
  };

  // 화면(Rust 쪽)에 알린다. 화면이 이미 사라졌으면 조용히 넘어간다.
  function emit(event) {
    try { state.listener?.(event); } catch {}
  }

  function listen(callback) {
    state.listener = callback;
  }

  class PublisherError extends Error {
    constructor(message, code) {
      super(message);
      this.code = code;
    }
  }
  // 서버가 준 문구로 실패한다.
  const fail = (message) => { throw new PublisherError(message); };
  // 안내 코드로 실패한다. 문구는 Rust 쪽에서 고른다.
  const failCode = (code) => { throw new PublisherError(code, code); };

  function videoElement() {
    return document.getElementById("publisher-video");
  }

  async function openCamera() {
    if (!window.isSecureContext || !navigator.mediaDevices?.getUserMedia) {
      failCode("insecure");
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
    }).catch(() => failCode("server_unreachable"));
    const body = await res.json().catch(() => ({}));
    if (res.status !== 201) fail(body.message || "방송을 시작하지 못했어요. 다시 시도해 주세요.");
    state.id = body.id;
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
        if (pc.connectionState === "failed" || pc.connectionState === "closed") reject(connectFailed());
      };
      pc.addEventListener("connectionstatechange", check);
      check();
      setTimeout(() => reject(connectFailed()), 15000);
    });
  }

  const connectFailed = () => new PublisherError("connect_failed", "connect_failed");

  async function publish() {
    const pc = new RTCPeerConnection();
    state.pc = pc;
    const [videoTrack] = state.stream.getVideoTracks();
    const [audioTrack] = state.stream.getAudioTracks();
    const videoTransceiver = pc.addTransceiver(videoTrack, { direction: "sendonly", streams: [state.stream] });
    state.videoSender = videoTransceiver.sender;
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
      headers: { "Content-Type": "application/sdp" },
      body: pc.localDescription.sdp,
    }).catch(() => { throw connectFailed(); });
    if (res.status !== 201) {
      // 5xx(MediaMTX에 닿지 못함 등)는 연결 실패로 보고 같은 와이파이인지 확인하라고 안내한다.
      // 4xx는 서버가 거절한 이유(로그인 필요, 이미 끝난 방송 등)를 그대로 보여 준다.
      const body = res.status < 500 ? await res.json().catch(() => ({})) : {};
      if (body.message) fail(body.message);
      throw connectFailed();
    }
    state.session = res.headers.get("Location");
    await pc.setRemoteDescription({ type: "answer", sdp: await res.text() });
    await waitConnected(pc);
    watchConnection(pc);
  }

  // 송출 중에 연결이 끊기면 방송을 끝낸다. disconnected는 잠깐 끊겼다 돌아오기도 하므로 10초 기다린다.
  function watchConnection(pc) {
    let timer = null;
    pc.addEventListener("connectionstatechange", () => {
      if (state.pc !== pc) return;
      clearTimeout(timer);
      const s = pc.connectionState;
      if (s === "failed" || s === "closed") connectionLost();
      else if (s === "disconnected") {
        timer = setTimeout(() => {
          if (state.pc === pc && pc.connectionState === "disconnected") connectionLost();
        }, 10000);
      }
    });
  }

  function connectionLost() {
    if (state.id == null) return;
    requestEnd(false);
    stop();
    emit({ kind: "connection_lost" });
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
    state.wantWakeLock = true;
    await holdWakeLock();
    return state.id;
  }

  function closeConnection() {
    releaseWakeLock();
    state.pc?.close();
    state.pc = null;
    state.videoSender = null;
    state.session = null;
  }

  // 송출하는 동안 화면이 꺼지지 않게 한다. 지원하지 않거나 거절되면 그냥 넘어가고,
  // 걸었는지를 화면에 알려 못 걸었으면 "화면을 켜 두세요" 안내를 띄우게 한다.
  async function holdWakeLock() {
    if (!state.wantWakeLock) return;
    if (!holdsWakeLock() && navigator.wakeLock) {
      try {
        const lock = await navigator.wakeLock.request("screen");
        state.wakeLock = lock;
        // 브라우저가 풀면 화면이 보이는 동안에는 바로 다시 건다. 숨어 있으면 돌아올 때(visibilitychange) 건다.
        lock.addEventListener("release", () => {
          if (state.wakeLock === lock && document.visibilityState === "visible") holdWakeLock();
        });
      } catch {
        state.wakeLock = null;
      }
    }
    if (state.wantWakeLock) emit({ kind: "wake_lock", held: holdsWakeLock() });
  }

  function releaseWakeLock() {
    state.wantWakeLock = false;
    state.wakeLock?.release().catch(() => {});
    state.wakeLock = null;
  }

  function holdsWakeLock() {
    return !!state.wakeLock && !state.wakeLock.released;
  }

  // 다른 앱을 보고 돌아오면 브라우저가 Wake Lock을 풀어 두므로 다시 건다.
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") holdWakeLock();
  });

  // 다음 카메라의 제약 조건. 휴대폰은 facingMode로 전면·후면을 고르고,
  // facingMode를 모르는 컴퓨터 카메라는 장치 목록에서 다음 카메라를 고른다.
  async function nextCameraConstraints(current) {
    const settings = current?.getSettings?.() ?? {};
    if (settings.facingMode) {
      const facing = settings.facingMode === "user" ? "environment" : "user";
      return { facing, video: { facingMode: { exact: facing }, width: { ideal: 1280 }, height: { ideal: 720 } } };
    }
    const cameras = (await navigator.mediaDevices.enumerateDevices()).filter((d) => d.kind === "videoinput");
    if (cameras.length < 2) failCode("no_other_camera");
    const index = cameras.findIndex((d) => d.deviceId === settings.deviceId);
    const next = cameras[(index + 1) % cameras.length];
    return { facing: state.facing, video: { deviceId: { exact: next.deviceId }, width: { ideal: 1280 }, height: { ideal: 720 } } };
  }

  async function switchCamera() {
    if (!state.stream) await preview();
    const [current] = state.stream.getVideoTracks();
    const next = await nextCameraConstraints(current);
    // 휴대폰은 카메라 두 개를 동시에 못 여는 경우가 많아, 지금 카메라를 먼저 끈다.
    current?.stop();
    let track;
    try {
      [track] = (await navigator.mediaDevices.getUserMedia({ video: next.video, audio: false })).getVideoTracks();
    } catch (e) {
      // 새 카메라를 못 열면 원래 카메라로 되돌린다.
      const deviceId = current?.getSettings?.().deviceId;
      const [back] = (await navigator.mediaDevices.getUserMedia({
        video: deviceId ? { deviceId: { exact: deviceId } } : true,
        audio: false,
      }).catch(() => ({ getVideoTracks: () => [] }))).getVideoTracks();
      if (back) await useVideoTrack(current, back);
      throw e;
    }
    state.facing = next.facing;
    await useVideoTrack(current, track);
  }

  // 스트림·미리보기·보내는 트랙을 새 영상 트랙으로 바꾼다. 연결은 다시 맺지 않는다.
  async function useVideoTrack(old, track) {
    if (old) state.stream.removeTrack(old);
    state.stream.addTrack(track);
    if (state.videoSender) await state.videoSender.replaceTrack(track);
    const video = videoElement();
    if (video) {
      video.srcObject = state.stream;
      await video.play().catch(() => {});
    }
  }

  function stop() {
    closeConnection();
    state.stream?.getTracks().forEach((t) => t.stop());
    state.stream = null;
    state.id = null;
  }

  // 종료 API를 부른다. 실패해도 상태 감시기가 경로가 사라진 것을 보고 끝내므로 결과는 보지 않는다.
  function requestEnd(keepalive) {
    if (state.id == null) return Promise.resolve();
    return fetch(`/api/live/${state.id}/end`, {
      method: "POST",
      keepalive,
    }).catch(() => {});
  }

  // "방송 종료": 종료 API를 부른 뒤 연결을 닫고 카메라·마이크를 끈다.
  async function end() {
    await requestEnd(false);
    stop();
  }

  // 송출 화면을 떠날 때 종료를 시도한다. 페이지가 사라져도 요청이 가도록 keepalive를 쓴다.
  // 앱 안에서 다른 화면으로 가면 Rust 쪽이 `leave()`를 부른다.
  function leave() {
    state.listener = null;
    requestEnd(true);
    stop();
  }

  // 탭을 닫거나 다른 사이트로 갈 때.
  window.addEventListener("pagehide", () => {
    requestEnd(true);
  });

  function describe(e) {
    if (e instanceof PublisherError) return e.code ? { code: e.code } : { message: e.message };
    if (e?.name === "NotAllowedError" || e?.name === "SecurityError") return { code: "permission_denied" };
    if (e?.name === "NotFoundError" || e?.name === "OverconstrainedError") return { code: "no_device" };
    if (e?.name === "NotReadableError") return { code: "device_busy" };
    return { code: "unknown" };
  }

  return { state, preview, start, stop, end, leave, switchCamera, listen, describe };
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn error(json: &str) -> String {
        match serde_json::from_str::<PublisherMessage>(json).unwrap() {
            PublisherMessage::Error(failure) => failure.message(),
            other => panic!("에러가 아닌 메시지: {other:?}"),
        }
    }

    #[test]
    fn 권한_거절은_다시_허용하는_방법을_알려_준다() {
        let message = error(r#"{"kind":"error","code":"permission_denied"}"#);
        assert!(message.contains("권한"), "{message}");
        assert!(message.contains("허용"), "{message}");
    }

    #[test]
    fn https가_아니면_터널의_https_주소로_들어오라고_한다() {
        let message = error(r#"{"kind":"error","code":"insecure"}"#);
        assert!(message.contains("HTTPS"), "{message}");
        assert!(message.contains("터널"), "{message}");
    }

    #[test]
    fn 연결_실패는_같은_와이파이인지_확인하고_다시_시도하라고_한다() {
        let message = error(r#"{"kind":"error","code":"connect_failed"}"#);
        assert!(message.contains("같은 와이파이"), "{message}");
        assert!(message.contains("다시 시도"), "{message}");
    }

    #[test]
    fn 세_가지_안내는_서로_다르다() {
        let permission = error(r#"{"kind":"error","code":"permission_denied"}"#);
        let insecure = error(r#"{"kind":"error","code":"insecure"}"#);
        let connect = error(r#"{"kind":"error","code":"connect_failed"}"#);
        assert_ne!(permission, insecure);
        assert_ne!(insecure, connect);
        assert_ne!(permission, connect);
    }

    #[test]
    fn 서버가_준_문구는_그대로_보여_준다() {
        let message = error(r#"{"kind":"error","message":"제목을 입력해 주세요."}"#);
        assert_eq!(message, "제목을 입력해 주세요.");
    }

    #[test]
    fn 송출_중_연결이_끊기면_방송이_끝난_화면으로_바뀌고_끊겼다고_알린다() {
        let event: PublisherEvent = serde_json::from_str(r#"{"kind":"connection_lost"}"#).unwrap();
        let (phase, notice) = event.apply(&Phase::Live { id: 3 }).unwrap();
        assert_eq!(phase, Phase::Ended { id: 3 });
        assert!(notice.contains("송출 연결이 끊겨"), "{notice}");
        assert!(notice.contains("종료"), "{notice}");
    }

    #[test]
    fn 송출_중이_아닐_때_연결이_끊긴_소식은_무시한다() {
        let event: PublisherEvent = serde_json::from_str(r#"{"kind":"connection_lost"}"#).unwrap();
        for phase in [
            Phase::Preview,
            Phase::Connecting,
            Phase::Ending { id: 3 },
            Phase::Ended { id: 3 },
        ] {
            assert!(event.apply(&phase).is_none(), "{phase:?}");
        }
    }

    #[test]
    fn 화면_꺼짐_막기를_다시_못_걸면_화면을_켜_두라고_한다() {
        let lost: PublisherEvent =
            serde_json::from_str(r#"{"kind":"wake_lock","held":false}"#).unwrap();
        assert_eq!(lost, PublisherEvent::WakeLock { held: false });
        let held: PublisherEvent =
            serde_json::from_str(r#"{"kind":"wake_lock","held":true}"#).unwrap();
        assert_eq!(held, PublisherEvent::WakeLock { held: true });
    }

    #[test]
    fn 모르는_코드는_새로고침을_권한다() {
        let message = error(r#"{"kind":"error","code":"something_new"}"#);
        assert!(message.contains("새로고침"), "{message}");
    }
}
