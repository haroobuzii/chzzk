# 11: README 정리와 실제 확인, 제출 ✋

**What to build:** 채점자와 강사가 README만 보고 ⑥ 전체를 이해하고 확인할 수 있다.

- README에 더할 것(1~4단계 유지)
  - ⑥ 범위와 범위 밖
  - 공개 주소, 다운로드 페이지 주소
  - 도메인 `haroobuzii.com`, 최초 등록 $11.17, 갱신 $11.17/년(Cloudflare, 자동 갱신)
  - DNS(Cloudflare DNS only, A 레코드)와 HTTPS(Caddy, Let's Encrypt)
  - OCI: 싱가포르, `VM.Standard.A1.Flex` 2 OCPU·12 GB, 부트 볼륨 100 GB, Always Free 근거, 재고 부족·Pay As You Go 전환 이유, $1 예산 알림, 7일 저사용 회수 조건
  - 재현 절차(인프라 스크립트 → 서버 준비 → 비밀 값 → DNS → 첫 배포), 자동 배포 흐름
  - 다운로드와 OS별 설치·실행 방법
  - 테스트한 OS·아키텍처와 결과 표(Windows PC, 맥, 아이폰)
  - 리소스 정리 방법
  - 2단계 안내 수정: 배포 서버에서는 같은 와이파이가 아니어도 됨
- 사람 확인을 함께 하고 표에 기록한다.
- 사용자 승인 후 푸시, `step-6` 태그.

**Blocked by:** 01~10

**Status:** ready-for-human

- [ ] README에 과제 원문 체크리스트 항목이 모두 들어 있다.
- [ ] 테스트 개수 검사와 비밀 키 검사가 통과한다.
- [ ] 외부 네트워크에서 공개 링크로 내려받아 실행한 결과가 기록되어 있다.
- [ ] `step-6` 태그가 올라간다(사용자 승인 후).
