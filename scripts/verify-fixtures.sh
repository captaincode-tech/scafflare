#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_BIN="${CARGO_BIN:-$(command -v cargo || true)}"
WORKDIR="${SCAFFLARE_FIXTURE_WORKDIR:-$(mktemp -d)}"
KEEP_FIXTURES="${KEEP_FIXTURES:-false}"

cleanup() {
  if [[ "$KEEP_FIXTURES" != "true" ]]; then
    rm -rf "$WORKDIR"
  else
    printf 'Fixtures retained at %s\n' "$WORKDIR"
  fi
}
trap cleanup EXIT

if [[ ! -x "$CARGO_BIN" ]]; then
  printf 'Rust cargo binary not found at %s\n' "$CARGO_BIN" >&2
  exit 1
fi

run_fixture() {
  local name="$1"
  shift
  local target_dir="$WORKDIR/$name"

  printf '\n=== Generating %s ===\n' "$name"
  "$CARGO_BIN" run --quiet --manifest-path "$ROOT/Cargo.toml" -p scafflare -- \
    init "$name" --directory "$target_dir" --non-interactive --yes --quiet "$@"

  printf '=== Validating %s ===\n' "$name"
  pushd "$target_dir/$name" >/dev/null
  npm install --ignore-scripts --no-audit
  npm audit --omit=dev || true
  npm run typecheck
  npm run lint
  npm test
  npm run build
  popd >/dev/null
}

run_fixture node-minimal \
  --set framework=none \
  --set architecture=minimal \
  --set database=none \
  --set biome_enabled=true \
  --set vitest_enabled=true

run_fixture express-minimal \
  --set framework=express \
  --set architecture=minimal \
  --set database=none \
  --set biome_enabled=true \
  --set vitest_enabled=true

run_fixture express-layered \
  --set framework=express \
  --set architecture=layered \
  --set database=none \
  --set biome_enabled=true \
  --set vitest_enabled=true

run_fixture hono-layered \
  --set framework=hono \
  --set architecture=layered \
  --set database=none \
  --set biome_enabled=true \
  --set vitest_enabled=true

run_fixture express-clean-sqlite \
  --set framework=express \
  --set architecture=clean \
  --set database=sqlite \
  --set biome_enabled=true \
  --set vitest_enabled=true \
  --set pino_enabled=true

run_fixture hono-clean-sqlite \
  --set framework=hono \
  --set architecture=clean \
  --set database=sqlite \
  --set biome_enabled=true \
  --set vitest_enabled=true \
  --set pino_enabled=true

printf '\nAll six fixture validation gates passed.\n'
