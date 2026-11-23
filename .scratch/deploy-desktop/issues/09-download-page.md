# 09: 다운로드 페이지 `/download`

**What to build:** 웹 머리글이나 첫 화면의 "앱 다운로드"로 다운로드 페이지에 간다. OS별 링크, 지원 아키텍처·최소 OS, 설치·실행 방법, iOS TestFlight 링크를 본다.

- 링크: 08의 고정 Release 주소(Windows x64, macOS Apple Silicon, macOS Intel).
- 설치 안내: Windows SmartScreen "추가 정보 → 실행", 맥은 `.dmg` 열고 응용 프로그램으로 끌기.
- TestFlight 링크는 설정 값. 비어 있으면 "준비 중".
- 앱은 보기 전용이고 로그인·댓글·후원은 웹에서 한다는 안내.

**Blocked by:** 08

**Status:** ready-for-agent

- [ ] 서버가 그린 HTML에 OS별 링크와 아키텍처 문구가 있다(라우터 테스트).
- [ ] TestFlight 링크 설정이 없으면 "준비 중"이 나온다.
- [ ] 머리글에서 다운로드 페이지로 갈 수 있다.
- [ ] README 테스트 개수를 맞춘다.
