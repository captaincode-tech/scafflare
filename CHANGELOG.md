# Changelog

All notable changes to Scafflare are documented in this file.

## [Unreleased]

### Added

- Release automation that verifies the full quality gate, builds native Linux, macOS, and Windows archives, generates SHA-256 checksums, and creates GitHub Release assets from verified version tags.
- Scheduled Rust dependency auditing and Dependabot configuration for Cargo and GitHub Actions.
- English-first public documentation, contributor templates, and repository ownership metadata.

### Changed

- Clarified the binary-only distribution model: neither `scafflare` nor `scafflare-core` is published to crates.io.
- Replaced obsolete project branding and documented the pre-publication identity-normalization recommendation.

## [0.1.0] - 2026-08-12

### Added

- A language-neutral CLI with `init`, `add`, `remove`, `list`, `doctor`, `validate`, and `recipe validate` commands.
- YAML recipe parsing and validation with path-traversal, dependency-cycle, conflict, compatibility, and capability controls.
- Strict MiniJinja rendering, declarative conditions, deep JSON merges, and a versioned project lockfile.
- Preview, staging-based commits, best-effort rollback, and overwrite protection.
- Fifteen official Node.js/TypeScript recipes for Express, Hono, architecture styles, Drizzle/LibSQL, Zod, Pino, Vitest, Biome, Husky, and GitHub Actions.
- A working Todo CRUD flow for Clean + SQLite, including validation, typed errors, unit tests, and integration tests.
- Multi-platform Rust quality gates and CI workflows.
