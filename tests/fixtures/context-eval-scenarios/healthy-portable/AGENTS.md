# billing-service

Rust workspace. Run `cargo test`, `cargo clippy -- -D warnings`, and
`cargo fmt --check` before committing.

<!-- uze:begin package:security-guidelines:instructions -->
- Never log a raw card number or CVV, even at debug level.
- All new endpoints require an integration test hitting a real (sandboxed)
  payment provider, not a mock.
<!-- uze:end package:security-guidelines:instructions -->
