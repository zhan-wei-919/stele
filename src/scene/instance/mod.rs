//! Scene-layer render-ready primitives shared by composer and renderer.
//!
//! These types are pure data (`#[repr(C)] + Pod` where required) with no wgpu
//! dependency. They live here — and not under `renderer/` — because the
//! composer produces them while assembling a scene buffer, and the renderer
//! is downstream. Keeping the canonical definitions in `scene` pins the
//! dependency direction store/scene → renderer.

mod atlas_region;
mod glyph;
mod rect;

pub(crate) use atlas_region::AtlasRegion;
pub(crate) use glyph::GlyphInstance;
pub(crate) use rect::RectInstance;
