//! Focused tests for path tessellation lowering and fringe generation.

use std::hint::black_box;
use std::time::Instant;

use super::fringe::boundary_edges_for_test;
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

#[test]
fn boundary_edge_collection_matches_expected_triangle_edges() {
    let (vertices, indices) = single_triangle_mesh();
    let edges = boundary_edges_for_test(&vertices, &indices);

    assert_eq!(
        edges.into_iter().map(|edge| (edge.from, edge.to)).collect::<Vec<_>>(),
        vec![(0, 1), (1, 2), (2, 0)]
    );
}

#[test]
fn boundary_edge_collection_returns_grid_perimeter_edges() {
    let (vertices, indices) = grid_mesh(16, 16);
    let edges = boundary_edges_for_test(&vertices, &indices);

    assert_eq!(edges.len(), 64);
}

#[test]
fn boundary_edge_collection_returns_outer_ring_for_triangle_fan() {
    let (vertices, indices) = fan_mesh(256);
    let edges = boundary_edges_for_test(&vertices, &indices);

    assert_eq!(edges.len(), 256);
}

#[test]
fn boundary_edge_collection_keeps_all_edges_for_disconnected_triangles() {
    let (vertices, indices) = disconnected_triangles_mesh(512);
    let edges = boundary_edges_for_test(&vertices, &indices);

    assert_eq!(edges.len(), 512 * 3);
}

#[test]
#[ignore = "manual perf smoke test"]
fn reports_boundary_fringe_perf_profiles() {
    for case in benchmark_cases() {
        let elapsed = measure_case(case.iterations, &case.vertices, &case.indices);

        println!(
            "perf.fringe case={} vertices={} indices={} iterations={} total_us={} avg_ns={}",
            case.name,
            case.vertices.len(),
            case.indices.len(),
            case.iterations,
            elapsed.as_micros(),
            elapsed.as_nanos() / case.iterations as u128,
        );
    }
}

struct BenchmarkCase {
    name: &'static str,
    vertices: Vec<PathVertex>,
    indices: Vec<u32>,
    iterations: usize,
}

fn benchmark_cases() -> Vec<BenchmarkCase> {
    let (single_vertices, single_indices) = single_triangle_mesh();
    let (fan_vertices, fan_indices) = fan_mesh(4096);
    let (grid_medium_vertices, grid_medium_indices) = grid_mesh(32, 32);
    let (grid_large_vertices, grid_large_indices) = grid_mesh(128, 128);
    let (disconnected_vertices, disconnected_indices) = disconnected_triangles_mesh(4096);

    vec![
        BenchmarkCase {
            name: "single_triangle",
            vertices: single_vertices,
            indices: single_indices,
            iterations: 200_000,
        },
        BenchmarkCase {
            name: "fan_4096",
            vertices: fan_vertices,
            indices: fan_indices,
            iterations: 1_000,
        },
        BenchmarkCase {
            name: "grid_32x32",
            vertices: grid_medium_vertices,
            indices: grid_medium_indices,
            iterations: 1_000,
        },
        BenchmarkCase {
            name: "grid_128x128",
            vertices: grid_large_vertices,
            indices: grid_large_indices,
            iterations: 100,
        },
        BenchmarkCase {
            name: "disconnected_4096",
            vertices: disconnected_vertices,
            indices: disconnected_indices,
            iterations: 400,
        },
    ]
}

fn measure_case(
    iterations: usize,
    vertices: &[PathVertex],
    indices: &[u32],
) -> std::time::Duration {
    for _ in 0..10 {
        black_box(build_boundary_fringe(vertices, indices));
    }

    let started = Instant::now();
    for _ in 0..iterations {
        black_box(build_boundary_fringe(vertices, indices));
    }
    started.elapsed()
}

fn single_triangle_mesh() -> (Vec<PathVertex>, Vec<u32>) {
    (
        vec![
            sample_vertex([0.0, 0.0]),
            sample_vertex([10.0, 0.0]),
            sample_vertex([0.0, 10.0]),
        ],
        vec![0, 1, 2],
    )
}

fn grid_mesh(width: usize, height: usize) -> (Vec<PathVertex>, Vec<u32>) {
    let mut vertices = Vec::with_capacity((width + 1) * (height + 1));
    for y in 0..=height {
        for x in 0..=width {
            vertices.push(sample_vertex([x as f32, y as f32]));
        }
    }

    let row_stride = width + 1;
    let mut indices = Vec::with_capacity(width * height * 6);
    for y in 0..height {
        for x in 0..width {
            let top_left = (y * row_stride + x) as u32;
            let top_right = top_left + 1;
            let bottom_left = top_left + row_stride as u32;
            let bottom_right = bottom_left + 1;
            indices.extend_from_slice(&[
                top_left,
                top_right,
                bottom_right,
                top_left,
                bottom_right,
                bottom_left,
            ]);
        }
    }

    (vertices, indices)
}

fn fan_mesh(spokes: usize) -> (Vec<PathVertex>, Vec<u32>) {
    let mut vertices = Vec::with_capacity(spokes + 1);
    vertices.push(sample_vertex([0.0, 0.0]));
    for index in 0..spokes {
        let angle = index as f32 / spokes as f32 * std::f32::consts::TAU;
        vertices.push(sample_vertex([angle.cos() * 100.0, angle.sin() * 100.0]));
    }

    let mut indices = Vec::with_capacity(spokes * 3);
    for index in 1..spokes {
        indices.extend_from_slice(&[0, index as u32, index as u32 + 1]);
    }
    indices.extend_from_slice(&[0, spokes as u32, 1]);

    (vertices, indices)
}

fn disconnected_triangles_mesh(triangle_count: usize) -> (Vec<PathVertex>, Vec<u32>) {
    let mut vertices = Vec::with_capacity(triangle_count * 3);
    let mut indices = Vec::with_capacity(triangle_count * 3);
    for triangle_index in 0..triangle_count {
        let base_index = vertices.len() as u32;
        let x = triangle_index as f32 * 2.0;
        vertices.push(sample_vertex([x, 0.0]));
        vertices.push(sample_vertex([x + 1.0, 0.0]));
        vertices.push(sample_vertex([x, 1.0]));
        indices.extend_from_slice(&[base_index, base_index + 1, base_index + 2]);
    }

    (vertices, indices)
}

fn sample_vertex(position: [f32; 2]) -> PathVertex {
    PathVertex {
        position,
        color: [1.0, 1.0, 1.0, 1.0],
        coverage: 1.0,
    }
}
