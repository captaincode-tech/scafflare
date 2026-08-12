# Scafflare

**Scafflare** یک CLI سریع و standalone در Rust برای ساخت backendهای Node.js/TypeScript با Recipeهای YAML ترکیب‌پذیر است. هسته نسبت به زبان و framework بی‌طرف است: Express، Hono، Drizzle و ابزارهای کیفیت صرفاً Recipe هستند، نه وابستگی‌های hard-coded هسته.

> امنیت پیش‌فرض: Recipeها داده هستند. Scafflare هیچ command خارجی را خودکار اجرا نمی‌کند؛ اجرای validation commandهای اعلام‌شده فقط با `--run-commands` ممکن است.

## نصب

برای ساخت از source به Rust Stable نیاز دارید:

```bash
git clone https://github.com/scafflare/scafflare.git
cd scafflare
cargo build --release
./target/release/scafflare --help
```

Binary تولیدشده در `target/release/scafflare` روی Linux، macOS و Windows قابل انتشار است. در Windows نام فایل `scafflare.exe` خواهد بود.

## شروع سریع

نمونهٔ کامل Express + Clean Architecture + SQLite/LibSQL + Drizzle را ایجاد کنید:

```bash
scafflare init todo-api \
  --non-interactive \
  --framework express \
  --architecture clean \
  --database sqlite \
  --pino --vitest --biome --hooks --github-actions \
  --yes

cd todo-api
npm install
npm run db:push
npm run typecheck
npm run lint
npm test
npm run build
npm run dev
```

برای wizard تعاملی، فقط `scafflare init todo-api` را اجرا کنید. قبل از هر تغییر، preview فایل‌ها نمایش داده می‌شود. برای automation باید `--yes` را صریحاً وارد کنید.

![Scafflare init → build → health-check demo](docs/assets/scafflare-demo.gif)

## دستورات

| دستور | کاربرد |
|---|---|
| `scafflare init <project-name>` | ایجاد پروژه با wizard یا flagهای non-interactive |
| `scafflare add <recipe...>` | افزودن Recipe به پروژهٔ مدیریت‌شده |
| `scafflare remove <recipe>` | حذف ایمن Recipe و فایل‌های exclusively-owned و بدون تغییر کاربر |
| `scafflare list` | نمایش Recipeهای bundle شده |
| `scafflare doctor` | بررسی Node/npm و lockfile پروژه |
| `scafflare validate` | بررسی lockfile و نسخهٔ Recipeهای نصب‌شده |
| `scafflare recipe validate <path>` | اعتبارسنجی recipe.yaml و templateهای مرجع |

گزینه‌های global `--json` و `--quiet` برای CI و automation موجود هستند. خروجی `--json` فقط به stdout نوشته می‌شود. `--run-commands` commandهای validation در فرم آرایه‌ای YAML را بعد از commit اجرا می‌کند؛ commandهای shell-like هرگز اجرا نمی‌شوند.

## Recipeهای رسمی 0.1

| دسته | Recipeها |
|---|---|
| Runtime و زبان | `node`، `typescript` |
| HTTP | `express`، `hono` |
| معماری | `architecture-minimal`، `architecture-layered`، `architecture-clean` |
| Data | `sqlite-libsql`، `drizzle` |
| قابلیت‌ها | `zod`، `pino` |
| کیفیت | `vitest`، `biome`، `husky-lint-staged`، `github-actions` |

انتخاب **Clean + SQLite** به‌صورت خودکار `zod` را اضافه می‌کند و Todo CRUD واقعی در مسیر `Route → Controller → Service → Repository Interface → Drizzle Repository → Database` تولید می‌شود.

## ساخت Recipe جدید

هر Recipe یک پوشه با `recipe.yaml` و templateهای زیر `templates/` است:

```text
recipes/custom/example/
├── recipe.yaml
└── templates/
    └── src/example.ts.jinja
```

```yaml
schema_version: 1
metadata:
  name: example
  version: 0.1.0
  description: An example extension
variables:
  enabled: true
depends_on: [typescript]
conflicts: []
files:
  - source: templates/src/example.ts.jinja
    destination: src/example.ts
    strategy: create
    when: "enabled"
validation_commands:
  - [npm, run, typecheck]
post_generation_instructions:
  - Review the generated example module.
```

مسیرهای `source` و `destination` باید relative و فاقد `..`، مسیر absolute یا prefix ویندوز باشند. Strategyهای پشتیبانی‌شده `create`، `replace`، `merge_json`، `skip` و `fail` هستند. `merge_json` همیشه merge ساختاریافته انجام می‌دهد و string injection در `package.json` یا `tsconfig.json` ندارد.

برای آزمایش یک Recipe محلی:

```bash
scafflare recipe validate recipes/custom/example/recipe.yaml
```

جزئیات چرخهٔ generation، lockfile و مدل امنیتی در [معماری](docs/architecture.md) آمده است.

## توسعه و quality gates

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

CI پروژه همین گیت‌ها را روی Linux، macOS و Windows اجرا و binaryهای release را artifact می‌کند. گزارش اجرای واقعی fixtureها و CRUD در [گزارش اعتبارسنجی](docs/validation-report.md) ثبت شده است.

## وضعیت MVP و محدودیت‌ها

Registry آنلاین، signature verification، pluginهای native، Laravel/Python/Go، Prisma و Docker عمداً خارج از scope نسخهٔ `0.1.0` هستند. registry abstraction و Recipeهای bundle شده، مبنای توسعهٔ بعدی را فراهم کرده‌اند. حذف Recipe وابستگی‌های package manager را از manifest پاک‌سازی نمی‌کند؛ این محدودیت برای جلوگیری از حذف ناامن dependencyهای اشتراکی در بخش Roadmap ثبت شده است.

## مجوز

Scafflare تحت [MIT License](LICENSE) منتشر می‌شود.
