# Validation Report for 0.1.0

**Executed:** 2026-08-12
**Environment:** Ubuntu 24.04, Rust 1.97.1 Stable, Node.js 22.13.0, and npm 10.9.2.

## Rust quality gates

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed with no warnings. |
| `cargo test --workspace` | Passed: 16 Rust tests passed, 0 failed. |
| `cargo build --workspace --release` | Passed. |
| `cargo audit` | Passed: 83 locked crate dependencies scanned; no RustSec vulnerabilities reported. |
| `cargo publish --dry-run -p scafflare` | Correctly refused because `scafflare` has `publish = false`; no crates.io publication is intended. |

The Rust tests cover recipe parsing, path traversal and symlink escape prevention, conditions, dependency ordering, conflicts, strict template rendering, structured JSON merges, idempotency, state serialization, rollback, golden output, and preservation of user-modified files.

## Official recipes

All **15** official recipes passed `scafflare recipe validate`, including Node.js, TypeScript, Express, Hono, architecture, database, quality, and supporting recipes.

## Generated fixture matrix

Every generated fixture completed install, production dependency audit, typecheck, lint, test, and build successfully.

| Fixture | Install | `npm audit --omit=dev` | Typecheck | Lint | Test | Build |
|---|---:|---:|---:|---:|---:|---:|
| Node + TypeScript + Minimal | Passed | 0 vulnerabilities | Passed | Passed | Passed | Passed |
| Express + Minimal | Passed | 0 vulnerabilities | Passed | Passed | Passed | Passed |
| Express + Layered | Passed | 0 vulnerabilities | Passed | Passed | Passed | Passed |
| Hono + Layered | Passed | 0 vulnerabilities | Passed | Passed | Passed | Passed |
| Express + Clean + Drizzle + SQLite + Pino | Passed | 0 vulnerabilities | Passed | Passed | Passed | Passed |
| Hono + Clean + Drizzle + SQLite + Pino | Passed | 0 vulnerabilities | Passed | Passed | Passed | Passed |

## npm audit investigation

The earlier claim of **9 npm vulnerabilities** does not reproduce. No generated production dependency tree reports a vulnerability when audited with `npm audit --omit=dev`.

A full `npm audit` for the two Clean + SQLite fixtures currently reports **4 moderate** vulnerabilities. They are transitive development-tooling findings in `drizzle-kit` through `@esbuild-kit/esm-loader`, `@esbuild-kit/core-utils`, and `esbuild` (GHSA-67mh-4wv8-2f99). They are absent from `npm audit --omit=dev`, which confirms they are not installed as production dependencies. The four non-Clean fixtures report zero vulnerabilities in both audit modes. No high or critical production vulnerability was found, so no dependency upgrade or `npm audit fix --force` was performed.

## Release automation validation

The tag-triggered release workflow runs all Rust gates, validates official recipes, executes the six-fixture matrix, and runs `cargo audit` before building release archives. The Linux x86_64 packaging path was tested locally: the `scafflare` binary was archived successfully and a SHA-256 checksum manifest was generated. The workflow is configured to build native archives for Linux x86_64, macOS x86_64, macOS ARM64, and Windows x86_64; it creates a GitHub Release only for a pushed version tag and was not triggered during this validation.

## Distribution model

Scafflare distributes verified native binaries through GitHub Releases. Both `scafflare` and the internal `scafflare-core` crate set `publish = false`, so crates.io publication is intentionally disabled.
