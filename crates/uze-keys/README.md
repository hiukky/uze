# uze-keys

The input vocabulary — to the keyboard what `uze-theme` is to the palette.
The product names what a gesture *means* (`Action`), says where that meaning
is live (`Scope`), and keeps which key reaches it a separate, changeable
question (`Chord`).

```text
a keystroke ──▶ Chord ──▶ Keymap::resolve(chord, scopes) ──▶ Action
a surface   ──▶ Action ─▶ Keymap::chord_for(action, scopes) ─▶ Chord
```

`Keymap` is the only thing allowed to answer either direction. The crate names
no terminal library: one adapter in the consumer turns real key events into
chords.

```bash
cargo test -p uze-keys
```

See [Shortcuts](../../web/content/docs/keys.mdx) for the authoring guide.
