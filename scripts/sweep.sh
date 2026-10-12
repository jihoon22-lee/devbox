#!/usr/bin/env bash
# 공유 target/cache는 자동 삭제하지 않는다. 소유권 확인 전 정리는 유보한다.
set -euo pipefail
if [ "$#" -ne 0 ]; then
  echo "sweep: 경로/삭제 옵션을 받지 않습니다. 공유 cache와 사용자 자료를 보존합니다." >&2
  exit 2
fi
echo "sweep: 정리 유보. 완료한 소유 산출물의 경로·소유권·비활성 상태를 확인한 뒤 별도로 정리하세요."
