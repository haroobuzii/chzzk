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

**Status:** done (CI run pending)

- [x] actionlint와 shellcheck가 CI에서 통과한다. (로컬 통과, CI 실행 대기)
- [ ] main 푸시 한 번으로 서버 버전이 바뀌고 워크플로가 초록불이다.
- [ ] 테스트가 실패하면 배포 단계가 돌지 않는다.
- [ ] 저장소와 로그에 비밀 값이 나오지 않는다.

**작업 기록 (2026-11-23)**

- `ci.yml`에 `lint` 작업(actionlint 1.7.12, shellcheck, `docker compose config`)과 `workflow_call`을 더했다. 기존 셸 스크립트의 shellcheck 지적은 고치거나(SC2001) 이유를 적고 끔(SC2016 두 곳: 글자 그대로의 백틱, SC2046: 커밋 해시 목록 나누기).
- `deploy.yml`: `ci.yml` 호출 → `ubuntu-24.04-arm`에서 buildx(gha 캐시)로 빌드해 GHCR `:<짧은 해시>`·`:latest` → `scripts/deploy-server.sh`로 SSH 배포와 `/healthz` version 확인(5분). `concurrency: deploy-server`.
- 서버는 GHCR 공개 패키지를 로그인 없이 받는다. 공개로 바꾸는 법과 Secrets(`DEPLOY_SSH_KEY`, `DEPLOY_HOST`, `DEPLOY_KNOWN_HOSTS`)는 `docs/deploy.md`.
- 로컬: actionlint·shellcheck·compose 검사 통과, fmt·세 빌드 검사·테스트 252개·README 개수·비밀 키 검사 통과.
- 남은 일(사람): 배포 전용 키 만들기와 Secrets 등록(사용자 승인 후), main 병합 뒤 첫 실행, GHCR 패키지 공개, 남은 세 항목 확인.
