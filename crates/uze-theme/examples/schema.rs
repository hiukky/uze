//! Regenerates `themes/theme.schema.json` from the vocabulary.
//!
//! The schema is generated rather than written so it cannot drift from the
//! tokens and symbols it describes; this is the one command that rewrites
//! the checked-in copy, and the crate's own test fails until it has been run.

fn main() -> std::io::Result<()> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/themes/theme.schema.json");
    let mut json = serde_json::to_string_pretty(&uze_theme::json_schema())?;
    json.push('\n');
    std::fs::write(path, json)?;
    println!("wrote {path}");
    Ok(())
}
