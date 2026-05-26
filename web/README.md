This is a [Next.js](https://nextjs.org) project bootstrapped with [`create-next-app`](https://nextjs.org/docs/app/api-reference/cli/create-next-app).

## Getting Started

First, run the development server:

```bash
npm run dev
# or
yarn dev
# or
pnpm dev
# or
bun dev
```

Open [http://localhost:3100](http://localhost:3100) with your browser to see the result.

You can start editing the page by modifying `app/page.tsx`. The page auto-updates as you edit the file.

This project uses [`next/font`](https://nextjs.org/docs/app/building-your-application/optimizing/fonts) to automatically optimize and load [Geist](https://vercel.com/font), a new font family for Vercel.

## Learn More

To learn more about Next.js, take a look at the following resources:

- [Next.js Documentation](https://nextjs.org/docs) - learn about Next.js features and API.
- [Learn Next.js](https://nextjs.org/learn) - an interactive Next.js tutorial.

You can check out [the Next.js GitHub repository](https://github.com/vercel/next.js) - your feedback and contributions are welcome!

## Deploy on Vercel

The easiest way to deploy your Next.js app is to use the [Vercel Platform](https://vercel.com/new?utm_medium=default-template&filter=next.js&utm_source=create-next-app&utm_campaign=create-next-app-readme) from the creators of Next.js.

Check out our [Next.js deployment documentation](https://nextjs.org/docs/app/building-your-application/deploying) for more details.

## Running full-stack (frontend + Rust backend)

1. Start Postgres + Mailpit (the dev mail catcher) and the API:
   ```bash
   cd backend
   docker compose up -d postgres mailpit
   DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo run -p recon-api -- seed
   RECON_DEV=1 DATABASE_URL=postgres://recon:recon@localhost:5432/recon \
     SMTP_HOST=localhost SMTP_PORT=1025 cargo run -p recon-api
   ```
2. Start the frontend against it:
   ```bash
   cd web
   echo 'NEXT_PUBLIC_API_BASE_URL=http://localhost:8080' > .env.local
   pnpm dev
   ```
3. Open http://localhost:3100 and sign in.

### Dev credentials (created by `seed`)

All passwords are `Password123!`:

| Email | Role | Tenants |
|-------|------|---------|
| `mia@acme.test` | operator (maker of the pending case) | Acme Capital |
| `theo@acme.test` | approver (checker for four-eyes) | Acme Capital |
| `ada@acme.test` | admin (user management) | Acme Capital **and** Globex (tenant switcher demo) |

Password-reset emails are caught by Mailpit — open its UI at **http://localhost:8025** to click the reset link.

> **Production note:** set a strong `JWT_SECRET` (the dev fallback is insecure and logs a warning) and `SECURE_COOKIE=1` so the refresh cookie is sent only over HTTPS.

### Ingesting bank/ledger files

1. Sign in (any role can ingest).
2. Go to **Sources** → **New source** (give it a name, kind, and currency).
3. Click **Upload** on the source row, choose **CSV** or **CAMT.053**, pick a file, and
   (for CSV) map the columns by 0-based index + choose how amounts are encoded
   (single signed column, or separate debit/credit columns). Bad rows reject the whole
   file with a per-row report; re-uploading an already-loaded statement is rejected as a
   duplicate.
4. Create a second source and upload its file.
5. Go to **Runs** → **New run**, pick the two sources + a date window, and **Create run**.
   You land on the run detail with matches and breaks.

Supported formats this slice: CSV (configurable mapping) and CAMT.053 (ISO 20022 XML).

### Running the E2E against the live stack
With Postgres up and `RECON_DEV=1 ... cargo run -p recon-api` serving on :8080:
```bash
pnpm -C web e2e
```
Playwright starts the web dev server automatically; each test reseeds the backend via `/api/dev/reseed`.
