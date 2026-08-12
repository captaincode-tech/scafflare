# ADR-001: تولید تراکنشی با staging و اجرای command به‌صورت opt-in

**وضعیت:** پذیرفته‌شده  
**تاریخ:** ۱۲ اوت ۲۰۲۶

## زمینه

یک scaffolder نباید به‌دلیل template نامعتبر، conflict دیرهنگام، یا خطای دیسک، پروژه را در وضعیت ناقص رها کند. همچنین Recipe شخص ثالث نباید بتواند در زمان نصب، بدون اطلاع کاربر command خارجی اجرا کند.

## تصمیم

StackForge تمام renderها را ابتدا در یک staging directory تولید می‌کند. پیش از commit، preview نمایش داده می‌شود و در صورت نیاز به overwrite تأیید می‌گیرد. هنگام commit از backup موقت برای فایل‌های جایگزین‌شده استفاده می‌شود. شکست commit موجب rollback best-effort می‌شود و lockfile فقط پس از commit کامل نوشته می‌شود.

`validation_commands` و `post_generation_instructions` در Recipe نگه‌داری می‌شوند، اما commandها فقط با `--run-commands` و پس از نمایش کامل آرگومان‌ها اجرا می‌شوند. هیچ shell، interpolation یا command string از Recipe به‌طور خودکار اجرا نمی‌شود.

## پیامدها

این تصمیم زمان و فضای دیسک محدودی برای staging مصرف می‌کند، ولی writeهای قابل پیش‌بینی، preview درست و recovery قابل‌فهم به دست می‌دهد. اجرای command همچنان برای CI یا کاربران آگاه در دسترس است، اما سطح حملهٔ Recipeها را به‌طور معناداری کاهش می‌دهد.
