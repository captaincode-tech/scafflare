## خلاصه

تغییر و انگیزهٔ آن را به‌صورت کوتاه توضیح دهید.

## اعتبارسنجی

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] در صورت تغییر Recipe، `stackforge recipe validate` اجرا شده است.
- [ ] در صورت تغییر template Node.js، fixture مرتبط install، typecheck، lint، test و build شده است.

## امنیت و سازگاری

- [ ] مسیرهای جدید با sandbox path validator سازگار هستند.
- [ ] تغییر command خارجی بدون اجرای خودکار و با opt-in باقی مانده است.
- [ ] تغییر breaking در CHANGELOG ثبت شده است.
