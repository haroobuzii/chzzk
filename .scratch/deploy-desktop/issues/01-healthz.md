# 01: 상태 확인 주소 `/healthz`

**What to build:** 운영자와 자동 배포가 `GET /healthz`로 서버 상태를 한눈에 본다.

- DB에 간단한 질의가 되면 200, 안 되면 503이다.
- 본문은 `{status, db, mediamtx, version}` JSON이다.
  - `db`: `"ok"` 또는 `"down"`
  - `mediamtx`: 관리 API에 닿으면 `"ok"`, 아니면 `"unreachable"`. 녹화·로그인은 되므로 이때도 200을 유지한다.
  - `version`: 빌드할 때 넣은 커밋 해시. 없으면 `"dev"`.
- 로그인 없이 부를 수 있고, 캐시하지 않는다.

스펙의 "백엔드 변경 → 상태 확인"을 따른다.

**Blocked by:** None (can start immediately)

**Status:** done

- [x] DB 정상, 중계소 정상이면 200과 `db: "ok"`, `mediamtx: "ok"`다(가짜 MediaMTX).
- [x] 중계소에 닿지 않으면 200과 `mediamtx: "unreachable"`이다.
- [x] DB에 닿지 않으면 503과 `db: "down"`이다(닫힌 풀로 흉내).
- [x] 빌드 때 넣은 버전이 응답에 나온다.
- [x] 기존 테스트가 모두 통과하고 README 테스트 개수를 맞춘다.
