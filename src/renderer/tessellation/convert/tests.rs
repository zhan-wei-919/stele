//! Focused tests for path tessellation lowering and fringe generation.

use super::fringe::build_boundary_fringe;
use super::tessellate::tessellate_path;
use crate::draw_list::{LineCap, LineJoin, PathCmd, PathVerb, RenderLayer, StrokeStyle};
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
    let cmd = PathCmd::new(
        vec![
            PathVerb::MoveTo { to: [0.0, 0.0] },
            PathVerb::LineTo { to: [20.0, 0.0] },
            PathVerb::LineTo { to: [10.0, 20.0] },
            PathVerb::Close,
        ],
        Some([0.1, 0.2, 0.3, 1.0]),
        Some(StrokeStyle::new(
            [0.9, 0.8, 0.7, 1.0],
            2.0,
            LineCap::Butt,
            LineJoin::Miter,
        )),
        RenderLayer::Content,
    );

    let mesh = tessellate_path(&cmd, 1.0);

    assert!(!mesh.indices.is_empty());
    assert!(mesh.vertices.iter().any(|vertex| vertex.coverage == 1.0));
    assert!(mesh.vertices.iter().any(|vertex| vertex.coverage == 0.0));
}
