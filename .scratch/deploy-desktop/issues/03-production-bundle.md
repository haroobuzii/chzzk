# 03: 운영용 서버 묶음(이미지·Compose·Caddy)

**What to build:** 배포 서버에 그대로 올릴 수 있는 운영용 묶음을 만들고, 이 맥에서 통째로 띄워 동작을 확인한다.

- **이미지**: 앱을 담는 Dockerfile(cargo-chef 다단계, `dx bundle` 웹+서버). ARM64(linux/arm64)로 빌드된다. 빌드 인자로 커밋 해시를 받아 `/healthz` 버전에 넣는다.
- **운영용 Compose**(개발용과 별도 파일)
  - Caddy: 80·443. 도메인은 환경 변수(기본 `chzzk.haroobuzii.com`). HTTP→HTTPS, 요청 본문 2 GiB, 넉넉한 읽기 시간, 앱으로 프록시.
  - 앱: GHCR 이미지 주소(태그는 환경 변수), `/healthz` 건강 검사, 영상 보관 볼륨.
  - PostgreSQL: 볼륨, 비밀번호는 `.env`에서.
  - MediaMTX: 개발용과 같은 설정, UDP 8189 공개, 추가 ICE 후보 = `PUBLIC_IP`.
  - 모든 서비스 `restart: unless-stopped`.
- **서버용 `.env` 예시**: 자리표시자만(`DB_PASSWORD`, `TOSS_CLIENT_KEY`, `TOSS_SECRET_KEY`, `PUBLIC_IP`, `DOMAIN`, `IMAGE_TAG`). DB 비밀번호를 무작위로 만드는 방법을 적는다.
- **로컬 확인**: Caddy를 `localhost` 내부 인증서로 바꿔 띄우는 방법(환경 변수)으로 이 맥에서 `https://localhost/healthz`가 200이고 녹화 등록·재생이 되는지 본다.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] ARM64 이미지가 빌드된다(맥에서 `docker buildx`).
- [ ] `docker compose config`가 운영용 파일에서 오류 없이 통과한다.
- [ ] 로컬에서 운영용 묶음을 띄우면 `/healthz`가 200이고, 녹화 등록·재생이 된다.
- [ ] 저장소에 실제 비밀 값이 없다(`scripts/check-secrets.sh` 통과).
