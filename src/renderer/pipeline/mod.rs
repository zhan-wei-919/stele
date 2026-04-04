//! Render-pipeline construction helpers for glyph and rectangle passes.

pub mod bind_group;
pub mod glyph;
pub mod rect;

pub(crate) use bind_group::{create_screen_size_bind_group, screen_uniform};
pub(crate) use glyph::create_glyph_pipeline;
pub(crate) use rect::create_rect_pipeline;
