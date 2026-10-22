#!/usr/bin/env bash
# Everything must be green before a receipt is trusted to this thing.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== server tests =="
cargo test --manifest-path server/Cargo.toml

echo "== app typecheck / unit / build =="
(cd app && npm run typecheck && npm test && npm run build)

echo "== browser end-to-end (fake vision, copy of the ledger) =="
(cd app && node tests/e2e.mjs)

echo
echo "all green"
