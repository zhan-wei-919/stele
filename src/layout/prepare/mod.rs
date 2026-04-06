//! Prepare-stage measurement that turns spans into FreeType-free layout data.

mod measure;
mod stage;
mod types;

pub(crate) use measure::prepare_document;
pub(crate) use types::{PreparedBlock, PreparedGlyph, PreparedItem};
