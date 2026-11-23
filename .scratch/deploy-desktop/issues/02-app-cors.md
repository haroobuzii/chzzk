# 02: 앱용 접근 허용(CORS)

**What to build:** 데스크톱·iOS 앱의 웹뷰가 배포 서버의 공개 읽기와 시청 연결을 부를 수 있다. 그 밖의 요청은 지금처럼 같은 출처만 받는다.

- 허용 출처: 앱 웹뷰 출처(`dioxus://index.html`, `http://dioxus.index.html` 계열)뿐이다.
- 허용 경로
  - 공개 읽기: 방송 목록·단건 GET, 녹화 영상 GET(Range 포함)
  - 시청 연결(WHEP): POST·PATCH·DELETE와 사전 요청(OPTIONS)
- 쿠키 허용 헤더(`Access-Control-Allow-Credentials`)는 붙이지 않는다.
- 로그인·댓글·후원·등록·송출 같은 상태 변경 요청은 앱 출처에서 와도 지금처럼 403이다(ADR 0006 CSRF).
- 앱이 부를 GET JSON 주소(목록·단건)를 스펙 계약으로 문서화한다. 없으면 기존 공개 API를 그대로 쓴다.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 앱 출처로 보낸 목록·단건·영상 GET에 허용 헤더가 붙고, 쿠키 허용 헤더는 없다.
- [ ] 앱 출처로 보낸 WHEP 사전 요청과 POST에 허용 헤더가 붙는다.
- [ ] 모르는 출처에는 허용 헤더가 없다.
- [ ] 앱 출처로 보낸 로그인·댓글·후원 요청은 403이다.
- [ ] 기존 테스트가 모두 통과하고 README 테스트 개수를 맞춘다.
