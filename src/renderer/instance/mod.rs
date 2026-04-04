//! GPU-side per-instance vertex layouts for renderer primitives.

pub mod glyph;
pub mod rect;

pub use glyph::{glyph_instance_layout, GlyphInstance};
pub use rect::{rect_instance_layout, RectInstance};
