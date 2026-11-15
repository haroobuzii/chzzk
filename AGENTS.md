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

## Agent skills

### Issue tracker

Issues live as local markdown files under `.scratch/<feature>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Default canonical labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
