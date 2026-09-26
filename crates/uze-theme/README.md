# uze-theme

The design vocabulary: colour `Token`s, named `Symbol`s, the theme file
schema, and the resolver that completes a partial theme from the built-in
default.

Name the meaning (`Token::TextMuted`, `Symbol::MarkOfficial`) and let the
active theme decide what it looks like. Nothing outside `src/ui/theme.rs` may
name a colour value or write a chrome glyph inline — two architecture tests
fail the build over it.

A leaf crate: it resolves no path, reads no environment and names no rendering
library, so every consumer adapts `uze_theme::Rgb` to what it draws with.

```bash
cargo test -p uze-theme
```

See [Appearance](../../web/content/docs/configuration/appearance.mdx) for the two axes.
