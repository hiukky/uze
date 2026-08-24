# invoice-service

Use pnpm, not npm or yarn.

Run `pnpm test` before finishing any change.

Use Claude subagents for code review before opening a PR.

Prefer `zod` schemas for request validation; every route handler under
`src/routes/` should validate its body before touching the database.
