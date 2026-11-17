// 가짜 카메라·마이크. 앱 안 브라우저처럼 진짜 카메라를 쓸 수 없는 곳에서 라이브 송출을 확인할 때 쓴다.
//
// 쓰는 법: 송출 화면(/live/new)이 열린 탭에서, 카메라를 켜기 전에 이 파일 내용을 그대로 실행한다
// (브라우저 자동화 도구의 JavaScript 실행 기능). 그 뒤 페이지가 getUserMedia를 부르면
// 진짜 카메라 대신 아래 가짜 장치가 연결된다.
//
// - 카메라 두 개: 후면(environment, 파란 화면에 BACK)과 전면(user, 주황 화면에 FRONT).
//   facingMode나 deviceId로 고르면 그에 맞는 쪽을 준다. 카메라 전환 확인에 쓴다.
// - 마이크: 440Hz 소리.
// - 화면에 시각이 계속 바뀌어 그려지므로 시청 화면에서 영상이 실제로 흐르는지 눈으로 볼 수 있다.
// - 새로고침하면 사라진다. 다시 실행하면 된다.
(() => {
  if (window.__chzzkFakeCamera) return "이미 설치되어 있다";

  const cameras = {
    environment: { deviceId: "fake-back", label: "가짜 후면 카메라", color: "#1f4fbf", text: "BACK" },
    user: { deviceId: "fake-front", label: "가짜 전면 카메라", color: "#c2611d", text: "FRONT" },
  };

  function videoTrack(camera, width, height) {
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    const draw = () => {
      ctx.fillStyle = camera.color;
      ctx.fillRect(0, 0, width, height);
      ctx.fillStyle = "white";
      ctx.font = `bold ${Math.round(height / 6)}px sans-serif`;
      ctx.fillText(camera.text, width * 0.08, height * 0.4);
      ctx.font = `${Math.round(height / 10)}px monospace`;
      ctx.fillText(new Date().toLocaleTimeString(), width * 0.08, height * 0.7);
    };
    draw();
    const timer = setInterval(draw, 100);
    const track = canvas.captureStream(15).getVideoTracks()[0];
    track.addEventListener("ended", () => clearInterval(timer));
    // stop()으로 끝낼 때도 그리기를 멈춘다.
    const stop = track.stop.bind(track);
    track.stop = () => {
      clearInterval(timer);
      stop();
    };
    // 페이지가 어느 카메라인지 알 수 있게 설정 값을 흉내 낸다.
    const getSettings = track.getSettings.bind(track);
    track.getSettings = () => ({
      ...getSettings(),
      deviceId: camera.deviceId,
      facingMode: camera === cameras.user ? "user" : "environment",
    });
    return track;
  }

  function audioTrack() {
    const audio = new AudioContext();
    const oscillator = audio.createOscillator();
    const gain = audio.createGain();
    gain.gain.value = 0.05;
    oscillator.frequency.value = 440;
    const destination = audio.createMediaStreamDestination();
    oscillator.connect(gain).connect(destination);
    oscillator.start();
    return destination.stream.getAudioTracks()[0];
  }

  // 제약 조건에서 원하는 카메라를 고른다. 아무것도 없으면 후면.
  function pickCamera(video) {
    if (!video || video === true) return cameras.environment;
    const wanted = (value) => (value && typeof value === "object" ? value.exact ?? value.ideal : value);
    const deviceId = wanted(video.deviceId);
    if (deviceId === cameras.user.deviceId) return cameras.user;
    if (deviceId === cameras.environment.deviceId) return cameras.environment;
    return wanted(video.facingMode) === "user" ? cameras.user : cameras.environment;
  }

  function size(video, key, fallback) {
    const value = video && typeof video === "object" ? video[key] : undefined;
    if (typeof value === "number") return value;
    return (value && (value.exact ?? value.ideal)) || fallback;
  }

  navigator.mediaDevices.getUserMedia = async (constraints = {}) => {
    const tracks = [];
    if (constraints.video) {
      const video = constraints.video;
      tracks.push(videoTrack(pickCamera(video), size(video, "width", 1280), size(video, "height", 720)));
    }
    if (constraints.audio) tracks.push(audioTrack());
    return new MediaStream(tracks);
  };

  navigator.mediaDevices.enumerateDevices = async () => [
    ...Object.values(cameras).map((camera) => ({
      kind: "videoinput",
      deviceId: camera.deviceId,
      groupId: "fake",
      label: camera.label,
      toJSON() {
        return this;
      },
    })),
    { kind: "audioinput", deviceId: "fake-mic", groupId: "fake", label: "가짜 마이크", toJSON() { return this; } },
  ];

  window.__chzzkFakeCamera = true;
  return "가짜 카메라를 설치했다";
})();
