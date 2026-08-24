# widget-cli

A small command-line tool for managing widgets.

## Development

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

Widgets are validated against the schema in `schema/widget.json` before
being written to disk.
