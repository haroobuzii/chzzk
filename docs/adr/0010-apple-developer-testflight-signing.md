# iOS 앱은 TestFlight 공개 링크로 배포하고, macOS 앱도 같은 Apple 개발자 계정으로 서명·공증한다

강사가 iOS 앱 배포까지 요구해, 사용자가 Apple 개발자 프로그램(개인, 연 $99)에 가입한다. 개인 등록이라 TestFlight와 앱 정보에 사용자의 실명이 판매자 이름으로 보인다(사용자 동의). iOS 앱(번들 ID `com.haroobuzii.chzzk`, 이름 "녹화방송")은 main 푸시마다 GitHub Actions가 빌드해 TestFlight에 올리고, 외부 테스트 공개 링크로 누구나 설치하게 한다. 첫 외부 배포는 Apple의 베타 심사를 받는다.

같은 계정으로 macOS 앱을 Developer ID로 서명하고 공증해, 내려받은 사람이 경고 없이 열 수 있게 한다. Windows 앱 서명은 유료 인증서가 따로 필요해 하지 않고, 다운로드 페이지에 SmartScreen에서 "추가 정보 → 실행"을 누르는 법을 적는다. 서명 인증서와 App Store Connect API 키는 GitHub Secrets에만 둔다.

## Considered Options

- **무료 계정으로 내 아이폰에만 설치**: 7일마다 다시 설치해야 하고 다른 사람에게 배포할 수 없다. 강사 요구(배포)를 채우지 못한다.
- **App Store 정식 출시**: 심사가 더 엄격하고, 웹을 여는 버튼이 중심인 앱은 거절될 수 있다.
- **조직 등록**: 실명 대신 회사 이름이 나오지만 사업자와 D-U-N-S 번호가 필요하다.
