//! Low-level conversion from `PathCmd` verbs into lyon tessellation output.

use std::collections::HashMap;

use log::warn;

use lyon::math::point;
use lyon::path::Path;
use lyon::tessellation::geometry_builder::{BuffersBuilder, VertexBuffers};
use lyon::tessellation::{
    FillOptions, FillTessellator, FillVertex, FillVertexConstructor, LineCap as LyonLineCap,
    LineJoin as LyonLineJoin, StrokeOptions, StrokeTessellator, StrokeVertex,
    StrokeVertexConstructor,
};

use crate::renderer::draw_list::{LineCap, LineJoin, PathCmd, PathVerb};
use crate::renderer::instance::PathVertex;

use super::CachedMesh;

const AA_FRINGE_WIDTH: f32 = 1.0;
const PATH_TESSELLATION_TOLERANCE: f32 = 0.02;

/// Builds a cached mesh from the high-level path command.
pub(super) fn tessellate_path(cmd: &PathCmd, scale_factor: f32) -> CachedMesh {
    if cmd.verbs.is_empty() {
        return CachedMesh::default();
    }
    if !cmd.is_visible() {
        debug_assert!(false, "PathCmd fill and stroke cannot both be None");
        return CachedMesh::default();
    }

    let Some(path) = build_path(&cmd.verbs, scale_factor) else {
        return CachedMesh::default();
    };
    let fill_options = FillOptions::tolerance(PATH_TESSELLATION_TOLERANCE);
    let mut mesh = CachedMesh::default();

    if let Some(fill) = cmd.fill {
        if cmd.fill_is_valid() {
            let fill_mesh = tessellate_fill_geometry(cmd, &path, fill, &fill_options);
            append_aa_mesh(fill_mesh, &mut mesh);
        } else {
            debug_assert!(false, "PathCmd fill color must stay within [0, 1]");
        }
    }

    if let Some(stroke) = cmd.stroke {
        if stroke.is_valid() {
            let options = stroke_options(stroke, scale_factor);
            let stroke_mesh = tessellate_stroke_geometry(cmd, &path, stroke.color, &options);
            append_aa_mesh(stroke_mesh, &mut mesh);
        } else {
            debug_assert!(false, "StrokeStyle width must be positive and color valid");
        }
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

fn build_path(verbs: &[PathVerb], scale_factor: f32) -> Option<Path> {
    let mut builder = Path::builder();
    let mut has_current_point = false;

    for verb in verbs {
        match *verb {
            PathVerb::MoveTo { to } => {
                if has_current_point {
                    builder.end(false);
                }
                builder.begin(scale_point(to, scale_factor));
                has_current_point = true;
            }
            PathVerb::LineTo { to } => {
                if !has_current_point {
                    debug_assert!(false, "PathCmd LineTo requires a previous MoveTo");
                    return None;
                }
                builder.line_to(scale_point(to, scale_factor));
            }
            PathVerb::QuadTo { ctrl, to } => {
                if !has_current_point {
                    debug_assert!(false, "PathCmd QuadTo requires a previous MoveTo");
                    return None;
                }
                builder.quadratic_bezier_to(
                    scale_point(ctrl, scale_factor),
                    scale_point(to, scale_factor),
                );
            }
            PathVerb::CubicTo { ctrl1, ctrl2, to } => {
                if !has_current_point {
                    debug_assert!(false, "PathCmd CubicTo requires a previous MoveTo");
                    return None;
                }
                builder.cubic_bezier_to(
                    scale_point(ctrl1, scale_factor),
                    scale_point(ctrl2, scale_factor),
                    scale_point(to, scale_factor),
                );
            }
            PathVerb::Close => {
                if !has_current_point {
                    debug_assert!(false, "PathCmd Close requires an open subpath");
                    return None;
                }
                builder.close();
                has_current_point = false;
            }
        }
    }

    if has_current_point {
        builder.end(false);
    }

    Some(builder.build())
}

fn stroke_options(
    stroke: crate::renderer::draw_list::StrokeStyle,
    scale_factor: f32,
) -> StrokeOptions {
    let line_cap = lyon_line_cap(stroke.line_cap);
    StrokeOptions::tolerance(PATH_TESSELLATION_TOLERANCE)
        .with_line_width(stroke.width * scale_factor)
        .with_start_cap(line_cap)
        .with_end_cap(line_cap)
        .with_line_join(lyon_line_join(stroke.line_join))
}

fn scale_point(point_xy: [f32; 2], scale_factor: f32) -> lyon::math::Point {
    point(point_xy[0] * scale_factor, point_xy[1] * scale_factor)
}

fn lyon_line_cap(line_cap: LineCap) -> LyonLineCap {
    match line_cap {
        LineCap::Butt => LyonLineCap::Butt,
        LineCap::Round => LyonLineCap::Round,
        LineCap::Square => LyonLineCap::Square,
    }
}

fn lyon_line_join(line_join: LineJoin) -> LyonLineJoin {
    match line_join {
        LineJoin::Miter => LyonLineJoin::Miter,
        LineJoin::Round => LyonLineJoin::Round,
        LineJoin::Bevel => LyonLineJoin::Bevel,
    }
}

fn log_tessellation_error(stage: &str, cmd: &PathCmd, error: &impl std::fmt::Display) {
    warn!(
        "path.tessellate_failed stage={} layer={:?} verb_count={} fill={} stroke={} error={}",
        stage,
        cmd.layer,
        cmd.verbs.len(),
        cmd.fill.is_some(),
        cmd.stroke.is_some(),
        error,
    );
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

fn build_boundary_fringe(vertices: &[PathVertex], indices: &[u32]) -> CachedMesh {
    let boundary_edges = collect_boundary_edges(vertices, indices);
    if boundary_edges.is_empty() {
        return CachedMesh::default();
    }

    let mut normal_sums = vec![[0.0, 0.0]; vertices.len()];
    let mut fallback_normals = vec![[0.0, 0.0]; vertices.len()];
    let mut boundary_vertex_mask = vec![false; vertices.len()];

    for edge in &boundary_edges {
        let from = vertices[edge.from as usize].position;
        let to = vertices[edge.to as usize].position;
        let edge_length = vector_length(subtract(to, from));
        if edge_length <= f32::EPSILON {
            continue;
        }

        let weighted_normal = scale(edge.normal, edge_length);
        accumulate_normal(
            edge.from as usize,
            weighted_normal,
            edge.normal,
            &mut normal_sums,
            &mut fallback_normals,
            &mut boundary_vertex_mask,
        );
        accumulate_normal(
            edge.to as usize,
            weighted_normal,
            edge.normal,
            &mut normal_sums,
            &mut fallback_normals,
            &mut boundary_vertex_mask,
        );
    }

    let mut outer_indices = vec![None; vertices.len()];
    let mut fringe_vertices = Vec::new();

    for (index, is_boundary_vertex) in boundary_vertex_mask.iter().copied().enumerate() {
        if !is_boundary_vertex {
            continue;
        }

        let outward_normal = normalized(normal_sums[index])
            .or_else(|| normalized(fallback_normals[index]))
            .unwrap_or([0.0, 0.0]);
        if vector_length(outward_normal) <= f32::EPSILON {
            continue;
        }

        let inner_vertex = vertices[index];
        let outer_index = vertices.len() as u32 + fringe_vertices.len() as u32;
        outer_indices[index] = Some(outer_index);
        fringe_vertices.push(PathVertex {
            position: add(
                inner_vertex.position,
                scale(outward_normal, AA_FRINGE_WIDTH),
            ),
            color: inner_vertex.color,
            coverage: 0.0,
        });
    }

    let mut fringe_indices = Vec::with_capacity(boundary_edges.len() * 6);
    for edge in boundary_edges {
        let Some(outer_from) = outer_indices[edge.from as usize] else {
            continue;
        };
        let Some(outer_to) = outer_indices[edge.to as usize] else {
            continue;
        };

        fringe_indices.extend_from_slice(&[
            edge.from, edge.to, outer_to, edge.from, outer_to, outer_from,
        ]);
    }

    CachedMesh {
        vertices: fringe_vertices,
        indices: fringe_indices,
        last_used: 0,
    }
}

fn collect_boundary_edges(vertices: &[PathVertex], indices: &[u32]) -> Vec<BoundaryEdge> {
    let mut edges = HashMap::<(u32, u32), DirectedEdge>::new();

    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [triangle[0], triangle[1], triangle[2]];
        if !valid_triangle_indices(vertices, [a, b, c]) {
            debug_assert!(
                false,
                "path fringe generation received an out-of-range index"
            );
            continue;
        }

        for &(from, to, third) in &[(a, b, c), (b, c, a), (c, a, b)] {
            if from == to || from == third || to == third {
                continue;
            }

            let key = edge_key(from, to);
            edges
                .entry(key)
                .and_modify(|edge| edge.occurrences = edge.occurrences.saturating_add(1))
                .or_insert(DirectedEdge {
                    from,
                    to,
                    third,
                    occurrences: 1,
                });
        }
    }

    let mut boundary_edges = Vec::new();
    for edge in edges.into_values() {
        if edge.occurrences != 1 {
            continue;
        }

        let Some(normal) = outward_boundary_normal(vertices, edge.from, edge.to, edge.third) else {
            continue;
        };
        boundary_edges.push(BoundaryEdge {
            from: edge.from,
            to: edge.to,
            normal,
        });
    }

    boundary_edges
}

fn valid_triangle_indices(vertices: &[PathVertex], triangle: [u32; 3]) -> bool {
    triangle
        .into_iter()
        .all(|index| (index as usize) < vertices.len())
}

fn edge_key(a: u32, b: u32) -> (u32, u32) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

fn outward_boundary_normal(
    vertices: &[PathVertex],
    from: u32,
    to: u32,
    third: u32,
) -> Option<[f32; 2]> {
    let from_position = vertices[from as usize].position;
    let to_position = vertices[to as usize].position;
    let third_position = vertices[third as usize].position;

    let edge = subtract(to_position, from_position);
    let left_normal = normalized([-edge[1], edge[0]])?;
    let to_third = subtract(third_position, from_position);
    let interior_is_left = dot(to_third, left_normal) > 0.0;
    Some(if interior_is_left {
        scale(left_normal, -1.0)
    } else {
        left_normal
    })
}

fn accumulate_normal(
    index: usize,
    weighted_normal: [f32; 2],
    fallback_normal: [f32; 2],
    normal_sums: &mut [[f32; 2]],
    fallback_normals: &mut [[f32; 2]],
    boundary_vertex_mask: &mut [bool],
) {
    normal_sums[index] = add(normal_sums[index], weighted_normal);
    fallback_normals[index] = add(fallback_normals[index], fallback_normal);
    boundary_vertex_mask[index] = true;
}

fn add(lhs: [f32; 2], rhs: [f32; 2]) -> [f32; 2] {
    [lhs[0] + rhs[0], lhs[1] + rhs[1]]
}

fn subtract(lhs: [f32; 2], rhs: [f32; 2]) -> [f32; 2] {
    [lhs[0] - rhs[0], lhs[1] - rhs[1]]
}

fn scale(vector: [f32; 2], scalar: f32) -> [f32; 2] {
    [vector[0] * scalar, vector[1] * scalar]
}

fn dot(lhs: [f32; 2], rhs: [f32; 2]) -> f32 {
    lhs[0] * rhs[0] + lhs[1] * rhs[1]
}

fn vector_length(vector: [f32; 2]) -> f32 {
    dot(vector, vector).sqrt()
}

fn normalized(vector: [f32; 2]) -> Option<[f32; 2]> {
    let length = vector_length(vector);
    if length <= f32::EPSILON {
        None
    } else {
        Some(scale(vector, 1.0 / length))
    }
}

#[derive(Clone, Copy, Debug)]
struct DirectedEdge {
    from: u32,
    to: u32,
    third: u32,
    occurrences: u8,
}

#[derive(Clone, Copy, Debug)]
struct BoundaryEdge {
    from: u32,
    to: u32,
    normal: [f32; 2],
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

#[cfg(test)]
mod tests {
    use super::{build_boundary_fringe, tessellate_path};
    use crate::renderer::draw_list::{
        LineCap, LineJoin, PathCmd, PathVerb, RenderLayer, StrokeStyle,
    };
    use crate::renderer::instance::PathVertex;

    #[test]
    fn build_boundary_fringe_emits_zero_coverage_outer_vertices() {
        let vertices = vec![
            PathVertex {
                position: [0.0, 0.0],
                color: [1.0, 0.0, 0.0, 1.0],
                coverage: 1.0,
            },
            PathVertex {
                position: [10.0, 0.0],
                color: [1.0, 0.0, 0.0, 1.0],
                coverage: 1.0,
            },
            PathVertex {
                position: [0.0, 10.0],
                color: [1.0, 0.0, 0.0, 1.0],
                coverage: 1.0,
            },
        ];
        let fringe = build_boundary_fringe(&vertices, &[0, 1, 2]);

        assert_eq!(fringe.vertices.len(), 3);
        assert_eq!(fringe.indices.len(), 18);
        assert!(fringe.vertices.iter().all(|vertex| vertex.coverage == 0.0));
    }

    #[test]
    fn tessellate_path_adds_fringe_vertices_for_fill_and_stroke_geometry() {
        let cmd = PathCmd {
            verbs: vec![
                PathVerb::MoveTo { to: [0.0, 0.0] },
                PathVerb::LineTo { to: [20.0, 0.0] },
                PathVerb::LineTo { to: [10.0, 20.0] },
                PathVerb::Close,
            ],
            fill: Some([0.1, 0.2, 0.3, 1.0]),
            stroke: Some(StrokeStyle {
                color: [0.9, 0.8, 0.7, 1.0],
                width: 2.0,
                line_cap: LineCap::Butt,
                line_join: LineJoin::Miter,
            }),
            layer: RenderLayer::Content,
        };

        let mesh = tessellate_path(&cmd, 1.0);

        assert!(!mesh.indices.is_empty());
        assert!(mesh.vertices.iter().any(|vertex| vertex.coverage == 1.0));
        assert!(mesh.vertices.iter().any(|vertex| vertex.coverage == 0.0));
    }
}
