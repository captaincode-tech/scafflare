# Changelog

تمام تغییرهای مهم این پروژه در این فایل ثبت می‌شوند.

## [0.1.0] - 2026-08-12

### Added

- CLI مستقل از زبان با دستورهای `init`، `add`، `remove`، `list`، `doctor`، `validate` و `recipe validate`.
- parser و validator YAML Recipe با کنترل path traversal، dependency cycle، conflict، compatibility و capability.
- رندر MiniJinja با undefined strict، conditionهای declarative، merge عمیق JSON و lockfile نسخه‌دار.
- preview، commit staging-based، rollback best-effort و محافظت از overwrite.
- ۱۵ Recipe رسمی Node.js/TypeScript، Express، Hono، معماری‌ها، Drizzle/LibSQL، Zod، Pino، Vitest، Biome، Husky و GitHub Actions.
- Todo CRUD واقعی برای Clean + SQLite، شامل validation، خطاهای typed، unit test و integration test.
- quality gates Rust و workflowهای چندسکویی.
