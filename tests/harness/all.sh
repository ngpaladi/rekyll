#!/usr/bin/env bash
# Runs every differential: site builds, Markdown corpus, filter corpus, YAML scalars, strftime.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
cargo build --quiet 2>/dev/null || { echo "build failed"; exit 1; }
fail=0
"$ROOT/tests/harness/diff.sh" || fail=1
"$ROOT/tests/harness/md_diff.sh" | tail -20 || fail=1
"$ROOT/tests/harness/filters_diff.sh" | tail -20 || fail=1
"$ROOT/tests/harness/units_diff.sh" | tail -20 || fail=1
exit $fail
