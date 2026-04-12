//! lyon tessellation entrypoints that combine main geometry with AA fringe meshes.

use log::warn;

use lyon::path::Path;
use lyon::tessellation::geometry_builder::{BuffersBuilder, VertexBuffers};
use lyon::tessellation::{
    FillOptions, FillTessellator, FillVertex, FillVertexConstructor, StrokeOptions,
    StrokeTessellator, StrokeVertex, StrokeVertexConstructor,
};

use super::fringe::build_boundary_fringe;
use super::lower::{build_path, stroke_options, PATH_TESSELLATION_TOLERANCE};
use crate::draw_list::PathCmd;
use crate::renderer::instance::PathVertex;
use crate::renderer::tessellation::CachedMesh;

pub(in crate::renderer::tessellation) fn tessellate_path(
    cmd: &PathCmd,
    scale_factor: f32,
) -> CachedMesh {
    if cmd.verbs().is_empty() {
        return CachedMesh::default();
    }

    let Some(path) = build_path(cmd.verbs(), scale_factor) else {
        return CachedMesh::default();
    };
    let fill_options = FillOptions::tolerance(PATH_TESSELLATION_TOLERANCE);
    let mut mesh = CachedMesh::default();

    if let Some(fill) = cmd.fill() {
        let fill_mesh = tessellate_fill_geometry(cmd, &path, fill, &fill_options);
        append_aa_mesh(fill_mesh, &mut mesh);
    }

    if let Some(stroke) = cmd.stroke() {
        let options = stroke_options(stroke, scale_factor);
        let stroke_mesh = tessellate_stroke_geometry(cmd, &path, stroke.color(), &options);
        append_aa_mesh(stroke_mesh, &mut mesh);
    }

    mesh
}

fn tessellate_fill_geometry(
    cmd: &PathCmd,
    path: &Path,
    fill: [f32; 4],
    options: &FillOptions,
) -> VertexBuffers<PathVertex, u32> {
    let mut buffers = VertexBuffers::<PathVertex, u32>::new();
    let mut tessellator = FillTessellator::new();
    let mut builder = BuffersBuilder::new(
        &mut buffers,
        SolidColorConstructor {
            color: fill,
            coverage: 1.0,
        },
    );
    if let Err(error) = tessellator.tessellate_path(path, options, &mut builder) {
        log_tessellation_error("fill", cmd, &error);
    }
    buffers
}

fn tessellate_stroke_geometry(
    cmd: &PathCmd,
    path: &Path,
    color: [f32; 4],
    options: &StrokeOptions,
) -> VertexBuffers<PathVertex, u32> {
    let mut buffers = VertexBuffers::<PathVertex, u32>::new();
    let mut tessellator = StrokeTessellator::new();
    let mut builder = BuffersBuilder::new(
        &mut buffers,
        SolidColorConstructor {
            color,
            coverage: 1.0,
        },
    );
    if let Err(error) = tessellator.tessellate_path(path, options, &mut builder) {
        log_tessellation_error("stroke", cmd, &error);
    }
    buffers
}

fn append_aa_mesh(main_mesh: VertexBuffers<PathVertex, u32>, output: &mut CachedMesh) {
    if main_mesh.vertices.is_empty() || main_mesh.indices.is_empty() {
        return;
    }

    let vertex_offset = output.vertices.len() as u32;
    output.vertices.extend_from_slice(&main_mesh.vertices);
    output
        .indices
        .extend(main_mesh.indices.iter().map(|index| index + vertex_offset));

    let fringe = build_boundary_fringe(&main_mesh.vertices, &main_mesh.indices);
    if fringe.vertices.is_empty() || fringe.indices.is_empty() {
        return;
    }

    output.vertices.extend(fringe.vertices);
    output.indices.extend(
        fringe
            .indices
            .into_iter()
            .map(|index| index + vertex_offset),
    );
}

fn log_tessellation_error(stage: &str, cmd: &PathCmd, error: &impl std::fmt::Display) {
    warn!(
        "path.tessellate_failed stage={} layer={:?} verb_count={} fill={} stroke={} error={}",
        stage,
        cmd.layer(),
        cmd.verbs().len(),
        cmd.fill().is_some(),
        cmd.stroke().is_some(),
        error,
    );
}

struct SolidColorConstructor {
    color: [f32; 4],
    coverage: f32,
}

impl FillVertexConstructor<PathVertex> for SolidColorConstructor {
    fn new_vertex(&mut self, vertex: FillVertex<'_>) -> PathVertex {
        PathVertex {
            position: vertex.position().to_array(),
            color: self.color,
            coverage: self.coverage,
        }
    }
}

impl StrokeVertexConstructor<PathVertex> for SolidColorConstructor {
    fn new_vertex(&mut self, vertex: StrokeVertex<'_, '_>) -> PathVertex {
        PathVertex {
            position: vertex.position().to_array(),
            color: self.color,
            coverage: self.coverage,
        }
    }
}
