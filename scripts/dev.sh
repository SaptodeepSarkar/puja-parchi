#!/usr/bin/env bash
# Start the puja desk: build the phone app if needed, then run the server.
set -euo pipefail
cd "$(dirname "$0")/.."

if [ ! -f app/dist/index.html ]; then
  echo "building the phone app…"
  (cd app && npm install --no-audit --no-fund && npm run build)
fi

if [ ! -f data/ledger/"Durga Puja 2026.xlsx" ] && [ -z "${PUJA_LEDGER_PATH:-}" ]; then
  echo
  echo "  No ledger at data/ledger/Durga Puja 2026.xlsx"
  echo "  Copy your workbook there, or set PUJA_LEDGER_PATH in .env"
  echo
fi

# The models are part of "start the server": a capture with no model server
# answering is a wasted stub. Set PUJA_MODELS=off to skip (cloud providers).
if [ "${PUJA_MODELS:-on}" != "off" ]; then
  echo "starting the local models…"
  ./scripts/models.sh up || echo "  (a model is not answering - the server and app will say so)"
fi

exec cargo run --manifest-path server/Cargo.toml "$@"
