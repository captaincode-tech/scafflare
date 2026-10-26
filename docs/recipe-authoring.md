# راهنمای ساخت Recipe

Recipe یک واحد دادهٔ قابل ترکیب است که بدون تغییر Rust core قابلیت یا ecosystem جدیدی اضافه می‌کند. Recipe شامل `recipe.yaml` و templateهای MiniJinja است. templateها فقط فایل متن تولید می‌کنند و نباید executable hook داشته باشند.

## Schema

| بخش | مسئولیت |
|---|---|
| `metadata` | name kebab-case، نسخهٔ semver و description |
| `prompts` و `variables` | ورودی و defaultهای قابل رندر |
| `depends_on` | وابستگی Recipe و ترتیب generation |
| `compatible_with` و `conflicts` | کنترل ترکیب‌های معتبر |
| `required_capabilities` | پیش‌نیاز محیط مانند `node-runtime` |
| `files` | source، destination، strategy و شرط |
| `validation_commands` | argvهای قابل نمایش و اجرای opt-in |
| `post_generation_instructions` | دستورالعمل متناظر برای کاربر |

## مسیرها و امنیت

تمام sourceها باید زیر `templates/` باشند. destination و source فقط می‌توانند مسیر relative با slash استاندارد داشته باشند. Scafflare `..`، مسیر absolute، backslash و drive prefix ویندوز را رد می‌کند. این محدودیت مسیرهای Unix و Windows را هم‌زمان پوشش می‌دهد.

## شرط‌ها

`when` می‌تواند یک boolean variable، مقایسهٔ `==` یا `!=` و conjunction با `&&` باشد:

```yaml
when: "framework == express && database == sqlite"
```

هر متغیر ناشناخته یا نوع نامعتبر، generation را پیش از نوشتن فایل متوقف می‌کند.

## strategy فایل

| strategy | رفتار |
|---|---|
| `create` | فایل جدید؛ اگر فایل موجود دقیقاً یکسان باشد no-op، در غیر این صورت conflict |
| `replace` | جایگزینی قابل preview و rollback |
| `merge_json` | parse و deep merge ساختاریافتهٔ JSON |
| `skip` | عدم تغییر فایل و ثبت در preview |
| `fail` | در صورت وجود مقصد خطای روشن می‌دهد |

در صورت وجود چند fragment برای یک JSON، Scafflare نتیجهٔ merge نهایی را فقط یک‌بار می‌نویسد. هیچ injection متنی در JSON انجام نمی‌شود.

## تست

```bash
scafflare recipe validate path/to/recipe.yaml
scafflare init fixture --non-interactive --framework hono --architecture layered --database sqlite --yes
```

Recipe جدید باید validation موفق، generation بدون conflict ناخواسته، اجرای مجدد idempotent و در صورت ساخت پروژهٔ Node.js، install/typecheck/lint/test/build معتبر داشته باشد.

## نسخه‌گذاری و ارتقا

`scafflare upgrade` بر پایهٔ `metadata.version` کار می‌کند: هر تغییر محتوا در Recipe (recipe.yaml یا templateها) باید با افزایش نسخهٔ semver همراه باشد، وگرنه پروژه‌های موجود تغییر را دریافت نمی‌کنند. برای Recipeهای رسمی، تست `recipe_checksums` بدون bump نسخه شکست می‌خورد.

- Recipe نباید نسخه‌اش را کاهش دهد؛ downgrade رد می‌شود.
- اضافه‌کردن فایل جدید امن است. حذف فایل از template باعث حذف فایلِ بدون تغییر کاربر و رهاسازی (orphan) فایل تغییرداده‌شده می‌شود.
- متغیرهای جدید باید مقدار پیش‌فرض در `variables` داشته باشند، چون `upgrade` promptی نمی‌پرسد (فقط `--set`).
- برای فایل‌هایی که کاربر معمولاً ویرایش می‌کند (مثل `package.json`) از `merge_json` استفاده کنید تا ارتقا کلید‌به‌کلید merge شود. آرایه‌ها در merge اتمی هستند.
- فایل‌های متنی با `create` به‌صورت خطی (diff3) merge می‌شوند؛ تغییر همزمان یک خط توسط کاربر و template conflict می‌سازد.

