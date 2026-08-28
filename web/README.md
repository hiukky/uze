# uze docs

Next.js + [Fumadocs](https://fumadocs.dev) site for [uze](https://github.com/hiukky/uze).

```bash
bun install
bun dev      # http://localhost:3000
bun build
```

Content lives in `content/docs/`. The harness compatibility matrix in
`content/docs/harnesses.mdx` is generated — see `cargo run --bin
uze-harness-matrix` at the repo root; do not hand-edit the marked block.

Deployed on Vercel (Root Directory: `web`) from the repo root.
