# 08: 앱 자동 빌드와 다운로드 파일

**What to build:** main에 푸시하면 Windows·맥 앱 파일이 만들어져, 고정 태그 `app-latest`의 GitHub Release에 같은 이름으로 올라간다. 다운로드 링크는 항상 최신을 가리킨다.

- 매트릭스: Windows x64(windows 러너, MSI), macOS arm64·x64(macos 러너, `.dmg`).
- 파일 이름 고정: `chzzk-windows-x64.msi`, `chzzk-macos-arm64.dmg`, `chzzk-macos-x64.dmg`.
- 맥 앱은 Developer ID 서명·공증. Secrets(인증서, 암호, 공증 API 키)가 없으면 서명 없이 만들고 로그에 알린다.
- 서버 주소는 운영 주소로 빌드한다.
- Secrets 등록과 Apple 인증서 만들기는 사용자와 함께 한다(묻고 진행).

**Blocked by:** 06

**Status:** done (CI run pending)

- [ ] 푸시 후 Release `app-latest`에 세 파일이 올라간다. (CI 확인 대기: main 푸시 뒤 "앱 배포" 실행)
- [ ] Windows에서 설치·실행되어 배포 서버 목록이 보인다(사람 확인).
- [ ] 맥에서 서명·공증된 앱이 경고 없이 열린다(서명 키 등록 뒤, 사람 확인).

**구현 메모**
- 워크플로 `.github/workflows/app-release.yml`, 스크립트 `scripts/bundle-app.sh`, 설명과 Secrets 이름 `docs/desktop-release.md`.
- 로컬 확인(맥 arm64): 워크플로와 같은 `scripts/bundle-app.sh dmg chzzk-macos-arm64.dmg`로 3.7 MB `.dmg`를 만들었다. 서버는 빌드되지 않았다. `.dmg`가 열리고 안에 `녹화방송.app`(CFBundleName·DisplayName "녹화방송", 번들 ID `com.haroobuzii.chzzk`, ad-hoc 서명 유효, 운영 서버 주소 포함)이 있으며 앱이 실행된다. actionlint·shellcheck 통과.
- `dx build --platform web`은 여전히 서버까지 빌드한다(Cargo.toml fullstack 이동 뒤 확인).
- Windows MSI 제품 이름은 `Chzzk`다(dx 기본 WiX 템플릿이 코드페이지 1252). 창 제목은 "녹화방송"이다.
