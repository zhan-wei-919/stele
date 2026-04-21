#![allow(dead_code)]

mod draw_list {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct ClipRect;

    #[derive(Clone, Debug)]
    pub(crate) struct ImageCmd;

    #[derive(Clone, Debug)]
    pub(crate) struct PathCmd;
}

#[path = "../../src/scene/block.rs"]
mod scene_block;
pub(crate) use scene_block::{BlockDataArena, BlockId};
#[path = "../../src/scene/buffer.rs"]
mod scene_buffer_impl;
mod scene {
    pub(crate) use super::scene_buffer_impl::{SceneBuffer, SceneBufferInner, SceneFrameMetadata};

    pub(crate) mod instance {
        #[derive(Clone, Copy, Debug)]
        pub(crate) struct GlyphInstance;

        #[derive(Clone, Copy, Debug)]
        pub(crate) struct RectInstance;
    }
}

fn main() {
    let scene_buffer = scene::SceneBuffer::new(
        bumpalo::Bump::new(),
        |owner| scene::SceneBufferInner::empty_in(owner, scene::SceneFrameMetadata::default()),
    );
    let _clone = scene_buffer.clone();
}
