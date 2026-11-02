#!/usr/bin/env bash
# Desktop smoke test (macOS/Linux): launch the built app, wait until the UI
# has opened a terminal (a shell child process exists), then terminate the
# app and check that no shell is left behind.
#
# Usage: scripts/smoke-test.sh <path-to-app-binary>
# On headless Linux, run under xvfb-run.
set -euo pipefail

BIN=${1:?usage: smoke-test.sh <app-binary>}
START_TIMEOUT=${START_TIMEOUT:-60}
LOG=$(mktemp)

STARTED=$(date +%s%N 2>/dev/null || date +%s000000000)
"$BIN" >"$LOG" 2>&1 &
APP=$!
echo "app pid $APP"

cleanup() { kill -9 "$APP" 2>/dev/null || true; }
trap cleanup EXIT

# Children of the app that own a terminal. Excludes helpers such as
# WebKitGTK's network/web processes, which have no controlling tty.
shells() {
  ps -A -o pid=,ppid=,tty= | awk -v app="$APP" '$2 == app && $3 != "?" && $3 != "??" { print $1 }'
}

for ((i = 0; i < START_TIMEOUT * 2; i++)); do
  if ! kill -0 "$APP" 2>/dev/null; then
    echo "FAIL: app exited during startup"; cat "$LOG"; exit 1
  fi
  [[ -n "$(shells)" ]] && break
  sleep 0.5
done
if [[ -z "$(shells)" ]]; then
  echo "FAIL: no shell child after ${START_TIMEOUT}s (frontend did not open a terminal)"
  cat "$LOG"; exit 1
fi
NOW=$(date +%s%N 2>/dev/null || date +%s000000000)
# Launch until the first terminal runs a shell (window, UI and backend up).
# macOS date has no %N, so there it is whole seconds.
echo "startup to first terminal: $(( (NOW - STARTED) / 1000000 )) ms (0.5 s polling)"

sleep 2 # let any duplicate open settle
PIDS=()
while IFS= read -r p; do PIDS+=("$p"); done < <(shells) # bash 3.2-compatible
echo "shell children: ${#PIDS[@]}"
ps -o pid,ppid,tty,command -p "$(IFS=,; echo "${PIDS[*]}")"
if [[ ${#PIDS[@]} -ne 1 ]]; then
  echo "FAIL: expected exactly one shell, found ${#PIDS[@]}"; exit 1
fi

# SIGTERM: the app may not run its own cleanup, so this checks the OS-level
# guarantee (PTY master closes -> kernel hangs up the shell).
kill -TERM "$APP"
for ((i = 0; i < 20; i++)); do
  alive=0
  for p in "${PIDS[@]}"; do kill -0 "$p" 2>/dev/null && alive=1; done
  [[ $alive -eq 0 ]] && break
  sleep 0.5
done
for p in "${PIDS[@]}"; do
  if kill -0 "$p" 2>/dev/null; then echo "FAIL: shell $p survived app termination"; exit 1; fi
done
echo "PASS: terminal opened and shell cleaned up"
