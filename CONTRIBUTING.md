# مشارکت در StackForge

از مشارکت شما استقبال می‌کنیم. لطفاً پیش از ایجاد Pull Request، یک issue برای تغییرهای بزرگ باز کنید تا scope و قرارداد Recipe به توافق برسد.

## راه‌اندازی توسعه

```bash
git clone <your-fork-url>
cd stackforge
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

هر تغییر Rust باید تست مرتبط داشته باشد. هر تغییر در Recipe باید با `stackforge recipe validate` و حداقل یک generation غیرتعاملی تست شود. برای تغییر template Node.js، `npm install`، `npm run typecheck`، `npm run lint`، `npm test` و `npm run build` را برای fixture مرتبط اجرا کنید.

## اصول طراحی

هسته نباید دربارهٔ framework یا language خاص دانش hard-coded داشته باشد. مسیرها باید با validator ایمن کنترل شوند، داده‌های ساختاریافته باید با parser و merge ساختاریافته تغییر کنند، و هیچ command خارجی نباید بدون انتخاب صریح کاربر اجرا شود.

## Pull Request

Pull Request باید تغییر محدود و قابل review داشته باشد، پیام commit معنادار داشته باشد، و گیت‌های Rust و fixtureهای مرتبط را پاس کند. از افزودن TODO، mock یا placeholder به تغییرات پذیرش‌شده خودداری کنید.
