# گزارش اعتبارسنجی نسخه 0.1.0

**تاریخ اجرا:** ۱۲ اوت ۲۰۲۶  
**محیط:** Ubuntu 24.04، Rust 1.97.1 Stable، Node.js 22.13.0 و npm 10.9.2

## گیت‌های Rust

| دستور | نتیجه |
|---|---|
| `cargo fmt --all -- --check` | موفق |
| `cargo clippy --workspace --all-targets -- -D warnings` | موفق، بدون warning |
| `cargo test --workspace` | موفق، ۱۳ تست واحد/یکپارچهٔ Rust |
| `cargo build --release` | در اعتبارسنجی release اجرا می‌شود |

تست‌های Rust parser، path traversal، condition، dependency ordering، conflict، strict template rendering، JSON merge، idempotency، state serialization، rollback، golden output و محافظت از فایل کاربر را پوشش می‌دهند.

## fixtureهای تولیدشده

| ترکیب | install | typecheck | lint | test | build |
|---|---:|---:|---:|---:|---:|
| Node + TypeScript + Minimal | موفق | موفق | موفق | موفق | موفق |
| Express + Clean + Drizzle + SQLite + Zod + Pino | موفق | موفق | موفق | موفق | موفق |
| Hono + Layered + Drizzle + SQLite | موفق | موفق | موفق | موفق | موفق |
| Express بدون Database | موفق | موفق | موفق | موفق | موفق |
| Non-interactive generation | موفق | موفق | موفق | موفق | موفق |

Recipeهای رسمی نیز به‌صورت مستقل با `stackforge recipe validate` بررسی شدند.

## آزمون عملی server و CRUD

نمونهٔ Clean + Express پس از `npm run db:push` و `npm run build` با `NODE_ENV=production` اجرا شد. `GET /health` پاسخ `{"status":"ok"}` داد. عملیات `POST /todos`، `GET /todos`، `PATCH /todos/1` و `DELETE /todos/1` به‌ترتیب پاسخ‌های صحیح `201`، `200`، `200` و `204` دادند.

## نکتهٔ dependency audit

`npm install` نمونهٔ کامل در زمان اجرا ۹ آسیب‌پذیری transitively reported کرد: ۶ مورد moderate، ۲ مورد high و ۱ مورد critical. این نتیجه به dependency tree منتشرشده در registry مربوط است و نباید با `npm audit fix --force` بدون بازبینی breaking change رفع شود. پیش از انتشار رسمی، نگه‌دارنده باید نسخه‌های dependency را با audit به‌روز بازبینی کند.
