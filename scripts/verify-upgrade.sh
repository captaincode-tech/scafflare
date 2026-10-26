#!/usr/bin/env bash
# Verifies `scafflare upgrade` against real npm projects:
#   1. generate with the legacy `node` 0.1.0 recipe (fixtures/upgrade/legacy-recipes)
#   2. npm install + `npm install dayjs` (a real package.json change) + a hand edit
#   3. upgrade with the bundled recipes and assert user changes survived
#   4. install, typecheck, lint, test, build, and confirm a second upgrade is a no-op
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKDIR="${SCAFFLARE_FIXTURE_WORKDIR:-$(mktemp -d)}"
KEEP_FIXTURES="${KEEP_FIXTURES:-false}"
LEGACY="$ROOT/fixtures/upgrade/legacy-recipes"

cleanup() {
  if [[ "$KEEP_FIXTURES" != "true" ]]; then
    rm -rf "$WORKDIR"
  else
    printf 'Fixtures retained at %s\n' "$WORKDIR"
  fi
}
trap cleanup EXIT

cargo build --quiet --manifest-path "$ROOT/Cargo.toml" -p scafflare
BIN="$ROOT/target/debug/scafflare"
[[ -x "$BIN" ]] || { printf 'scafflare binary not found at %s\n' "$BIN" >&2; exit 1; }

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }

check_json() {
  # check_json <file> <node expression using `p`> <description>
  node -e "const p = JSON.parse(require('fs').readFileSync('$1','utf8')); process.exit(($2) ? 0 : 1)" \
    || fail "$3"
}

run_upgrade_fixture() {
  local name="$1"
  shift
  local parent="$WORKDIR/$name"
  local dir="$parent/$name"

  printf '\n=== Legacy init %s ===\n' "$name"
  "$BIN" --recipe-root "$LEGACY" init "$name" --directory "$parent" \
    --non-interactive --yes --quiet "$@"
  grep -q 'version: 0.1.0' "$dir/.scafflare/lock.yaml" || fail "legacy lockfile missing node 0.1.0"

  pushd "$dir" >/dev/null
  npm install --ignore-scripts --no-audit --no-fund
  npm install dayjs --ignore-scripts --no-audit --no-fund
  printf '// my custom header\n' | cat - src/server.ts > src/server.ts.tmp && mv src/server.ts.tmp src/server.ts

  printf '=== Upgrade %s ===\n' "$name"
  "$BIN" status --exit-code && fail "status should report the outdated project" || [[ $? -eq 2 ]]
  "$BIN" upgrade --dry-run --diff >/dev/null
  "$BIN" upgrade --yes

  grep -q 'name: node' .scafflare/lock.yaml
  node -e "
    const lock = require('fs').readFileSync('.scafflare/lock.yaml','utf8');
    if (!/name: node\n  version: 0\.2\.0/.test(lock)) process.exit(1);
  " || fail "node recipe was not upgraded to 0.2.0"
  check_json package.json "p.dependencies && p.dependencies.dayjs" "dayjs dependency was lost"
  check_json package.json "p.engines.node === '>=22.13.0'" "template change was not applied"
  head -n 1 src/server.ts | grep -q 'my custom header' || fail "user edit to src/server.ts was lost"

  printf '=== Validate upgraded %s ===\n' "$name"
  npm install --ignore-scripts --no-audit --no-fund
  npm run typecheck
  npm run lint
  npm test
  npm run build

  "$BIN" upgrade --yes | grep -q 'already up to date' || fail "second upgrade must be a no-op"
  "$BIN" validate
  "$BIN" status --exit-code
  popd >/dev/null
}

run_upgrade_fixture express-clean-sqlite \
  --set framework=express \
  --set architecture=clean \
  --set database=sqlite \
  --set biome_enabled=true \
  --set vitest_enabled=true \
  --set pino_enabled=true

run_upgrade_fixture hono-layered \
  --set framework=hono \
  --set architecture=layered \
  --set database=none \
  --set biome_enabled=true \
  --set vitest_enabled=true

printf '\n=== Smoke test on shipped version 1 lockfile example ===\n'
cp -R "$ROOT/examples/clean-express-todo" "$WORKDIR/example"
"$BIN" status --directory "$WORKDIR/example" >/dev/null
"$BIN" upgrade --dry-run --directory "$WORKDIR/example" >/dev/null

printf '\nAll upgrade fixture gates passed.\n'
