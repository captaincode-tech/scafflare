# Changelog

All notable changes to Scafflare are documented in this file.

## [Unreleased]

## [0.2.0] - Unreleased

### Added

- `scafflare upgrade`: brings every installed recipe to the bundled version with a three-way merge (base render, your working tree, new render). Your edits are never overwritten; untouched files fast-forward, edits to different lines merge, and `package.json`-style JSON merges key by key. Supports `--dry-run`, `--diff`, `--yes`, `--set KEY=VALUE`, `--json`, and `--conflicts abort|markers`.
- `scafflare status`: reports outdated recipes, modified or missing managed files, and unresolved conflicts; `--exit-code` exits with 2 when attention is needed.
- Conflict handling that is safe by default: `abort` writes nothing and reports every conflict (exit code 2); `markers` writes git-style markers for text and a `<file>.scafflare-new` sidecar for JSON, unmanaged, and no-base files, and later runs refuse to continue until they are resolved.
- Lockfile version 2 records a content-addressed merge base for each managed file under `.scafflare/base/`. Version 1 lockfiles are read and migrated automatically; files without a base degrade safely (fast-forward when untouched, additive JSON merge, otherwise a reported conflict).
- Official recipe content guard (`recipes/official/CHECKSUMS`): CI fails when a recipe changes without a version bump.
- Extensive tests: property-based merge tests, 26 upgrade scenarios including crash-convergence at every write point and random operation sequences, CLI end-to-end tests, golden project trees, and `scripts/verify-upgrade.sh`, which upgrades real npm projects.

- Release automation that verifies the full quality gate, builds native Linux, macOS, and Windows archives, generates SHA-256 checksums, and creates GitHub Release assets from verified version tags.
- Scheduled Rust dependency auditing and Dependabot configuration for Cargo and GitHub Actions.
- English-first public documentation, contributor templates, and repository ownership metadata.

### Changed

- `scafflare add` now merges into files you have edited (for example `package.json` after `npm install <pkg>`) instead of refusing; it asks you to run `scafflare upgrade` first when installed recipes are outdated. `scafflare remove` also requires an up-to-date project and keeps its conservative behaviour (modified files are preserved, manifest entries are never removed).
- Writes are ordered so an interrupted run converges on the next attempt: project files, new bases, the lockfile, and only then obsolete bases.
- Minimum supported Rust version is now 1.85 (the previous 1.75 claim no longer matched the locked dependencies); CI verifies it.
- Clarified the binary-only distribution model: neither `scafflare` nor `scafflare-core` is published to crates.io.
- Replaced obsolete project branding and documented the pre-publication identity-normalization recommendation.

### Breaking

- Projects touched by 0.2.0 get `lockfile_version: 2` and `.scafflare/base/`. Scafflare 0.1.x rejects version 2 lockfiles with `unsupported lockfile_version`; commit `.scafflare/` to version control so the whole team upgrades together.

## [0.1.0] - 2026-08-12

### Added

- A language-neutral CLI with `init`, `add`, `remove`, `list`, `doctor`, `validate`, and `recipe validate` commands.
- YAML recipe parsing and validation with path-traversal, dependency-cycle, conflict, compatibility, and capability controls.
- Strict MiniJinja rendering, declarative conditions, deep JSON merges, and a versioned project lockfile.
- Preview, staging-based commits, best-effort rollback, and overwrite protection.
- Fifteen official Node.js/TypeScript recipes for Express, Hono, architecture styles, Drizzle/LibSQL, Zod, Pino, Vitest, Biome, Husky, and GitHub Actions.
- A working Todo CRUD flow for Clean + SQLite, including validation, typed errors, unit tests, and integration tests.
- Multi-platform Rust quality gates and CI workflows.
