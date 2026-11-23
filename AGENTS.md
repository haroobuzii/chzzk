# 작업지침

- 일반적인 작업 흐름
  - `/setup-matt-pocock-skills` : 최초 한번
  - `/grill-with-docs` : 큰 작업을 기획할 때
  - `/to-spec`
  - `/to-tickets`
  - `/implement-spec`
  - `/retro`
- 간단한 작업은 위 흐름을 타지 않아도 된다.
- 위 명령은 사용자가 직접 입력하는 스킬이라 에이전트의 스킬 목록에는 보이지 않는다. 모두 `~/.claude/skills/`에 설치되어 있다.

## 참고 문서

- 작업 폴더(worktree)에서 일하거나 도우미를 여럿 띄울 때: `docs/agents/parallel-work.md`
- MediaMTX(라이브 송출·시청, WHIP/WHEP, 관리 API)를 다룰 때: `docs/mediamtx.md`
- 코드를 검토할 때: `CODING_STANDARDS.md`
- 토스 결제(후원)를 다룰 때: `docs/toss-payments.md`
- 데스크톱 앱 빌드·배포·맥 서명을 다룰 때: `docs/desktop-release.md`
- 배포 서버 자동 배포(GHCR, SSH, Secrets)를 다룰 때: `docs/deploy.md`
- 비밀 키를 확인할 때: `scripts/check-secrets.sh`만 쓴다. 키 값은 출력하거나 이 컴퓨터 밖(GitHub 검색, 웹 요청 등)으로 보내지 않는다.
- 단계마다 과제 원문(체크리스트 포함): `.scratch/<기능>/assignment.md`. 새 단계를 시작하면 원문부터 여기에 저장한다.

## Agent skills

### Issue tracker

Issues live as local markdown files under `.scratch/<feature>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Default canonical labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
