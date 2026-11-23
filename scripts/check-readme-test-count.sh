#!/usr/bin/env bash
# README에 적힌 최신 테스트 개수가 실제로 통과한 개수와 같은지 확인한다.
#
# 쓰는 법: cargo test 출력을 파일로 받은 뒤 그 파일을 넘긴다.
#   cargo test --no-default-features --features server 2>&1 | tee test-output.txt
#   scripts/check-readme-test-count.sh test-output.txt
#
# README의 결과 절은 최신 단계가 위에 오므로, "`cargo test`: N개" 줄 중 첫 번째를 최신 값으로 본다.
set -euo pipefail

output="${1:?cargo test 출력 파일을 넘겨 주세요}"
readme="${2:-README.md}"

# "test result: ok. 148 passed; ..." 줄들의 통과 개수를 모두 더한다(테스트 묶음이 여럿일 수 있다).
actual=$(grep -Eo 'test result: [a-zA-Z]+\. [0-9]+ passed' "$output" | grep -Eo '[0-9]+ passed' | grep -Eo '[0-9]+' | awk '{s+=$1} END {print s+0}')
# README에 적힌 글자 그대로의 백틱을 찾는다(셸 확장이 아니다).
# shellcheck disable=SC2016
written=$(grep -Eo '`cargo test`: [0-9]+개' "$readme" | head -1 | grep -Eo '[0-9]+' || true)

if [ -z "$written" ]; then
  echo "README에서 '\`cargo test\`: N개' 줄을 찾지 못했습니다." >&2
  exit 1
fi

if [ "$actual" != "$written" ]; then
  echo "README의 테스트 개수($written개)가 실제 통과 개수($actual개)와 다릅니다. README의 최신 결과를 고쳐 주세요." >&2
  exit 1
fi

echo "README 테스트 개수 확인: ${actual}개로 같다."
