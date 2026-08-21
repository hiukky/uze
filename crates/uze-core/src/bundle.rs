//! Imported standard bundle representation.

use std::path::PathBuf;

use serde::Serialize;

use crate::capability::{CapabilityKind, Representation};

#[derive(Clone, Debug, Serialize)]
pub struct ImportedBundle {
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub importer: String,
    pub standard_items: Vec<BundleItem>,
    pub optional_enhancements: Vec<BundleItem>,
    pub compatibility_fallback: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct BundleItem {
    pub path: PathBuf,
    pub kind: CapabilityKind,
    pub representation: Representation,
    pub byte_len: usize,
}
