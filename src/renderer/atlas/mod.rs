//! Glyph atlas allocation and upload helpers.

pub mod glyph_atlas;
pub mod packer;
pub(crate) mod upload;

pub(crate) use crate::scene::instance::AtlasRegion;
pub(crate) use glyph_atlas::GlyphAtlas;
