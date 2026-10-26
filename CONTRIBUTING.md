# Contributing to Scafflare

Thank you for considering a contribution. Please open an issue before starting a large change so the scope and recipe contract can be agreed before implementation.

## Development setup

```bash
git clone https://github.com/captaincode-tech/scafflare.git
cd scafflare
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Rust changes must include relevant tests. Recipe changes must pass `scafflare recipe validate` and at least one non-interactive generation. For Node.js template changes, run `npm install`, `npm run typecheck`, `npm run lint`, `npm test`, and `npm run build` for every affected fixture. Run `scripts/verify-fixtures.sh` before submitting a change that affects bundled recipes or generated Node.js projects, and `scripts/verify-upgrade.sh` for changes to upgrade, merge, or lockfile behavior.

Upgrades are driven by recipe versions: if you change the content of an official recipe, bump its `metadata.version` and refresh the manifest with `UPDATE_CHECKSUMS=1 cargo test -p scafflare-core --test recipe_checksums`. CI fails when content changes without a version bump. Golden project trees can be refreshed with `UPDATE_GOLDEN=1 cargo test -p scafflare --test golden_trees` after reviewing the diff.

## Design principles

The core must not hard-code knowledge of a particular framework or language. New paths must pass the safe-path validator, structured data must be changed through parsing and structured merging, and no external command may run without an explicit user opt-in.

## Pull requests

Keep pull requests focused and reviewable, use a meaningful commit message, and describe the motivation and validation performed. Do not introduce TODOs, mocks, or placeholders in an accepted change. Document user-visible changes in `CHANGELOG.md`; document breaking changes explicitly.

Before opening a pull request, complete the checklist in the pull-request template. Security issues must follow the private process in [SECURITY.md](SECURITY.md), not the public issue tracker.
