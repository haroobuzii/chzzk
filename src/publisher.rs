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

#[component]
pub fn NewLive() -> Element {
    let mut phase = use_signal(|| Phase::Preview);
    let mut error = use_signal(|| None::<String>);
    let mut title = use_signal(String::new);
    // 송출 중에 화면 꺼짐 막기를 걸었는지. 못 걸었으면 "화면을 켜 두세요" 안내를 띄운다.
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
                Ok(id) => {
                    wake_lock.set(
                        ask::<bool>("chzzkPublisher.holdsWakeLock()")
                            .await
                            .unwrap_or(false),
                    );
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
            button {
                class: "button secondary-button",
                r#type: "button",
                disabled: connecting || switching(),
                onclick: switch_camera,
                if switching() { "카메라 바꾸는 중…" } else { "카메라 전환" }
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
/// - `switchCamera()`: 전면·후면 카메라를 바꾼다. 송출 중이면 보내는 영상 트랙만 바꿔 끼워 연결은 그대로 둔다.
/// - `holdsWakeLock()`: 화면 꺼짐 막기(Screen Wake Lock)를 걸고 있는지 알려 준다.
/// - `describe(error)`: 에러를 `{ code }`(브라우저에서 생긴 실패) 또는 `{ message }`(서버가 준 문구)로 바꾼다.
///   코드를 한국어 안내로 바꾸는 일은 Rust 쪽 `Failure::message`가 한다.
///
/// 상태(`stream`, `pc`, `video`, `id`, `key`, `session`)는 객체 안에 둔다. 송출 키는 여기에만 있다.
const PUBLISHER_JS: &str = r#"
window.chzzkPublisher ??= (() => {
  const state = {
    stream: null, pc: null, video: null, id: null, key: null, session: null, facing: "environment",
    videoSender: null, wakeLock: null, wantWakeLock: false,
  };

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
      headers: { "Content-Type": "application/sdp", Authorization: `Bearer ${state.key}` },
      body: pc.localDescription.sdp,
    }).catch(() => { throw connectFailed(); });
    if (res.status !== 201) {
      // 5xx(MediaMTX에 닿지 못함 등)는 연결 실패로 보고 같은 와이파이인지 확인하라고 안내한다.
      // 4xx는 서버가 거절한 이유(키 불일치, 이미 끝난 방송 등)를 그대로 보여 준다.
      const body = res.status < 500 ? await res.json().catch(() => ({})) : {};
      if (body.message) fail(body.message);
      throw connectFailed();
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
  // 화면은 `holdsWakeLock()`이 false인 것을 보고 "화면을 켜 두세요" 안내를 띄운다.
  async function holdWakeLock() {
    if (!state.wantWakeLock || !navigator.wakeLock) return;
    if (state.wakeLock && !state.wakeLock.released) return;
    try {
      state.wakeLock = await navigator.wakeLock.request("screen");
    } catch {
      state.wakeLock = null;
    }
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
    state.key = null;
  }

  function describe(e) {
    if (e instanceof PublisherError) return e.code ? { code: e.code } : { message: e.message };
    if (e?.name === "NotAllowedError" || e?.name === "SecurityError") return { code: "permission_denied" };
    if (e?.name === "NotFoundError" || e?.name === "OverconstrainedError") return { code: "no_device" };
    if (e?.name === "NotReadableError") return { code: "device_busy" };
    return { code: "unknown" };
  }

  return { state, preview, start, stop, switchCamera, holdsWakeLock, describe };
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
    fn 모르는_코드는_새로고침을_권한다() {
        let message = error(r#"{"kind":"error","code":"something_new"}"#);
        assert!(message.contains("새로고침"), "{message}");
    }
}
