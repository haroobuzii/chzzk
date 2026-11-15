# 병렬 작업 수칙

`/implement-spec`처럼 도우미 여러 명이 동시에 각자의 작업 폴더(worktree)에서 일할 때 지킨다. 이 컴퓨터의 자원은 모두가 함께 쓴다.

- **작업 폴더의 시작점**: 작업 폴더는 GitHub에 올라간 `main`에서 시작할 수 있다. 시작하자마자 `git log --oneline -1`로 통합 브랜치의 끝인지 확인하고, 아니면 `git reset --hard <통합 브랜치>`로 맞춘다.
- **Docker**: PostgreSQL(5433)과 MediaMTX는 원래 폴더(`/Users/mac/projects/chzzk`)에서 띄운 것 하나를 함께 쓴다. 작업 폴더에서는 `docker compose ps`로 떠 있는지만 확인한다. 내려가 있으면 원래 폴더에서 `docker compose up -d`를 실행한다.
- **DB**: 테스트는 `#[sqlx::test]`가 임시 DB를 만드므로 공용 DB(`chzzk`)를 그대로 가리켜도 된다. 앱을 띄워 확인할 때는 `chzzk_t<번호>` 같은 내 DB를 만들어 쓰고, 끝나면 지운다. 아직 합쳐지지 않은 마이그레이션이 공용 DB에 들어가면 다른 폴더의 서버가 켜지지 않는다.
- **포트**: 8080은 이 컴퓨터의 다른 프로그램이 쓴다. 앱은 `dx serve --port 80<티켓 번호>`처럼 겹치지 않는 포트로 띄우고, 확인이 끝나면 끈다.
- **브라우저**: 앱 안 브라우저는 함께 쓴다. 확인할 때는 `tabs_create`로 내 탭을 새로 열어 그 탭만 쓰고, 끝나면 닫는다.
- **`.env`**: 작업 폴더에는 `.env`가 없다. 필요하면 `.env.example`을 복사하고 `LAN_IP`(`ipconfig getifaddr en0`)를 채운다. 끝나면 지운다.
