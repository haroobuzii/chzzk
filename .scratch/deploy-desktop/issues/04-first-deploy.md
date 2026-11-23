# 04: 첫 배포 ✋

**What to build:** 외부 네트워크에서 `https://chzzk.haroobuzii.com`이 열리고 1~4단계 기능이 동작한다.

- 오라클 VM(`chzzk`, 공인 IP `129.221.239.104`)에 cloud-init 결과(Docker, iptables 80·443·UDP 8189)를 확인한다.
- Cloudflare DNS에 `chzzk` A 레코드 → VM 공인 IP, 프록시 끔(DNS only). 사용자가 대시보드에서 넣거나, 사용자가 만든 API 토큰으로 넣는다.
- 서버에 앱 폴더, 운영용 Compose, 서버 `.env`(무작위 DB 비밀번호, 사용자의 토스 테스트 키 — 이 맥 `.env`에서 화면에 찍지 않고 옮긴다)를 둔다.
- 첫 이미지는 이 맥에서 ARM64로 빌드해 GHCR(공개 패키지)에 올리거나 서버에서 직접 빌드한다.
- `docker compose up -d` 후 Caddy가 인증서를 받는다.
- 사람 확인: 휴대폰 LTE로 HTTPS 자물쇠, 회원가입·로그인, 녹화 업로드·재생, 라이브 송출(같은 와이파이 아님)·시청, 댓글, 테스트 후원.

**Blocked by:** 03, 오라클 VM 생성(완료)

**Status:** done

- [x] `https://chzzk.haroobuzii.com/healthz`가 외부에서 200이다.
- [x] `http://`로 들어오면 `https://`로 옮겨진다.
- [x] 휴대폰 LTE에서 라이브 송출·시청이 된다.
- [x] 서버를 다시 켜도(`sudo reboot`) 데이터가 남고 서비스가 자동으로 돈다.
- [x] 비밀 값은 서버 `.env`에만 있다.

**확인 기록 (2026-11-23)**

- Cloudflare `chzzk` A 레코드 → `129.221.239.104`, DNS only. 1.1.1.1·8.8.8.8·Cloudflare 네임서버 모두 같은 IP로 응답.
- 첫 이미지는 이 맥에서 ARM64로 빌드해 `docker save | ssh docker load`로 옮겼다(GHCR는 05에서).
- Caddy가 Let's Encrypt 인증서를 받았고, `/healthz` 200(`version` 32c4a43), `http://` → 308.
- 서버 `.env`: DB 비밀번호는 서버에서 무작위 생성, 토스 테스트 키는 사용자 허락 후 값을 출력하지 않고 SSH로 옮김.
- 사용자가 휴대폰 LTE에서 가입·로그인, 녹화 업로드·재생, 라이브 송출·시청, 댓글, 테스트 후원을 확인("잘 돼").
- `sudo reboot` 뒤 서비스가 스스로 다시 떴고 `/healthz` 200, 올린 녹화 방송이 그대로 남아 있었다.
