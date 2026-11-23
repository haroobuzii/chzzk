#!/usr/bin/env bash
# 배포 서버의 앱을 GHCR 이미지 태그(짧은 커밋 해시)로 바꾸고, /healthz의 version이 그 태그가 될 때까지 기다린다.
# .github/workflows/deploy.yml이 부른다. 설명과 Secrets는 docs/deploy.md에 있다.
#
#   DEPLOY_HOST=<서버 주소> scripts/deploy-server.sh <짧은 커밋 해시>
#
# SSH 키와 known_hosts는 미리 ~/.ssh에 있어야 한다(워크플로는 Secrets에서 만든다).
# 서버에서 하는 일: compose.yml 바꾸기 → .env의 IMAGE_TAG 바꾸기 → docker compose pull app && up -d.
set -euo pipefail

tag="${1:?사용법: scripts/deploy-server.sh <짧은 커밋 해시>}"
host="${DEPLOY_HOST:?DEPLOY_HOST(서버 주소)가 필요하다}"
user="${DEPLOY_USER:-ubuntu}"
app_dir="${DEPLOY_DIR:-/srv/chzzk}"
health_url="${HEALTH_URL:-https://chzzk.haroobuzii.com/healthz}"
timeout_s="${HEALTH_TIMEOUT:-300}"

# 태그는 서버 명령과 sed 안에 그대로 들어가므로 커밋 해시 모양만 받는다.
if ! [[ $tag =~ ^[0-9a-f]{7,40}$ ]]; then
  echo "태그는 커밋 해시(소문자 16진수 7~40자)여야 한다: $tag" >&2
  exit 1
fi

cd "$(dirname "$0")/.."
ssh_opts=(-o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=15)

echo "compose.yml 올리는 중…"
scp -q "${ssh_opts[@]}" deploy/compose.yml "$user@$host:$app_dir/compose.yml.new"

echo "서버에서 이미지 $tag로 바꾸는 중…"
# 아래 내용은 서버의 bash가 읽는다. $1=앱 폴더, $2=태그.
ssh "${ssh_opts[@]}" "$user@$host" bash -s -- "$app_dir" "$tag" <<'REMOTE'
set -euo pipefail
cd "$1"
mv compose.yml.new compose.yml
if grep -q '^IMAGE_TAG=' .env; then
  sed -i "s/^IMAGE_TAG=.*/IMAGE_TAG=$2/" .env
else
  echo "IMAGE_TAG=$2" >> .env
fi
docker compose pull app
docker compose up -d
# 사흘 넘게 쓰지 않은 이미지를 지워 디스크가 차지 않게 한다. 지난 버전은 GHCR에 남아 있다.
docker image prune -af --filter until=72h >/dev/null
docker compose ps
REMOTE

echo "$health_url 의 version이 $tag가 될 때까지 기다린다(최대 ${timeout_s}초)…"
deadline=$((SECONDS + timeout_s))
while :; do
  version=$(curl -fsS --max-time 10 "$health_url" 2>/dev/null | jq -r '.version // empty' 2>/dev/null || true)
  if [ "$version" = "$tag" ]; then
    echo "배포 완료: version $version"
    exit 0
  fi
  if [ "$SECONDS" -ge "$deadline" ]; then
    echo "제한 시간 안에 새 버전이 뜨지 않았다(마지막 version: ${version:-응답 없음})." >&2
    exit 1
  fi
  sleep 5
done
