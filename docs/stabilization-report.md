# Scafflare stabilization report

**Date:** 2026-08-12
**Scope:** Existing-MVP stabilization only. No new product surface was introduced.

## Outcome

The stabilization gates that can run locally completed successfully. The Rust workspace passes formatting, strict Clippy, unit/integration tests, official Recipe validation, and release build. The generated-project matrix passed for six supported combinations. Each fixture completed generation, dependency installation, production-only audit, type checking, linting, tests, and build.

> This report does **not** label the project production-ready. The local gates below have passed, but the updated GitHub Actions workflow has not yet run remotely on a hosted runner.

| Gate | Command / coverage | Result |
|---|---|---|
| Formatting | `cargo fmt --all -- --check` | Passed |
| Rust linting | `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| Rust tests | `cargo test --workspace` | Passed: 13 core tests |
| Recipe validation | All 15 official `recipe.yaml` files | Passed |
| Release build | `cargo build --workspace --release` | Passed |
| Fixture matrix | 6 generated Node/TypeScript projects | Passed |
| Production dependency audit | `npm audit --omit=dev` for every fixture | Passed: 0 production vulnerabilities |

## Generated-project matrix

| Fixture | Framework | Architecture | Database | Result |
|---|---|---|---|---|
| `node-minimal` | None | Minimal | None | Passed |
| `express-minimal` | Express | Minimal | None | Passed |
| `express-layered` | Express | Layered | None | Passed |
| `hono-layered` | Hono | Layered | None | Passed |
| `express-clean-sqlite` | Express | Clean | SQLite / LibSQL / Drizzle | Passed |
| `hono-clean-sqlite` | Hono | Clean | SQLite / LibSQL / Drizzle | Passed |

Each matrix row runs `npm install --ignore-scripts`, `npm audit --omit=dev`, `npm run typecheck`, `npm run lint`, `npm test`, and `npm run build`.

## Stabilized behavior

The declarative Node entrypoint now exposes framework, architecture, database and optional cross-cutting capabilities through Recipe metadata and CLI `--set` values, rather than hard-coded Node selection logic. Local and community Recipe directories remain available through `--recipe-root`.

Managed-file safety is covered by the core integration suite. Adding a Recipe can re-render an unchanged managed file, while a user-modified managed file is refused. Removing a Recipe preserves user-modified managed files instead of replacing or deleting them. Transaction targets are also checked to prevent symlink escapes.

Layered output now has a single `src/server.ts` owner per framework adapter. The Express and Hono adapters wire the Layered Todo routes, controller, service, and in-memory repository; Clean SQLite wiring continues to use the database-backed implementation. This removes the previous conflicting `src/server.ts` ownership.

## Audit note

The two SQLite fixtures emit four **development-dependency** findings during plain `npm install`. The mandatory production audit is clean because the matrix executes `npm audit --omit=dev` and reports zero production vulnerabilities. The raw audit JSON for a generated Clean + Express + SQLite fixture is in `docs/stabilization-npm-audit.json`; its vulnerability totals are all zero.

## Evidence files

| File | Contents |
|---|---|
| `docs/stabilization-rust-test.log` | Complete Rust test run |
| `docs/stabilization-clippy.log` | Strict Clippy run |
| `docs/stabilization-release-build.log` | Release build run |
| `docs/stabilization-fixture-matrix.log` | Complete six-fixture generation and Node validation matrix |
| `docs/stabilization-npm-audit.json` | Production-only audit JSON |
| `scripts/verify-fixtures.sh` | Reproducible local matrix script |
| `.github/workflows/ci.yml` | Hosted CI including the same fixture matrix |
