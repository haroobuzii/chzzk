# 06: README 작성과 step-2 제출 준비

**What to build:** 채점자가 README만 읽고 2단계를 이해하고 따라 할 수 있게 한다. 1단계 내용은 유지하고 아래를 더한다.

- 2단계 구현 범위와 범위 밖 항목
- 구성도(우리 서버, MediaMTX, 터널, 같은 와이파이)
- 새 환경변수(`MEDIAMTX_WEBRTC_URL`, `MEDIAMTX_API_URL`, `LAN_IP`) 표와 예시, 와이파이 IP 찾는 법
- `cloudflared` 설치와 터널 실행
- 카메라·마이크 권한
  - iOS Safari와 Android Chrome에서 허용하는 법
  - 거절했을 때 다시 허용하는 법
- 직접 확인 절차
  1. 휴대폰에서 송출 시작
  2. 컴퓨터의 "지금 라이브"에서 시청
  3. 카메라 전환
  4. 송출 종료 → 시청 화면과 목록에 종료 표시
  5. 송출 화면을 닫아 자동 종료
- 화면 잠금·앱 전환 시 자동 종료된다는 안내
- 테스트 실행 방법과 실제 결과

실제 비밀 값은 넣지 않는다. 1단계와 같은 공개 저장소에 올리고 `step-2` 태그를 붙인다. 공개 저장소에 올리는 일은 사용자에게 먼저 확인한다.

**Blocked by:** 03, 04, 05

**Status:** ready-for-agent

- [x] README에 위 항목이 모두 있고, 1단계 항목도 남아 있다
- [x] README의 명령(`docker compose up -d`, `dx serve`, `cloudflared`)을 실제로 실행해 확인했다
- [x] 테스트 결과(통과 개수)가 실제 실행 결과와 같다
- [x] 저장소에 비밀 값이 없다
- [ ] 사용자 확인 뒤 GitHub에 올리고 `step-2` 태그를 붙인다. 제출 주소는 `tree/step-2`다
