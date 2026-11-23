# 6단계 손으로 한 확인 (README 기록용)

## 앱 라이브 시청 (티켓 07)

| OS | 웹뷰 | 앱 안 라이브 시청 | 방송 종료 반영 | WebRTC가 없을 때 "웹에서 시청하기" |
| --- | --- | --- | --- | --- |
| macOS 15 (Darwin 24.6) | WKWebView (출처 `dioxus://index.html`) | 된다 | 된다 | 보인다 |
| Windows | WebView2 | (나중에 채운다) | | |

### macOS 확인 방법 (2026-11-23)

- 로컬 서버(`dx serve --port 8007`, `LIVE_PATH_PREFIX=live-t07`)에 라이브를 만들고, ffmpeg 9의 WHIP 송출(640x360 시험 화면 + 사인파)로 MediaMTX `live-t07/<번호>`에 영상을 넣었다.
- 앱: `CHZZK_APP_SERVER_URL=http://127.0.0.1:8007 dx serve --desktop --no-default-features --features app --fullstack false`.
- 화면이 잠겨 있어 창을 직접 보지 못했다. 대신 임시 확인 코드(커밋하지 않음)로 상세 화면에 들어가 3초마다 웹뷰의 화면 글자와 `<video>` 상태를 앱 로그에 찍었다.
  - 시청: `RTCPeerConnection` 있음, `<video>` 640x360, `readyState` 4, `currentTime`이 계속 늘었다. MediaMTX에 이 경로의 `read` 세션이 연결됨(`peerConnectionEstablished: true`)으로 보였다. 서버의 WHEP 절대 주소와 앱 출처용 CORS로 연결된다.
  - 종료: `POST /api/live/1/end` 뒤 5초 안에 재생기가 사라지고 "방송이 종료되었습니다"와 "방송한 시간"이 보였다. MediaMTX의 `read` 세션도 사라졌다.
  - 대체 안내: 웹뷰에서 `window.RTCPeerConnection = undefined`로 지운 뒤 들어가면 "이 앱에서는 실시간 시청을 할 수 없어요", "웹에서 시청하기" 버튼이 보였다.
- 확인하지 못한 것: "소리 켜기"를 눌러 소리가 나는지(화면 잠김으로 누를 수 없었다).
