//! GPU-side per-instance vertex layouts for renderer primitives.

pub mod glyph;
pub mod image;
pub mod path;
pub mod rect;

pub use glyph::{glyph_instance_layout, GlyphInstance};
pub use image::{image_instance_layout, ImageInstance};
pub use path::{path_vertex_layout, PathVertex};
pub use rect::{rect_instance_layout, RectInstance};
