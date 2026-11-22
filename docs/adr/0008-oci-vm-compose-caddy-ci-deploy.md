# 배포 서버는 OCI A1 VM 한 대에 Docker Compose로 올리고, main 푸시마다 GitHub Actions가 배포한다

배포 서버는 홈 리전 싱가포르(`ap-singapore-1`)의 Always Free `VM.Standard.A1.Flex` 2 OCPU·12 GB(ARM64, Ubuntu) 한 대다. 지금 A1 무료 한도가 이만큼이라 "무료로 쓸 수 있는 가장 좋은 서버"다. 계정은 A1 재고를 잡기 쉬운 Pay As You Go로 쓰되, 무료 모양만 만들고 예산 알림($1)을 건다.

서버 안에서는 지금의 `docker-compose.yml` 구성(PostgreSQL, MediaMTX)에 앱과 Caddy를 더해 Docker Compose로 띄운다. Caddy가 `chzzk.haroobuzii.com`의 HTTPS 인증서(Let's Encrypt)를 자동으로 받고 앱으로 넘긴다. DNS는 Cloudflare에서 관리하되 "DNS only"(프록시 끔)로 둔다. 프록시를 켜면 업로드가 100 MB에서 막히고 WebRTC(UDP)가 지나가지 못한다. 서버에 공인 IP가 있으니 라이브 송출은 같은 와이파이가 아니어도 된다.

main에 푸시하면 GitHub Actions가 테스트하고, ARM64 러너에서 앱 이미지를 만들어 GHCR에 올리고, SSH로 서버에 들어가 새 이미지로 바꾼다. 서버 접속 키는 GitHub Secrets에, 토스 키·DB 비밀번호 같은 앱 비밀 값은 서버의 `.env`에만 둔다. 서버를 만드는 과정은 OCI CLI 스크립트로 저장소에 남겨 재현할 수 있게 한다.

이 구성은 ⑤단계(로컬 오토스케일)에서 같은 이미지를 여러 개 띄울 수 있게 앱의 상태(DB, 영상 파일, 세션)를 앱 밖에 둔다는 방향과 맞춘다.

## Considered Options

- **Railway·Fly.io 같은 PaaS**: 배포는 쉽지만 과제가 OCI를 요구하고, Railway는 UDP를 받지 못한다(ADR 0004).
- **Kubernetes(k3s)**: ⑤단계에서 쓸 수 있지만, 서버 한 대 배포에는 Compose가 단순하다.
- **서버에서 직접 빌드**: 2 OCPU로 Rust 빌드가 오래 걸리고 서버 부하가 커진다. 빌드는 GitHub의 ARM64 러너가 한다.
