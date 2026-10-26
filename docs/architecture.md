# معماری Scafflare MVP

**وضعیت:** پذیرفته‌شده  
**نسخهٔ هدف:** `0.1.0`

## هدف و مرزبندی

Scafflare یک CLI مستقل از زبان برای ساخت backendهای آمادهٔ production از Recipeهای YAML است. هسته فقط YAML، قالب و عملیات فایل را می‌شناسد؛ دانسته‌های Node.js، TypeScript، Express، Hono و Drizzle فقط در Recipeهای رسمی bundle شده قرار دارند. این جداسازی از الگوی template-based ابزارهای scaffolding الهام می‌گیرد، اما برخلاف ابزارهایی که hookهای اسکریپتی را به‌طور پیش‌فرض اجرا می‌کنند، اجرای command در Scafflare کاملاً opt-in است.[1] [2]

> اصل امنیتی: **هر Recipe داده است، نه کد قابل اجرا.** Commandهای خارجی فقط به‌عنوان metadata نمایش داده می‌شوند و تنها با `--run-commands` اجرا خواهند شد.

| مؤلفه | مسئولیت | وابستگی به اکوسیستم هدف |
|---|---|---|
| `scafflare-core::recipe` | parse، schema validation و discovery | ندارد |
| `scafflare-core::resolver` | dependency، conflict، compatibility و capability | ندارد |
| `scafflare-core::render` | قالب MiniJinja و variable context | ندارد |
| `scafflare-core::merge` | merge سه‌طرفهٔ JSON (کلید‌به‌کلید) و متن (diff3) | ندارد |
| `scafflare-core::reconcile` | تطبیق base/ours/theirs، conflict و outcome برای `init`/`add`/`upgrade`/`remove` | ندارد |
| `scafflare-core::blobs` | ذخیرهٔ content-addressed مبنای merge در `.scafflare/base/` | ندارد |
| `scafflare-core::transaction` | staging، preview، commit و rollback | ندارد |
| `scafflare-core::state` | lockfile نسخه‌دار `.scafflare/lock.yaml` | ندارد |
| `scafflare-core::doctor` | کنترل وضعیت ابزار و پروژه | ندارد |
| `scafflare-cli` | Clap، wizard، output human/JSON | ندارد |
| `recipes/official` | metadata، template و validation commandها | فقط در Recipe |

## ساختار workspace

```text
scafflare/
├── crates/
│   ├── scafflare-core/   # library قابل تست
│   └── scafflare-cli/    # binary تک‌فایل در انتشار
├── recipes/official/      # Recipeهای bundle شده
├── fixtures/              # recipe و خروجی‌های golden
├── docs/
└── .github/
```

## چرخهٔ دستور `init`

`init` ابتدا نام پروژه و مسیر مقصد را اعتبارسنجی می‌کند. در حالت تعاملی، wizard انتخاب‌ها را به نام Recipeها تبدیل می‌کند؛ در حالت non-interactive، flagها دقیقاً همین selection را می‌سازند. سپس resolver، graph dependency را مرتب می‌کند، conflict و capabilityهای موردنیاز را پیش از تغییر دیسک گزارش می‌دهد. renderer همهٔ فایل‌ها را در staging می‌سازد، و file planner یک preview deterministic به کاربر می‌دهد.

پس از تأیید preview، commit engine فایل‌ها را به مقصد منتقل می‌کند. اگر هر انتقال شکست بخورد، فایل‌های تازه‌ساخته‌شده حذف و فایل‌های overwrite شده از backup بازگردانده می‌شوند. lockfile فقط در انتهای commit موفق نوشته می‌شود. در اجرای غیرتعاملی، `--yes` تأیید preview را جایگزین می‌کند؛ نبود آن باعث خروج امن می‌شود.

## قرارداد Recipe

هر Recipe یک پوشه با فایل `recipe.yaml` و یک درخت `templates/` است. مسیرهای مقصد، مسیرهای template و متغیرهای شرطی با sandbox path validator کنترل می‌شوند: مسیر absolute، prefixهای Windows، یا هر component برابر `..` مردود است.

```yaml
schema_version: 1
metadata:
  name: example
  version: 0.1.0
  description: Example extension
prompts:
  - key: project_name
    message: Project name
    required: true
variables:
  package_manager: npm
depends_on: [node, typescript]
compatible_with: [architecture-minimal]
conflicts: []
required_capabilities: [node-runtime]
files:
  - source: templates/src/example.ts.jinja
    destination: src/example.ts
    strategy: create
    when: "feature_enabled == true"
  - source: templates/package.fragment.json.jinja
    destination: package.json
    strategy: merge_json
validation_commands:
  - npm run typecheck
post_generation_instructions:
  - Copy `.env.example` to `.env` before starting the server.
```

| فیلد | معنا | اعتبارسنجی MVP |
|---|---|---|
| `metadata` | شناسه و نسخهٔ Recipe | name kebab-case، نسخهٔ semver، description غیرخالی |
| `prompts` و `variables` | context قالب | کلید unique و مقدار scalar |
| `depends_on` | Recipeهای لازم | cycle-free و قابل resolve |
| `compatible_with` | انتخاب‌های سازگار | همهٔ انتخاب‌های صریح باید match شوند |
| `conflicts` | Recipeهای غیرقابل هم‌زیستی | conflict قطعی قبل از write |
| `required_capabilities` | قابلیت‌های محیط | با capabilityهای command تطبیق داده می‌شود |
| `files` | عملیات render و merge | مسیر sandboxed و strategy معتبر |
| `validation_commands` | دستورهای پیشنهادی | نمایش پیش‌فرض؛ اجرای opt-in |

