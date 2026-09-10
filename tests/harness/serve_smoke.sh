#!/usr/bin/env bash
# Smoke test for `rekyll serve`: serves the docs site on a spare port and
# checks routing, the reload script, and that path traversal is refused.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
export REKYLL_WORK="${REKYLL_WORK:-/tmp/rekyll-diff}"
mkdir -p "$REKYLL_WORK"
cargo build --quiet 2>/dev/null || { echo "build failed"; exit 1; }
BIN="${REKYLL_BIN:-$ROOT/target/debug/rekyll}"
PORT=${PORT:-4321}
DEST="$REKYLL_WORK/serve-site"
fail=0
check() { if "$@"; then :; else echo "FAIL: $*"; fail=1; fi; }

start() { setsid "$BIN" serve -s docs -d "$DEST" -P "$PORT" --no-watch "$@" >"$REKYLL_WORK/serve.log" 2>&1 & echo $!; }
wait_up() { for _ in $(seq 50); do curl -fs -o /dev/null "http://127.0.0.1:$PORT/" && return; sleep 0.1; done; }
get() { curl -s "$@"; }

pid=$(start); wait_up
check grep -q "<title>" <(get "http://127.0.0.1:$PORT/")
check grep -q "<title>" <(get "http://127.0.0.1:$PORT/features")           # a directory serves its index
check grep -q "<title>" <(get "http://127.0.0.1:$PORT/features/")
check grep -q "{" <(get "http://127.0.0.1:$PORT/assets/css/main.css")
check test "$(get -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/nope")" = 404
check test "$(get -o /dev/null -w '%{http_code}' --path-as-is "http://127.0.0.1:$PORT/../../Cargo.toml")" = 404
check test "$(get -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/%2e%2e/%2e%2e/Cargo.toml")" = 404
check grep -q "__rekyll_live" <(get "http://127.0.0.1:$PORT/")             # injected into HTML
check test -z "$(get "http://127.0.0.1:$PORT/assets/css/main.css" | grep __rekyll_live)"  # not into CSS
check test -z "$(grep __rekyll_live "$DEST/index.html")"                    # never on disk
check test "$(get "http://127.0.0.1:$PORT/__rekyll_live")" = 0
check test "$(get -I -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/")" = 200   # HEAD
kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null

pid=$(start --no-livereload --skip-initial-build); wait_up
check test -z "$(get "http://127.0.0.1:$PORT/" | grep __rekyll_live)"
kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null

[ $fail = 0 ] && echo "=== SERVE OK ===" || { echo "--- server log:"; cat "$REKYLL_WORK/serve.log"; }
exit $fail
