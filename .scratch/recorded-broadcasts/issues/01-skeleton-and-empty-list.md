# 01: 프로젝트 뼈대와 빈 목록 화면

**What to build:** 저장소 루트에 Dioxus 0.7 fullstack 크레이트를 만들고, 로컬 PostgreSQL(Docker Compose)에 연결한다. 앱을 켜면 첫 화면(`/`)이 실제 DB에서 방송 목록을 읽어 온다. 방송이 없으므로 "아직 방송이 없습니다" 안내와 등록 버튼이 보인다. 화면은 어두운 배경이고, 맨 위에 서비스 이름 "녹화방송"과 "방송 등록" 버튼이 있다. 구성은 단축 URL 과제와 같다. 기본 기능은 web이고, 서버 전용 의존성은 server 기능에서만 켠다. 시작할 때 마이그레이션을 적용한다.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [x] `docker compose up -d` 뒤 `dx serve`로 앱이 켜지고 `/`에 빈 목록 안내가 보인다
- [x] 앱이 시작할 때 방송 표를 마이그레이션으로 만든다. 칸은 방송 번호(`GENERATED ALWAYS AS IDENTITY`), 제목(`CHECK` 1~100자), 영상 키, 등록 시각(`TIMESTAMPTZ`, 기본 현재 시각)이다
- [x] 목록 조회는 서버 함수로 DB를 읽고 등록 시각 내림차순이다
- [x] 환경변수 `DATABASE_URL`, `DATA_DIR`(기본 `./data`), `MAX_UPLOAD_BYTES`(기본 2 GiB)를 읽는다. `.env.example`은 로컬 예시 값만 담고, `.env`와 보관 폴더는 git에서 제외한다
- [x] 목록 조회 자동 테스트가 있다(`#[sqlx::test]`, 빈 DB → 빈 목록)
- [x] 휴대폰 폭에서도 머리글이 깨지지 않는다
