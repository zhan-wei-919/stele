//! CPU-side glyph atlas indexing that accumulates patches for the view thread.

use std::collections::HashMap;

use log::info;

use crate::font::{FreeTypeRasterizer, GlyphKey, RasterizedGlyph};
use crate::io::{AtlasPatch, AtlasUpdate};
use crate::renderer::atlas::packer::ShelfPacker;
use crate::renderer::atlas::upload::AtlasUpload;
use crate::scene::instance::AtlasRegion;

const DEFAULT_ATLAS_SIZE: u32 = 2048;
const MAX_ATLAS_SIZE: u32 = 8192;

/// CPU-side atlas index that owns only logical allocation state and patch queues.
pub(crate) struct LogicalAtlas {
    pub(crate) current_size: u32,
    pub(crate) generation: u64,
    pub(crate) scale_factor_bits: u32,
    pub(crate) packer: ShelfPacker,
    pub(crate) regions: HashMap<GlyphKey, AtlasRegion>,
    pub(crate) pending_patches: Vec<AtlasPatch>,
    pub(crate) pending_requested_atlas_size: Option<u32>,
}

impl LogicalAtlas {
    /// Creates an empty logical atlas for the current scale factor.
    pub(crate) fn new(scale_factor: f32) -> Self {
        Self {
            current_size: DEFAULT_ATLAS_SIZE,
            generation: 0,
            scale_factor_bits: scale_factor.to_bits(),
            packer: ShelfPacker::new(DEFAULT_ATLAS_SIZE, DEFAULT_ATLAS_SIZE),
            regions: HashMap::new(),
            pending_patches: Vec::new(),
            pending_requested_atlas_size: None,
        }
    }

    /// Returns the atlas region for one glyph, rasterizing and indexing it if needed.
    pub(crate) fn get_or_insert(
        &mut self,
        key: GlyphKey,
        rasterizer: &FreeTypeRasterizer,
    ) -> AtlasRegion {
        if let Some(region) = self.regions.get(&key).copied() {
            return region;
        }

        let rasterized = rasterizer.rasterize_lcd(key);
        self.insert_rasterized(key, &rasterized, rasterizer)
    }

    /// Resets atlas allocation for a scale-factor change and requests a physical rebuild.
    pub(crate) fn reset_for_scale(&mut self, scale_factor: f32) {
        self.current_size = DEFAULT_ATLAS_SIZE;
        self.generation += 1;
        self.scale_factor_bits = scale_factor.to_bits();
        self.packer = ShelfPacker::new(self.current_size, self.current_size);
        self.regions.clear();
        self.pending_patches.clear();
        self.pending_requested_atlas_size = Some(self.current_size);
        info!(
            "atlas.reset size={} generation={} scale_factor_bits={}",
            self.current_size, self.generation, self.scale_factor_bits
        );
    }

    /// Drains one pending atlas update if the logical atlas changed since the last send.
    pub(crate) fn take_pending_update(&mut self) -> Option<AtlasUpdate> {
        let requested_atlas_size = self.pending_requested_atlas_size.take();
        let patches = std::mem::take(&mut self.pending_patches);
        if requested_atlas_size.is_none() && patches.is_empty() {
            return None;
        }

        let mut update = AtlasUpdate::new(self.generation);
        update.requested_atlas_size = requested_atlas_size;
        update.patches = patches;
        Some(update)
    }

    fn insert_rasterized(
        &mut self,
        key: GlyphKey,
        rasterized: &RasterizedGlyph,
        rasterizer: &FreeTypeRasterizer,
    ) -> AtlasRegion {
        let upload = AtlasUpload::from_rasterized(rasterized, rasterizer.subpixel_layout());
        if upload.width == 0 || upload.height == 0 {
            let region = empty_region(rasterized);
            self.regions.insert(key, region);
            return region;
        }

        loop {
            if let Some(origin) = self.packer.allocate(upload.width, upload.height) {
                let region = region_for(origin, &upload, self.current_size);
                self.regions.insert(key, region);
                self.pending_patches
                    .push(AtlasPatch::new(region, upload.rgba.clone()));
                return region;
            }

            self.grow_and_repack(rasterizer);
        }
    }

    fn grow_and_repack(&mut self, rasterizer: &FreeTypeRasterizer) {
        let keys = self.regions.keys().copied().collect::<Vec<_>>();
        let mut new_size = self.current_size.saturating_mul(2);

        loop {
            assert!(
                new_size <= MAX_ATLAS_SIZE,
                "logical atlas exceeded maximum size {}",
                MAX_ATLAS_SIZE
            );

            if let Some(repack) = build_repack(new_size, &keys, rasterizer) {
                self.current_size = new_size;
                self.generation += 1;
                self.packer = repack.packer;
                self.regions = repack.regions;
                self.pending_patches = repack.patches;
                self.pending_requested_atlas_size = Some(new_size);
                info!(
                    "atlas.repack old_size={} new_size={} generation={}",
                    new_size / 2,
                    new_size,
                    self.generation
                );
                return;
            }

            new_size = new_size.saturating_mul(2);
        }
    }
}

struct AtlasRepack {
    packer: ShelfPacker,
    regions: HashMap<GlyphKey, AtlasRegion>,
    patches: Vec<AtlasPatch>,
}

fn build_repack(
    atlas_size: u32,
    keys: &[GlyphKey],
    rasterizer: &FreeTypeRasterizer,
) -> Option<AtlasRepack> {
    let mut packer = ShelfPacker::new(atlas_size, atlas_size);
    let mut regions = HashMap::with_capacity(keys.len());
    let mut patches = Vec::with_capacity(keys.len());

    for key in keys {
        let rasterized = rasterizer.rasterize_lcd(*key);
        let upload = AtlasUpload::from_rasterized(&rasterized, rasterizer.subpixel_layout());
        let region = if upload.width == 0 || upload.height == 0 {
            empty_region(&rasterized)
        } else {
            let origin = packer.allocate(upload.width, upload.height)?;
            let region = region_for(origin, &upload, atlas_size);
            patches.push(AtlasPatch::new(region, upload.rgba));
            region
        };
        regions.insert(*key, region);
    }

    Some(AtlasRepack {
        packer,
        regions,
        patches,
    })
}

fn region_for(origin: (u32, u32), upload: &AtlasUpload, atlas_size: u32) -> AtlasRegion {
    let inv_size = 1.0 / atlas_size as f32;
    AtlasRegion {
        uv_min: [origin.0 as f32 * inv_size, origin.1 as f32 * inv_size],
        uv_max: [
            (origin.0 + upload.width) as f32 * inv_size,
            (origin.1 + upload.height) as f32 * inv_size,
        ],
        size: [upload.width as f32, upload.height as f32],
        bearing: upload.bearing,
    }
}

fn empty_region(rasterized: &RasterizedGlyph) -> AtlasRegion {
    AtlasRegion {
        uv_min: [0.0, 0.0],
        uv_max: [0.0, 0.0],
        size: [0.0, 0.0],
        bearing: [rasterized.bearing_x as f32, rasterized.bearing_y as f32],
    }
}
