//! Rectangle instance encoding shared between the CPU draw list and WGSL shaders.

use bytemuck::{Pod, Zeroable};

use crate::draw_list::RectCmd;

/// Per-instance data for a solid rectangle quad.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct RectInstance {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
}

impl RectInstance {
    /// Converts logical rectangle coordinates into physical pixels for the GPU.
    pub fn from_rect(cmd: RectCmd, scale_factor: f32) -> Self {
        let pos = cmd.pos();
        let size = cmd.size();
        Self {
            pos: [pos[0] * scale_factor, pos[1] * scale_factor],
            size: [size[0] * scale_factor, size[1] * scale_factor],
            color: cmd.color(),
        }
    }
}
