# 05: 서버 자동 배포

**What to build:** main에 푸시하면 테스트가 통과한 경우에만 배포 서버가 새 버전으로 바뀌고, 끝난 뒤 `/healthz`로 확인된다.

- 기존 CI 검사에 actionlint(워크플로)와 shellcheck(셸 스크립트)를 더한다.
- 배포 워크플로(main 푸시)
  1. 검사·테스트 통과
  2. `ubuntu-24.04-arm` 러너에서 이미지 빌드, GHCR에 커밋 해시 태그와 `latest`로 올림
  3. SSH로 서버에 들어가 `IMAGE_TAG`를 새 해시로 두고 `docker compose pull && up -d`
  4. `https://chzzk.haroobuzii.com/healthz`의 `version`이 새 해시가 될 때까지 기다림(제한 시간 넘기면 실패)
- 배포는 한 번에 하나만 돈다(concurrency).
- GitHub Secrets: SSH 개인 키(배포 전용 키 새로 만들기), 서버 주소, known_hosts. **Secrets 등록 전에 사용자에게 묻는다.**

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] actionlint와 shellcheck가 CI에서 통과한다.
- [ ] main 푸시 한 번으로 서버 버전이 바뀌고 워크플로가 초록불이다.
- [ ] 테스트가 실패하면 배포 단계가 돌지 않는다.
- [ ] 저장소와 로그에 비밀 값이 나오지 않는다.
