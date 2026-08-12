## Summary

Briefly describe the change and its motivation. Link related issues when applicable.

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo build --workspace --release`
- [ ] If a recipe changed, `scafflare recipe validate` was run.
- [ ] If a Node.js template changed, every affected fixture was installed, audited with `npm audit --omit=dev`, type-checked, linted, tested, and built.
- [ ] If bundled recipes or generated Node.js behavior changed, `scripts/verify-fixtures.sh` was run.

## Security and compatibility

- [ ] New paths comply with the sandbox path validator.
- [ ] External commands remain opt-in and are not added in shell-string form.
- [ ] Breaking or user-visible changes are documented in `CHANGELOG.md`.
- [ ] No secrets, generated build output, or unrelated files are included.

> Do not disclose security vulnerabilities in a pull request. Follow [SECURITY.md](../SECURITY.md).
