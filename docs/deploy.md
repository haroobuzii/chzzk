# 서버 자동 배포

main에 올리면 `.github/workflows/deploy.yml`이 배포 서버(`https://chzzk.haroobuzii.com`)를 새 버전으로 바꾼다. 결정 배경은 ADR 0008이다.

## 흐름

1. **검사·테스트**: `ci.yml`을 그대로 부른다(actionlint, shellcheck, 배포 서버용 compose 검사, fmt, 세 빌드 검사, 테스트, README 개수, 비밀 키 검사). 하나라도 실패하면 아래는 돌지 않는다.
2. **이미지**: `ubuntu-24.04-arm` 러너에서 Dockerfile로 ARM64 이미지를 만들어 `ghcr.io/haroobuzii/chzzk:<짧은 해시>`와 `:latest`로 올린다. 빌드 캐시는 GitHub Actions 캐시(gha)에 둔다.
3. **서버 바꾸기**: `scripts/deploy-server.sh`가 SSH로 들어가 아래를 한다.
   - `deploy/compose.yml`을 `/srv/chzzk/compose.yml`로 덮어쓴다.
   - `/srv/chzzk/.env`의 `IMAGE_TAG`를 새 해시로 바꾼다.
   - `docker compose pull app && docker compose up -d`, 사흘 넘은 안 쓰는 이미지 지우기.
4. **확인**: `https://chzzk.haroobuzii.com/healthz`의 `version`이 새 해시가 될 때까지 5초마다 본다. 5분이 넘으면 실패한다.

배포는 한 번에 하나만 돈다(`concurrency: deploy-server`). 도는 중인 배포는 끊지 않고 다음 푸시가 기다린다. Actions 화면의 "Run workflow"로 손으로도 돌릴 수 있다.

## GitHub Secrets

저장소 Settings → Secrets and variables → Actions에 넣는다. 값은 저장소와 로그에 남기지 않는다.

| 이름 | 내용 |
| --- | --- |
| `DEPLOY_SSH_KEY` | 배포 전용 SSH 개인 키 전체(`-----BEGIN OPENSSH PRIVATE KEY-----`부터 끝까지) |
| `DEPLOY_HOST` | 서버 주소(공인 IP). 사용자는 `ubuntu`, 앱 폴더는 `/srv/chzzk`로 정해져 있다 |
| `DEPLOY_KNOWN_HOSTS` | 서버의 호스트 키 줄(known_hosts 형식). 처음 보는 서버에 접속하지 않게 한다 |

GHCR에 올리는 권한은 워크플로의 `GITHUB_TOKEN`(`packages: write`)이라 따로 넣을 것이 없다.

### 배포 전용 SSH 키 만들기

이 맥에서 한다. 서버 관리용 `~/.ssh/chzzk_oci`와 따로 두어, 새면 이 키만 지우면 된다.

```sh
# 1) 암호 없는 배포 전용 키(Actions가 써야 하므로 암호를 걸지 않는다)
ssh-keygen -t ed25519 -N '' -C chzzk-deploy -f ~/.ssh/chzzk_deploy

# 2) 공개 키를 서버의 ubuntu 계정에 더한다(관리용 키로 접속)
ssh -i ~/.ssh/chzzk_oci ubuntu@<서버 IP> 'cat >> ~/.ssh/authorized_keys' < ~/.ssh/chzzk_deploy.pub

# 3) 새 키로 접속되는지 본다
ssh -i ~/.ssh/chzzk_deploy ubuntu@<서버 IP> 'docker compose -f /srv/chzzk/compose.yml ps'

# 4) Secrets 넣기(값이 화면에 나오지 않게 파일·파이프로 넘긴다)
gh secret set DEPLOY_SSH_KEY < ~/.ssh/chzzk_deploy
gh secret set DEPLOY_HOST --body '<서버 IP>'
# 이미 접속해 본 서버의 호스트 키를 그대로 쓴다(ssh-keyscan보다 안전하다).
ssh-keygen -F '<서버 IP>' -f ~/.ssh/known_hosts | grep -v '^#' | gh secret set DEPLOY_KNOWN_HOSTS
```

키를 없애려면 서버 `~/.ssh/authorized_keys`에서 `chzzk-deploy`로 끝나는 줄을 지우고 Secret을 지운다.

## GHCR 패키지 공개(처음 한 번)

GHCR 패키지는 처음 만들어질 때 비공개다. 서버가 로그인 없이(서버에 오래 사는 토큰 없이) 받을 수 있게 패키지를 공개로 바꾼다. 저장소가 공개라 이미지에 숨길 것은 없다(비밀 값은 서버 `.env`에만 있다).

패키지는 워크플로가 처음 이미지를 올릴 때 생긴다. 그래서 첫 실행은 "서버 바꾸기"에서 pull이 거부되어 실패할 수 있다. 그때:

1. GitHub 프로필 → **Packages** 탭(`https://github.com/haroobuzii?tab=packages`) → **chzzk**
2. 오른쪽 아래 **Package settings**
3. 맨 아래 **Danger Zone** → **Change visibility** → **Public** 선택 → 확인 칸에 `chzzk` 입력 → **I understand the consequences, change package visibility**
4. 실패한 워크플로 실행 화면에서 **Re-run failed jobs**

같은 Package settings의 **Manage Actions access**에 저장소 `chzzk`가 **Write**로 있어야 한다. 워크플로가 만든 패키지면 저절로 들어가 있다(맥에서 손으로 먼저 올렸다면 직접 더한다).

## 되돌리기

서버에서 `.env`의 `IMAGE_TAG`를 이전 해시로 바꾸고 `docker compose pull app && docker compose up -d` 한다. 지난 이미지는 GHCR에 해시 태그로 남아 있다.
