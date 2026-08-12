# Security Policy

## Supported versions

Security fixes are provided for the latest released `0.1.x` version. Before the first public release, fixes are made on the default branch and are included in the first affected release.

## Reporting a vulnerability

**Do not open a public issue for a suspected vulnerability.**

After this repository is public, use GitHub's **Private Vulnerability Reporting** form on the [Security Advisories page](https://github.com/captaincode-tech/scafflare/security/advisories/new). The report is private to repository maintainers and provides the coordinated-disclosure channel for Scafflare.

The repository is currently private, and GitHub only exposes Private Vulnerability Reporting for public repositories. Immediately before changing visibility to public, a repository administrator must enable it in **Settings → Advanced Security → Private vulnerability reporting**. Maintainers should also subscribe to the repository's **Security alerts** notifications so reports receive timely acknowledgement.

Please include the affected version or commit, a minimal reproduction, impact assessment, and any proposed mitigation. Maintainers target acknowledgement within **seven business days** and will coordinate a fix and disclosure timeline privately.

## Recipe security model

Recipes are declarative data by default. Scafflare rejects absolute paths, `..`, Windows path prefixes, and paths outside the project sandbox. Recipe-supplied external commands are retained for display and can run only with the user’s explicit `--run-commands` opt-in. String and shell-form commands are not executable. The MVP has no online registry and does not run script hooks.

## Dependency security

The repository validates Rust dependencies with `cargo audit` and validates generated production dependency trees with `npm audit --omit=dev`. Dependabot is configured to propose updates for Cargo manifests and GitHub Actions. Security checks do not replace review of dependency update compatibility or generated-project behavior.
