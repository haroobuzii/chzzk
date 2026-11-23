#!/usr/bin/env bash
# 저장소에 비밀 키가 들어가지 않았는지 확인한다. 키 값은 절대 출력하지 않고, 이 컴퓨터 밖으로 보내지 않는다.
#
# 쓰는 법:
#   scripts/check-secrets.sh                 # 내 컴퓨터: .env의 실제 키 + 키 모양 검사, 모든 커밋 기록까지
#   scripts/check-secrets.sh --patterns-only # GitHub 자동 검사: .env 없이 키 모양만 검사
#
# 찾으면 1로 끝난다. 결과에는 변수 이름, 파일 이름, 커밋 번호만 나온다.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
found=0

# 1) 키 모양: 토스 키(test_/live_ + ck/sk/gck/gsk) 중 테스트용 가짜(_fake)와 자리표시자가 아닌 것.
#    지금 작업 폴더의 추적 파일과 모든 커밋을 본다. 줄 내용은 보여 주지 않고 위치만 보여 준다.
pattern='(test|live)_g?(ck|sk)_[A-Za-z0-9_]{12,}'
allow='_fake|여기에|생략'
hits=$(git grep -nIE "$pattern" -- . ':!*.lock' 2>/dev/null | grep -vE "$allow" | cut -d: -f1-2 || true)
if [ -n "$hits" ]; then
  echo "키 모양이 작업 폴더의 추적 파일에 있다:" >&2
  printf '%s\n' "$hits" | sed 's/^/  /' >&2
  found=1
fi
for commit in $(git rev-list --all); do
  in_commit=$(git grep -lIE "$pattern" "$commit" -- . ':!*.lock' 2>/dev/null | while IFS= read -r path; do
    git show "$path" 2>/dev/null | grep -qvE "$allow" && git show "$path" | grep -E "$pattern" | grep -qvE "$allow" && echo "$path"
  done || true)
  if [ -n "$in_commit" ]; then
    echo "키 모양이 커밋 기록에 있다:" >&2
    printf '%s\n' "$in_commit" | sed 's/^/  /' >&2
    found=1
  fi
done

# 2) .env의 실제 값: 이름에 KEY·SECRET·TOKEN이 들어간 값이 저장소나 커밋 기록에 그대로 있는지.
if [ "${1:-}" != "--patterns-only" ]; then
  if git ls-files --error-unmatch .env >/dev/null 2>&1; then
    echo ".env가 git에 추적되고 있다. .gitignore를 확인하고 git rm --cached .env 하세요." >&2
    found=1
  fi
  if [ -f .env ]; then
    while IFS='=' read -r name value; do
      case "$name" in ''|\#*) continue ;; esac
      case "$name" in *KEY*|*SECRET*|*TOKEN*) ;; *) continue ;; esac
      value="${value%$'\r'}"
      value="${value#"${value%%[![:space:]]*}"}"
      value="${value%"${value##*[![:space:]]}"}"
      [ ${#value} -lt 12 ] && continue
      # 커밋 해시 목록을 일부러 낱개 인자로 나눠 넘긴다(해시에는 공백이 없다).
      # shellcheck disable=SC2046
      if git grep -qF -e "$value" $(git rev-list --all) -- . 2>/dev/null \
        || grep -rqF --exclude-dir=target --exclude-dir=.git --exclude-dir=.claude --exclude=.env -e "$value" . 2>/dev/null; then
        echo "$name의 실제 값이 저장소, 커밋 기록 또는 작업 폴더의 다른 파일에 있다." >&2
        found=1
      fi
    done < .env
  fi
fi

if [ "$found" -ne 0 ]; then
  echo "비밀 키 검사: 실패. 위 위치에서 키를 지우고, 이미 올렸다면 키를 재발급하세요." >&2
  exit 1
fi
echo "비밀 키 검사: 통과 (키 값은 출력하지 않았다)."
