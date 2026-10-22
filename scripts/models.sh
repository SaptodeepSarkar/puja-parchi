#!/usr/bin/env bash
# Start (or verify) the local vision and speech models.
#
# scripts/dev.sh runs this before the server, so "start the server" also means
# "the models are loaded". Running it twice is harmless.
#
#   ./scripts/models.sh          start anything that is not already up
#   ./scripts/models.sh status   report only
#   ./scripts/models.sh stop     stop the ones this script started
set -euo pipefail
cd "$(dirname "$0")/.."

OLLAMA_BIN=${OLLAMA_BIN:-$HOME/.local/ollama/bin/ollama}
OLLAMA_MODELS=${OLLAMA_MODELS:-$HOME/.local/ollama/models}
VISION_MODEL=${PUJA_VISION_MODEL:-gemma3:4b}
VISION_URL=${PUJA_VISION_BASE_URL:-http://127.0.0.1:11434/v1}
WHISPER_BIN=${WHISPER_BIN:-}
if [ -z "$WHISPER_BIN" ]; then
  # Prefer the Vulkan build when it is there: 0.2 s per 6 s clip against 3.1 s
  # for the CPU build, 0.2 GB of RAM instead of 0.7, and the 12 cores stay free
  # while a volunteer waits. Same words either way.
  WHISPER_BIN=$HOME/.local/src/whisper.cpp/build-vulkan/bin/whisper-server
  [ -x "$WHISPER_BIN" ] || WHISPER_BIN=$HOME/.local/src/whisper.cpp/build/bin/whisper-server
fi
# ggml-small.bin on purpose: the same words as large-v3-turbo in either build,
# but 0.78 GB of VRAM instead of 1.8 GB, which leaves the vision model room to
# live on the same 6 GB card. Speed is no longer the reason.
WHISPER_MODEL=${WHISPER_MODEL:-$HOME/.local/src/whisper.cpp/models/ggml-small.bin}
WHISPER_THREADS=${WHISPER_THREADS:-8}
KEEP_ALIVE=${PUJA_KEEP_ALIVE:-15m}
STT_URL=${PUJA_STT_BASE_URL:-http://127.0.0.1:8081/v1}
LOG_DIR=${PUJA_LOG_DIR:-data/logs}
RUN_DIR=${PUJA_RUN_DIR:-data/run}

# Any HTTP answer means the server is up: ollama returns 200 for /v1/models,
# whisper.cpp returns 404 because it has no such route. Both are "reachable".
reachable() { curl -s -o /dev/null -m 2 "$1/models" 2>/dev/null; }

is_local() {
  case "$1" in
    http://127.0.0.1*|http://localhost*|http://192.168.*|http://10.*|http://172.*) return 0 ;;
    *) return 1 ;;
  esac
}

wait_for() {
  local url=$1 tries=${2:-30}
  for _ in $(seq "$tries"); do
    reachable "$url" && return 0
    sleep 1
  done
  return 1
}

start_vision() {
  if ! is_local "$VISION_URL"; then
    echo "vision: $VISION_URL is remote, nothing to start"
    return 0
  fi
  if reachable "$VISION_URL"; then
    echo "vision: already up"
  else
    if [ ! -x "$OLLAMA_BIN" ]; then
      echo "vision: ollama not found at $OLLAMA_BIN (set OLLAMA_BIN)" >&2
      return 1
    fi
    echo "vision: starting ollama (model $VISION_MODEL)"
    mkdir -p "$LOG_DIR" "$RUN_DIR" "$OLLAMA_MODELS"
    OLLAMA_MODELS="$OLLAMA_MODELS" nohup "$OLLAMA_BIN" serve >>"$LOG_DIR/ollama.log" 2>&1 &
    echo $! >"$RUN_DIR/ollama.pid"
    if ! wait_for "$VISION_URL" 30; then
      echo "vision: ollama did not answer on $VISION_URL - see $LOG_DIR/ollama.log" >&2
      return 1
    fi
  fi
  # Keep the model hot through a busy spell, then let it go: 2.9 GB in VRAM and
  # ~0.9 GB of this laptop's RAM is real money when the desk goes quiet.
  local origin
  origin=$(printf '%s' "$VISION_URL" | sed 's|/v1/*$||')
  if curl -s -m 180 "$origin/api/generate" \
    -d "{\"model\":\"$VISION_MODEL\",\"keep_alive\":\"${KEEP_ALIVE:-15m}\"}" >/dev/null 2>&1; then
    echo "vision: $VISION_MODEL loaded, released after ${KEEP_ALIVE:-15m} idle"
  else
    echo "vision: $VISION_MODEL will load on the first capture"
  fi
}

start_speech() {
  if ! is_local "$STT_URL"; then
    echo "speech: $STT_URL is remote, nothing to start"
    return 0
  fi
  if reachable "$STT_URL"; then
    echo "speech: already up"
    return 0
  fi
  if [ ! -x "$WHISPER_BIN" ]; then
    echo "speech: whisper-server not found at $WHISPER_BIN (set WHISPER_BIN)" >&2
    return 1
  fi
  if [ ! -f "$WHISPER_MODEL" ]; then
    echo "speech: model not found at $WHISPER_MODEL (set WHISPER_MODEL)" >&2
    return 1
  fi
  local port
  port=$(printf '%s' "$STT_URL" | sed -n 's|.*:\([0-9]\+\).*|\1|p')
  port=${port:-8081}
  echo "speech: starting whisper-server on $port ($(basename "$WHISPER_MODEL"))"
  mkdir -p "$LOG_DIR" "$RUN_DIR"
  nohup "$WHISPER_BIN" -m "$WHISPER_MODEL" --host 127.0.0.1 --port "$port" \
    --convert --inference-path /v1/audio/transcriptions -t "$WHISPER_THREADS" \
    >>"$LOG_DIR/whisper.log" 2>&1 &
  echo $! >"$RUN_DIR/whisper.pid"
  if ! wait_for "$STT_URL" 60; then
    echo "speech: whisper did not answer on $STT_URL - see $LOG_DIR/whisper.log" >&2
    return 1
  fi
}

stop_one() {
  local name=$1 url=$2 file="$RUN_DIR/$1.pid"
  if [ -f "$file" ] && kill -0 "$(cat "$file")" 2>/dev/null; then
    kill "$(cat "$file")"
    rm -f "$file"
    echo "$name: stopped"
  elif reachable "$url"; then
    echo "$name: running, but not started by this script - left alone"
  else
    echo "$name: not running"
  fi
}

status() {
  if reachable "$VISION_URL"; then
    echo "vision: up   $VISION_URL  ($VISION_MODEL)"
  else
    echo "vision: DOWN $VISION_URL"
  fi
  if reachable "$STT_URL"; then
    echo "speech: up   $STT_URL  ($(basename "$WHISPER_MODEL"))"
  else
    echo "speech: DOWN $STT_URL"
  fi
}

case "${1:-up}" in
  up)
    # Both models together cost about 1.7 GB of RAM (vision mostly in VRAM).
    # Under that, the laptop starts swapping, which is what "laggy" means here.
    avail=$(awk '/MemAvailable/{print int($2/1024)}' /proc/meminfo 2>/dev/null || true)
    avail=${avail:-99999}
    if [ "$avail" -lt 1500 ]; then
      echo "  ! only ${avail} MB RAM free; the models want ~1.7 GB together." >&2
      echo "    Close the browser or the editor tabs you are not using, or expect swapping." >&2
    fi
    start_vision
    start_speech
    status
    ;;
  status) status ;;
  stop)
    stop_one ollama "$VISION_URL"
    stop_one whisper "$STT_URL"
    ;;
  *)
    echo "usage: $0 [up|status|stop]" >&2
    exit 2
    ;;
esac
