# 10: iOS 앱 TestFlight ✋

**What to build:** main에 푸시하면 iOS 앱이 빌드·서명되어 TestFlight에 새 빌드로 올라가고, 아이폰 사용자가 공개 링크로 설치한다.

- 06의 앱 코드를 iOS로 빌드(같은 화면, 녹화 재생, 라이브는 되면 시청 아니면 웹 안내).
- macos 러너에서 서명(배포 인증서·프로비저닝 프로필)하고 App Store Connect API 키로 업로드.
- 빌드 번호는 실행 번호로 자동 증가.
- Secrets가 없으면 이 작업을 건너뛴다.
- 사용자와 함께: Apple 개발자 승인 확인, App Store Connect 앱 등록, API 키 만들기, 외부 테스트 그룹과 공개 링크, 수출 규정 답변. Secrets 등록은 묻고 진행.
- 공개 링크를 다운로드 페이지 설정 값에 넣는다.

**Blocked by:** 06, Apple 개발자 승인

**Status:** ready-for-human

- [ ] 푸시 후 TestFlight에 새 빌드가 보인다.
- [ ] 공개 링크로 아이폰에 설치해 목록·녹화 재생이 된다(사람 확인).
- [ ] 다운로드 페이지에 TestFlight 링크가 나온다.
