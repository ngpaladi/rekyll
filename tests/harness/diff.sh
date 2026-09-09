#!/usr/bin/env bash
# Differential harness: build a fixture with real Jekyll and with rekyll, compare byte-for-byte.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURES="$ROOT/tests/fixtures"
WORK="${REKYLL_WORK:-/tmp/rekyll-diff}"
REKYLL_BIN="${REKYLL_BIN:-$ROOT/target/debug/rekyll}"

pass=0; fail=0; failed_names=()

run_one() {
  local name="$1" src="$FIXTURES/$1"
  local exp="$WORK/$name/expected" act="$WORK/$name/actual"
  rm -rf "$exp" "$act"; mkdir -p "$exp" "$act"

  # Real Jekyll. JEKYLL_ENV pinned so `jekyll.environment` is deterministic.
  if ! JEKYLL_ENV=development jekyll build -s "$src" -d "$exp" \
        --disable-disk-cache >"$WORK/$name/jekyll.log" 2>&1; then
    echo "  [SKIP] $name (jekyll build failed; see $WORK/$name/jekyll.log)"
    return 2
  fi
  if ! JEKYLL_ENV=development "$REKYLL_BIN" build -s "$src" -d "$act" \
        >"$WORK/$name/rekyll.log" 2>&1; then
    echo "  [FAIL] $name (rekyll build failed)"
    tail -20 "$WORK/$name/rekyll.log" | sed 's/^/      /'
    return 1
  fi

  if diff -r -u "$exp" "$act" >"$WORK/$name/diff.txt" 2>&1; then
    echo "  [ OK ] $name"
    return 0
  else
    echo "  [FAIL] $name"
    head -60 "$WORK/$name/diff.txt" | sed 's/^/      /'
    return 1
  fi
}

names=("$@")
if [ ${#names[@]} -eq 0 ]; then
  mapfile -t names < <(cd "$FIXTURES" && for e in *; do [ -d "$e" ] && echo "$e"; done | sort)
fi

echo "=== rekyll differential harness (${#names[@]} fixtures) ==="
for n in "${names[@]}"; do
  mkdir -p "$WORK/$n"
  run_one "$n"
  case $? in
    0) pass=$((pass+1)) ;;
    1) fail=$((fail+1)); failed_names+=("$n") ;;
  esac
done

echo "=== $pass passed, $fail failed ==="
[ $fail -eq 0 ] || { echo "failed: ${failed_names[*]}"; exit 1; }
