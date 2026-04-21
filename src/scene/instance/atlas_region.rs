//! Pure-data description of one glyph patch inside the atlas texture.

/// UVs, pixel size, and bearing for a glyph cached in the atlas.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AtlasRegion {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub size: [f32; 2],
    pub bearing: [f32; 2],
}
