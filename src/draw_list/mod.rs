//! Shared high-level scene primitives used by layout, store, and renderer.

mod clip;
mod glyph;
mod image;
mod layer;
mod path;
mod rect;
mod validation;

#[cfg(test)]
mod tests;

pub(crate) use clip::ClipRect;
pub(crate) use glyph::PositionedGlyph;
pub(crate) use image::ImageCmd;
pub use image::ImageData;
pub(crate) use layer::RenderLayer;
pub use path::{LineCap, LineJoin, PathVerb};
pub(crate) use path::{PathCmd, StrokeStyle};
pub(crate) use rect::RectCmd;
