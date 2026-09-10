#!/usr/bin/env bash
# Differential for Time#strftime: a Ruby script and a Rust example print the
# same tab-separated lines.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
export REKYLL_WORK="${REKYLL_WORK:-/tmp/rekyll-diff}"
mkdir -p "$REKYLL_WORK"
cargo build --quiet --example strftime_dump 2>/dev/null
ok=0
ruby tests/harness/strftime_ref.rb tests/harness/strftime_fmts.txt > "$REKYLL_WORK/strftime-ruby.txt"
cargo run --quiet --example strftime_dump -- tests/harness/strftime_fmts.txt > "$REKYLL_WORK/strftime-rust.txt" 2>/dev/null
diff -u "$REKYLL_WORK/strftime-ruby.txt" "$REKYLL_WORK/strftime-rust.txt" && echo "=== STRFTIME IDENTICAL ===" || ok=1
exit $ok