## راهبردهای فایل

| Strategy | رفتار | Idempotency |
|---|---|---|
| `create` | فایل تازه می‌سازد؛ محتوای یکسان را no-op می‌کند | بله |
| `replace` | فقط با تأیید overwrite، محتوا را جایگزین می‌کند | بله |
| `merge_json` | JSON template را parse و با فایل مقصد deep-merge می‌کند | بله |
| `skip` | تغییر نمی‌دهد و در preview ثبت می‌شود | بله |
| `fail` | وجود فایل مقصد را خطا می‌کند | بله |

## ارتقا و merge سه‌طرفه

lockfile نسخهٔ ۲ برای هر فایل مدیریت‌شده دو hash نگه می‌دارد: `sha256` (محتوای روی دیسک پس از آخرین عملیات، برای تشخیص تغییر کاربر) و `base` (render خالص بدون تغییر کاربر که در `.scafflare/base/<sha256>` ذخیره می‌شود و ancestor merge است). جداکردن این دو ضروری است؛ اگر hash فایل merge‌شده base می‌شد، ارتقای بعدی تغییرات کاربر را به‌عنوان تغییر template می‌دید و پاک می‌کرد. محتوای مقایسه‌شده و hash‌شده از نظر پایان خط (CRLF→LF) نرمال می‌شود و سبک پایان خط فایل کاربر هنگام نوشتن حفظ می‌شود.

برای هر مسیر با B=base، O=دیسک و T=render جدید:

| شرط | نتیجه |
|---|---|
| O == T | بدون نوشتن؛ base=T |
| O == B | نوشتن T (fast-forward) |
| T == B | حفظ O |
| هر دو تغییر کرده‌اند | JSON: merge سه‌طرفهٔ ساختاری؛ متن: diff3؛ در غیر این صورت conflict |
| T حذف شده | حذف اگر O == B، وگرنه فایل رها و orphan می‌شود |
| O حذف شده | احترام به حذف اگر T == B، وگرنه conflict |
| فایل جدید در T با فایل غیرمدیریت‌شده | adopt اگر یکسان است، وگرنه conflict |
| بدون base (lockfile v1) | fast-forward اگر hash(O) برابر `sha256` است؛ JSON: merge افزایشی؛ متن: conflict `no_base` |

همهٔ مسیرها پیش از هر نوشتن تحلیل می‌شوند؛ در حالت `abort` در صورت وجود conflict، plan خالی است و درخت پروژه byte-identical می‌ماند. ترتیب commit: فایل‌های پروژه، baseهای جدید، lockfile و در پایان حذف baseهای بی‌مصرف؛ بنابراین قطع شدن میانهٔ کار با اجرای دوبارهٔ `upgrade` همگرا می‌شود و lockfile قدیمی هرگز به base حذف‌شده اشاره نمی‌کند. `remove` عمداً محافظه‌کار است: فایل‌های تغییرداده‌شده حفظ و ورودی‌های manifest حذف نمی‌شوند.

## فرمت lockfile

```yaml
lockfile_version: 2
project_name: acme-api
recipes:
  - name: node
    version: 0.1.0
  - name: express
    version: 0.1.0
variables:
  framework: express
  architecture: clean
managed_files:
  package.json:
    recipes: [node, express]
    sha256: <hash محتوای روی دیسک>
    base: <hash render خالص؛ در v1 وجود ندارد>
```

## خطا و خروجی

خطاها به صورت `ScafflareError` typed مدل می‌شوند. هر خطا شامل code پایدار، message قابل‌فهم، recipe و path (در صورت وجود) است. خروجی انسانی فقط concise و رنگی است؛ `--json` به جای آن یک envelope قابل‌ماشین می‌دهد. `--quiet` پیام‌های موفقیت را حذف می‌کند، نه خطاها را.

## Registry

`Registry` یک trait با عملیات `list()`، `get()` و `source()` است. MVP از `BundledRegistry` مبتنی بر `include_dir` استفاده می‌کند. Registry online عمداً پیاده‌سازی نمی‌شود، اما رابط آن امکان افزودن registry cache و signature verification در نسخه‌های بعدی را نگه می‌دارد.

## تصمیم‌های معماری

- **ADR-001:** staging directory در کنار مقصد و commit با rollback best-effort به‌جای نوشتن مستقیم.
- **ADR-002:** JSON deep merge به‌جای string injection برای manifestهای ساختاریافته.
- **ADR-003:** command خارجی در data model باقی می‌ماند اما به‌صورت پیش‌فرض اجرا نمی‌شود.
- **ADR-004:** Recipeها همراه binary bundle می‌شوند تا CLI در حالت offline کار کند.
- **ارتقای سه‌طرفه (docs/adr/002-three-way-upgrade.md):** ارتقا با merge سه‌طرفه و base content-addressed.

## منابع

[1]: https://cargo-generate.github.io/cargo-generate/ "Cargo Generate Documentation"
[2]: https://yeoman.io/authoring/ "Writing Your Own Yeoman Generator"
