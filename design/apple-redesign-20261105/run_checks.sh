#!/usr/bin/env bash
# Build the desktop frontend, serve it with `vite preview`, run every
# check_app*.py against it in headless Edge, then stop the server.
#
#   design/apple-redesign-20261105/run_checks.sh            # build + all checks
#   design/apple-redesign-20261105/run_checks.sh --no-build # reuse apps/desktop/dist
#   design/apple-redesign-20261105/run_checks.sh dc sync    # only check_app_dc.py, check_app_sync.py
#
# check_app.py flags the °C axis ticks on the performance page as "fake" readings:
# a FAIL there alone is the known false positive (see HANDOFF). Exit code = number of failed scripts.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
APP=$(cd "$HERE/../../apps/desktop" && pwd)
PORT=${PORT:-4173}
BASE="http://127.0.0.1:$PORT/"

build=1; names=()
for a in "$@"; do
  case "$a" in --no-build) build=0 ;; *) names+=("$a") ;; esac
done
[ ${#names[@]} -eq 0 ] && names=(dc sync shell app)

listener_pid() { netstat -ano | grep "127.0.0.1:$PORT .*LISTENING" | awk '{print $5}' | head -1; }
if [ -n "$(listener_pid)" ]; then echo "端口 $PORT 已被占用（PID $(listener_pid)），请先结束它或设置 PORT=" >&2; exit 90; fi

if [ $build -eq 1 ]; then
  (cd "$APP" && npm run check 2>&1 | tail -1 && npm run build >/dev/null 2>&1) || { echo "前端检查/构建失败" >&2; exit 91; }
  echo "check/build 通过"
fi

(cd "$APP" && npx vite preview --host 127.0.0.1 --port "$PORT" --strictPort >/dev/null 2>&1 &)
stop() { pid=$(listener_pid); [ -n "$pid" ] && taskkill //PID "$pid" //F >/dev/null 2>&1; }
trap stop EXIT
for _ in $(seq 1 30); do curl -s -o /dev/null "$BASE" && break; sleep 1; done
curl -s -o /dev/null "$BASE" || { echo "vite preview 未启动" >&2; exit 92; }

failed=0
for n in "${names[@]}"; do
  script="$HERE/check_app${n:+_$n}.py"; [ "$n" = app ] && script="$HERE/check_app.py"
  out=$(PYTHONIOENCODING=utf-8 python "$script" "$BASE" 2>&1); code=$?
  if [ $code -eq 0 ]; then echo "PASS $(basename "$script")"; else
    failed=$((failed+1)); echo "FAIL $(basename "$script") (exit $code)"; printf '%s\n' "$out" | grep -E "FAIL|Error|Traceback" | head -8 | sed 's/^/    /'
  fi
done
echo "截图目录：$HERE/shots-app/"
exit $failed
