//! Renderer-facing draw-list types plus the GPU runtime that consumes them.

pub(crate) mod atlas;
pub(crate) mod draw_list;
pub(crate) mod image_cache;
pub(crate) mod instance;
pub(crate) mod pipeline;
mod runtime;
pub(crate) mod subpixel;
pub(crate) mod tessellation;

pub(crate) use draw_list::{
    BlockDrawGroup, ClipRect, DrawListOp, PositionedGlyph, RectCmd, RenderLayer,
};
pub(crate) use runtime::Renderer;
