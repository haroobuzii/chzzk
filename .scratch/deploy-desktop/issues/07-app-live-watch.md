# 07: 앱 라이브 시청

**What to build:** 앱에서 방송 중인 라이브를 눌러 앱 안에서 시청한다. 웹뷰가 WebRTC를 쓸 수 없으면 "웹에서 시청하기"를 보여 준다.

- 웹뷰 JavaScript가 서버의 WHEP 절대 주소로 연결한다(02의 CORS).
- WebRTC 지원 여부를 먼저 확인하고, 없으면 안내와 "웹에서 시청하기" 버튼을 보여 준다.
- 방송이 끝나면 웹과 같은 "방송이 끝났어요" 상태가 된다.
- 맥(WKWebView)과 Windows(WebView2)에서 각각 확인한 결과를 기록한다.

**Blocked by:** 06

**Status:** done

- [x] 맥 앱에서 로컬 서버의 라이브를 시청한다(또는 안 되면 안내가 뜬다).
- [x] 방송 종료가 앱에 반영된다.
- [x] 확인 결과(OS별 되는지)를 README 기록용 메모에 남긴다. (`.scratch/deploy-desktop/manual-checks.md`, 맥 완료·Windows는 나중에 채운다)
