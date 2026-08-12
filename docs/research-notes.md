# یادداشت بررسی رقبا

**تاریخ:** ۱۲ اوت ۲۰۲۶

## یافته‌ها

Cargo Generate از یک مخزن template و موتور قالب استفاده می‌کند و می‌تواند فایل‌های قالب‌دار را به مسیر نهایی تبدیل کند. Scafflare این ایده را نگه می‌دارد، اما برای جلوگیری از اجرای پیش‌فرض کد دلخواه، hook/scripting را در MVP غیرفعال و command خارجی را صرفاً به‌صورت دستورهای اعلامیِ نیازمند تأیید کاربر مدل می‌کند.[1]

Yeoman نشان می‌دهد که composition مولدها و ذخیره‌سازی یک فایل وضعیت در ریشهٔ پروژه، مدل مناسبی برای ادامهٔ کار روی یک پروژه است. Scafflare همین قابلیت را با lockfile مستقل `.scafflare/lock.yaml` پیاده می‌کند، بدون وابستگی runtime به Node.js.[2]

Hygen ارزش templateهای کوچک، محلی و ترکیب‌پذیر را نشان می‌دهد؛ با این تفاوت که تزریق متنی در فایل‌های ساختاریافته می‌تواند شکننده باشد. Scafflare برای JSON از merge ساختاریافتهٔ `serde_json::Value` استفاده می‌کند و برای دیگر فایل‌ها فقط replacement/skip/fail شفاف ارائه می‌دهد.[3]

## تصمیم‌های حاصل

| موضوع | انتخاب MVP | علت |
|---|---|---|
| منبع Recipe | Recipeهای bundle شده در کنار binary با registry abstraction | قابل انتشار، offline-first و قابل توسعه |
| قالب | MiniJinja با strict undefined | سبک، ایمن و خطاپذیری واضح برای variable ناقص |
| ترکیب | Resolver بر پایهٔ dependency، compatibility، conflict و capability | افزوده‌شدن ecosystem بدون تغییر هسته |
| فایل | staging directory، preview، سپس commit با rollback best-effort | جلوگیری از پروژهٔ نیمه‌نوشته |
| JSON | merge عمیق و deterministic | پرهیز از string injection در package.json و tsconfig |
| command | فقط validation و post-generation با opt-in صریح `--run-commands` | secure-by-default |

## منابع

[1]: https://cargo-generate.github.io/cargo-generate/ "Cargo Generate Documentation"
[2]: https://yeoman.io/authoring/ "Writing Your Own Yeoman Generator"
[3]: https://github.com/jondot/hygen "Hygen repository and README"
