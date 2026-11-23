#!/usr/bin/env bash
# 데스크톱 앱(녹화방송)을 지금 컴퓨터의 OS·아키텍처용 설치 파일 하나로 묶는다.
# GitHub Actions의 앱 배포(.github/workflows/app-release.yml)와 내 컴퓨터에서 똑같이 쓴다.
#
#   scripts/bundle-app.sh dmg chzzk-macos-arm64.dmg     # 맥
#   scripts/bundle-app.sh msi chzzk-windows-x64.msi     # Windows(Git Bash)
#
# 결과는 dist/<파일 이름>에 놓인다. 서버 주소는 CHZZK_APP_SERVER_URL(없으면 배포 서버 주소)로 빌드한다.
#
# 맥 서명·공증: 아래 값이 모두 있으면 Developer ID로 서명하고 공증한다. 하나라도 없으면
# ad-hoc 서명만 한 앱을 만들고 그 사실을 알린다. 값의 뜻은 docs/desktop-release.md에 있다.
#   APPLE_CERTIFICATE_P12_BASE64, APPLE_CERTIFICATE_PASSWORD, APPLE_TEAM_ID,
#   APPLE_API_KEY_ID, APPLE_API_ISSUER_ID, APPLE_API_KEY_P8_BASE64
set -euo pipefail

if [ "$#" -ne 2 ] || { [ "$1" != dmg ] && [ "$1" != msi ]; }; then
  echo "사용법: $0 <dmg|msi> <올릴 파일 이름>" >&2
  exit 2
fi
kind=$1
asset=$2

cd "$(dirname "$0")/.."
bundle_dir=dist/bundle
rm -rf "$bundle_dir" "dist/$asset"
mkdir -p "$bundle_dir"

# GitHub Actions 로그에는 알림 칸으로, 내 컴퓨터에서는 그냥 글로 보인다.
notice() {
  if [ -n "${GITHUB_ACTIONS:-}" ]; then
    echo "::notice title=맥 앱 서명::$1"
  else
    echo "알림: $1"
  fi
}

signed=false
if [ "$kind" = dmg ]; then
  missing=()
  for name in APPLE_CERTIFICATE_P12_BASE64 APPLE_CERTIFICATE_PASSWORD APPLE_TEAM_ID \
    APPLE_API_KEY_ID APPLE_API_ISSUER_ID APPLE_API_KEY_P8_BASE64; do
    if [ -z "${!name:-}" ]; then
      missing+=("$name")
    fi
  done

  # dx는 APPLE_CERTIFICATE가 있으면 서명하고, APPLE_API_KEY(또는 APPLE_ID)가 있으면 공증한다.
  # 일부만 있을 때 반쯤 서명된 앱이 나오지 않도록, 모두 있을 때만 넘기고 아니면 지운다.
  unset APPLE_CERTIFICATE APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH APPLE_ID APPLE_PASSWORD
  if [ "${#missing[@]}" -eq 0 ]; then
    signed=true
    key_dir=$(mktemp -d)
    trap 'rm -rf "$key_dir"' EXIT
    printf '%s' "$APPLE_API_KEY_P8_BASE64" | base64 --decode >"$key_dir/AuthKey.p8"
    export APPLE_CERTIFICATE="$APPLE_CERTIFICATE_P12_BASE64"
    export APPLE_CERTIFICATE_PASSWORD
    export APPLE_API_KEY="$APPLE_API_KEY_ID"
    export APPLE_API_ISSUER="$APPLE_API_ISSUER_ID"
    export APPLE_API_KEY_PATH="$key_dir/AuthKey.p8"
    echo "맥 앱을 Developer ID로 서명하고 공증한다."
  else
    notice "서명 비밀 값이 없어(${missing[*]}) 서명·공증 없이(ad-hoc) 만든다. 처음 열 때 '확인되지 않은 개발자' 경고가 난다."
  fi
fi

# app 기능만 켜서 앱만 빌드한다(서버는 빌드하지 않는다. Cargo.toml의 app-fullstack 참고).
dx bundle --desktop --release --no-default-features --features app \
  --package-types "$kind" --out-dir "$bundle_dir"

found=()
while IFS= read -r file; do
  found+=("$file")
done < <(find "$bundle_dir" -maxdepth 1 -name "*.$kind")
if [ "${#found[@]}" -ne 1 ]; then
  echo "$bundle_dir에서 .$kind 파일을 하나만 찾아야 하는데 ${#found[@]}개를 찾았다." >&2
  ls -la "$bundle_dir" >&2
  exit 1
fi
cp "${found[0]}" "dist/$asset"

if [ "$kind" = dmg ]; then
  # .dmg를 열어 안의 앱 이름과 서명을 확인한다.
  mount_dir=$(mktemp -d)
  hdiutil attach -nobrowse -readonly -mountpoint "$mount_dir" "dist/$asset" >/dev/null
  detach() { hdiutil detach -quiet "$mount_dir" || true; }
  app="$mount_dir/녹화방송.app"
  if [ ! -d "$app" ]; then
    echo ".dmg 안에 녹화방송.app이 없다(Dioxus.toml [bundle.macos] bundle_name)." >&2
    ls -la "$mount_dir" >&2
    detach
    exit 1
  fi
  name=$(/usr/libexec/PlistBuddy -c "Print :CFBundleDisplayName" "$app/Contents/Info.plist")
  if [ "$name" != "녹화방송" ]; then
    echo "앱 이름이 '녹화방송'이 아니라 '$name'이다." >&2
    detach
    exit 1
  fi
  if ! codesign --verify --deep --strict "$app"; then
    detach
    exit 1
  fi
  if [ "$signed" = true ]; then
    # 우리 팀 인증서로 서명됐고, 공증 표가 붙었고, Gatekeeper가 받아들이는지 본다.
    if ! codesign -dv "$app" 2>&1 | grep -q "^TeamIdentifier=$APPLE_TEAM_ID\$" \
      || ! spctl --assess --type execute -v "$app"; then
      echo "앱이 우리 팀 Developer ID로 서명·공증되지 않았다." >&2
      detach
      exit 1
    fi
  fi
  detach
  if [ "$signed" = true ]; then
    xcrun stapler validate "dist/$asset"
    spctl --assess --type open --context context:primary-signature -v "dist/$asset"
  fi
fi

echo "만들었다: dist/$asset ($(du -h "dist/$asset" | cut -f1))"
