# 08: 앱 자동 빌드와 다운로드 파일

**What to build:** main에 푸시하면 Windows·맥 앱 파일이 만들어져, 고정 태그 `app-latest`의 GitHub Release에 같은 이름으로 올라간다. 다운로드 링크는 항상 최신을 가리킨다.

- 매트릭스: Windows x64(windows 러너, MSI), macOS arm64·x64(macos 러너, `.dmg`).
- 파일 이름 고정: `chzzk-windows-x64.msi`, `chzzk-macos-arm64.dmg`, `chzzk-macos-x64.dmg`.
- 맥 앱은 Developer ID 서명·공증. Secrets(인증서, 암호, 공증 API 키)가 없으면 서명 없이 만들고 로그에 알린다.
- 서버 주소는 운영 주소로 빌드한다.
- Secrets 등록과 Apple 인증서 만들기는 사용자와 함께 한다(묻고 진행).

**Blocked by:** 06

**Status:** ready-for-agent

- [ ] 푸시 후 Release `app-latest`에 세 파일이 올라간다.
- [ ] Windows에서 설치·실행되어 배포 서버 목록이 보인다(사람 확인).
- [ ] 맥에서 서명·공증된 앱이 경고 없이 열린다(서명 키 등록 뒤, 사람 확인).
