use std::fs;
use std::path::PathBuf;

use rs_typing_trainer::glossary::{self, generate_docs};
use rs_typing_trainer::i18n::Lang;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = glossary::bundled()?;
    let docs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs");
    fs::write(docs.join("glossary.md"), generate_docs(catalog, Lang::En))?;
    fs::write(docs.join("glossary.ja.md"), generate_docs(catalog, Lang::Ja))?;
    Ok(())
}
