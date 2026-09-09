#!/usr/bin/env bash
# Differentials for two small units: YAML scalar resolution (Psych) and
# Time#strftime. Each pair of dumpers prints the same tab-separated lines.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
export REKYLL_WORK="${REKYLL_WORK:-/tmp/rekyll-diff}"
mkdir -p "$REKYLL_WORK"
cargo build --quiet --example scalars --example strftime_dump 2>/dev/null
ok=0
# TZ=UTC: Psych shows an unzoned time in the process zone; the instant is what matters.
TZ=UTC ruby tests/harness/psych_ref.rb tests/harness/scalars.txt | sed "s/ERROR(.*/ERROR/" > "$REKYLL_WORK/psych-ruby.txt"
cargo run --quiet --example scalars -- tests/harness/scalars.txt 2>/dev/null | sed "s/ERROR(.*/ERROR/" > "$REKYLL_WORK/psych-rust.txt"
diff -u "$REKYLL_WORK/psych-ruby.txt" "$REKYLL_WORK/psych-rust.txt" && echo "=== YAML SCALARS IDENTICAL ===" || ok=1
ruby tests/harness/strftime_ref.rb tests/harness/strftime_fmts.txt > "$REKYLL_WORK/strftime-ruby.txt"
cargo run --quiet --example strftime_dump -- tests/harness/strftime_fmts.txt > "$REKYLL_WORK/strftime-rust.txt" 2>/dev/null
diff -u "$REKYLL_WORK/strftime-ruby.txt" "$REKYLL_WORK/strftime-rust.txt" && echo "=== STRFTIME IDENTICAL ===" || ok=1
exit $ok
