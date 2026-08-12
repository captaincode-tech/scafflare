# stackforge-generated-clean

Backend TypeScript تولیدشده با StackForge، با معماری **Clean**.

## ساختار Todo

در انتخاب SQLite، مسیر کامل `Route → Controller → Service → Repository Interface → Drizzle Repository → Database` برای Todo تولید می‌شود. اعتبارسنجی ورودی‌ها با Zod انجام می‌شود و خطاهای typed به status code مناسب HTTP نگاشت می‌شوند.

## شروع سریع

```bash
npm install
npm run db:push
npm run dev
```

## API

| روش | مسیر | نتیجه |
|---|---|---|
| `POST` | `/todos` | ساخت Todo با `title` |
| `GET` | `/todos` | فهرست Todoها |
| `GET` | `/todos/:id` | دریافت یک Todo |
| `PATCH` | `/todos/:id` | به‌روزرسانی Todo |
| `DELETE` | `/todos/:id` | حذف Todo |
| `GET` | `/health` | سلامت سرویس |