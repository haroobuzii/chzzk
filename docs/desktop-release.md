# 데스크톱 앱 배포(녹화방송)

main에 올리면 `.github/workflows/app-release.yml`이 Windows·맥 설치 파일을 만들어 GitHub Release `app-latest`에 덮어 올린다. 결정 배경은 ADR 0009·0010, 조사 메모는 `docs/deploy-notes.md` 5~7장이다.

## 무엇이 어디에 올라가나

| 러너 | 파일 | 다운로드 주소 |
| --- | --- | --- |
| `windows-latest`(x64) | `chzzk-windows-x64.msi` | `https://github.com/haroobuzii/chzzk/releases/download/app-latest/chzzk-windows-x64.msi` |
| `macos-15`(arm64) | `chzzk-macos-arm64.dmg` | `.../releases/download/app-latest/chzzk-macos-arm64.dmg` |
| `macos-15-intel`(x64) | `chzzk-macos-x64.dmg` | `.../releases/download/app-latest/chzzk-macos-x64.dmg` |

- CI와 같은 검사·테스트(`ci.yml`)가 통과해야 묶기를 시작한다. 도는 중인 실행은 새 푸시가 와도 끊지 않는다.
- 세 파일이 모두 만들어졌을 때만 Release를 바꾼다. 태그 `app-latest`는 그때의 main 커밋으로 옮긴다.
- 앱이 부르는 서버는 `https://chzzk.haroobuzii.com`이다(`CHZZK_APP_SERVER_URL`).
- `workflow_dispatch`로 손으로도 돌릴 수 있다.

## 내 컴퓨터에서 같은 파일 만들기

```sh
scripts/bundle-app.sh dmg chzzk-macos-arm64.dmg   # 결과: dist/chzzk-macos-arm64.dmg
```

CI도 이 스크립트를 그대로 부른다. dx 0.7.10이 필요하다(`cargo binstall dioxus-cli@0.7.10`). 스크립트는 `.dmg` 안 앱 이름이 "녹화방송"인지와 서명이 유효한지 확인한다.

## 앱만 빌드하는 방법(서버는 빌드하지 않는다)

dx는 `dioxus` 의존성이나 고른 기능 목록에 `dioxus/fullstack`이 바로 적혀 있으면 서버도 함께 빌드한다(dx 0.7.10 `BuildRequest::fullstack_feature_enabled`). 그래서 `Cargo.toml`에서

- `dioxus` 의존성에서 `fullstack`을 빼고 `web`·`server` 기능이 켠다. `dx serve`와 `dx bundle --web`은 전처럼 서버까지 빌드한다.
- `app` 기능은 `app-fullstack`을 거쳐 fullstack을 켠다. 공유 화면 코드가 서버 함수 타입을 쓰므로 fullstack 자체는 필요하지만, dx는 한 단계 아래는 보지 않아 앱만 빌드한다.

## 앱 이름

`Dioxus.toml`의 `[bundle.macos] bundle_name = "녹화방송"`이 맥 `.app` 이름과 `CFBundleName`·`CFBundleDisplayName`이 된다. 실행 파일(`chzzk`)과 번들 ID(`com.haroobuzii.chzzk`)는 ASCII 그대로다.

Windows 설치 프로그램(MSI)의 제품 이름은 dx가 크레이트 이름에서 만든 `Chzzk`다. dx 기본 WiX 템플릿이 코드페이지 1252라 한글 이름을 넣으려면 템플릿을 따로 만들어야 한다. 앱 창 제목은 "녹화방송"이다.

## 맥 서명·공증

Secrets 여섯 개가 **모두** 있으면 Developer ID로 서명하고 공증한다. 하나라도 없으면 ad-hoc 서명만 한 앱을 만들고 실행 로그에 알림(notice)을 남긴다. ad-hoc 앱은 처음 열 때 막히므로 **시스템 설정 → 개인정보 보호 및 보안 → 그래도 열기**를 눌러야 한다.

| Secret 이름 | 넣을 값 |
| --- | --- |
| `APPLE_CERTIFICATE_P12_BASE64` | "Developer ID Application" 인증서와 개인 키를 내보낸 `.p12` 파일을 base64로 바꾼 것(`base64 -i cert.p12 \| pbcopy`) |
| `APPLE_CERTIFICATE_PASSWORD` | `.p12`를 내보낼 때 정한 암호 |
| `APPLE_TEAM_ID` | Apple 개발자 계정의 팀 ID(10자). 서명한 앱의 팀이 이것과 같은지 확인한다 |
| `APPLE_API_KEY_ID` | App Store Connect API 키의 Key ID |
| `APPLE_API_ISSUER_ID` | App Store Connect API 키의 Issuer ID |
| `APPLE_API_KEY_P8_BASE64` | 내려받은 `AuthKey_<KeyID>.p8`을 base64로 바꾼 것 |

- 값은 GitHub Secrets에만 넣는다. 이 저장소나 로그에 남기지 않는다.
- App Store Connect API 키는 iOS TestFlight 업로드(티켓 10)와 같이 쓴다. 역할은 "Developer" 이상이면 된다.
- 스크립트는 이 값을 dx가 읽는 이름(`APPLE_CERTIFICATE`, `APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_KEY_PATH`)으로 바꿔 넘긴다. dx가 `.app`과 `.dmg`를 hardened runtime으로 서명하고 `notarytool`로 공증한 뒤 staple한다.
- 서명했을 때는 팀 ID, `spctl`(Gatekeeper), `stapler validate`까지 확인하고, 하나라도 안 맞으면 실패한다.

## Windows

서명하지 않는다(ADR 0010). 처음 실행하면 SmartScreen이 막으므로 **추가 정보 → 실행**을 누른다. WiX는 dx가 알아서 내려받는다.
