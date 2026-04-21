//! GPU-side per-instance vertex layouts for renderer primitives.
//!
//! `GlyphInstance` and `RectInstance` are produced upstream in the scene layer
//! (see `scene::instance`). This module owns only the wgpu vertex-buffer
//! layout helpers plus the renderer-internal `ImageInstance` / `PathVertex`
//! primitives. The struct re-exports here are a facade so renderer internals
//! keep importing `crate::renderer::instance::GlyphInstance` unchanged.

pub mod glyph;
pub mod image;
pub mod path;
pub mod rect;

pub use glyph::glyph_instance_layout;
pub use image::{image_instance_layout, ImageInstance};
pub use path::{path_vertex_layout, PathVertex};
pub use rect::rect_instance_layout;

pub(crate) use crate::scene::instance::{GlyphInstance, RectInstance};
