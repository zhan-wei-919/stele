//! Boundary-edge analysis and fringe geometry generation for path vertex AA.

use crate::renderer::instance::PathVertex;
use crate::renderer::tessellation::CachedMesh;

const AA_FRINGE_WIDTH: f32 = 1.0;

pub(super) fn build_boundary_fringe(vertices: &[PathVertex], indices: &[u32]) -> CachedMesh {
    let boundary_edges = collect_boundary_edges(vertices, indices);
    build_boundary_fringe_from_edges(vertices, boundary_edges)
}

fn build_boundary_fringe_from_edges(
    vertices: &[PathVertex],
    boundary_edges: Vec<BoundaryEdge>,
) -> CachedMesh {
    if boundary_edges.is_empty() {
        return CachedMesh::default();
    }

    let normals = accumulate_boundary_normals(vertices, &boundary_edges);
    let (fringe_vertices, outer_indices) = build_outer_vertices(vertices, &normals);

    CachedMesh {
        vertices: fringe_vertices,
        indices: build_fringe_indices(&boundary_edges, &outer_indices),
        last_used: 0,
    }
}

fn accumulate_boundary_normals(
    vertices: &[PathVertex],
    boundary_edges: &[BoundaryEdge],
) -> BoundaryNormals {
    let mut normals = BoundaryNormals::new(vertices.len());

    for edge in boundary_edges {
        let from = vertices[edge.from as usize].position;
        let to = vertices[edge.to as usize].position;
        let edge_length = vector_length(subtract(to, from));
        if edge_length <= f32::EPSILON {
            continue;
        }

        let weighted_normal = scale(edge.normal, edge_length);
        normals.accumulate(edge.from as usize, weighted_normal, edge.normal);
        normals.accumulate(edge.to as usize, weighted_normal, edge.normal);
    }

    normals
}

fn build_outer_vertices(
    vertices: &[PathVertex],
    normals: &BoundaryNormals,
) -> (Vec<PathVertex>, Vec<Option<u32>>) {
    let mut outer_indices = vec![None; vertices.len()];
    let mut fringe_vertices = Vec::new();

    for (index, is_boundary_vertex) in normals.boundary_vertex_mask.iter().copied().enumerate() {
        if !is_boundary_vertex {
            continue;
        }

        let outward_normal = vertex_outward_normal(normals, index);
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

    (fringe_vertices, outer_indices)
}

fn build_fringe_indices(
    boundary_edges: &[BoundaryEdge],
    outer_indices: &[Option<u32>],
) -> Vec<u32> {
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

    fringe_indices
}

fn vertex_outward_normal(normals: &BoundaryNormals, index: usize) -> [f32; 2] {
    normalized(normals.normal_sums[index])
        .or_else(|| normalized(normals.fallback_normals[index]))
        .unwrap_or([0.0, 0.0])
}

fn collect_boundary_edges(vertices: &[PathVertex], indices: &[u32]) -> Vec<BoundaryEdge> {
    let mut edge_records = collect_edge_records(vertices, indices);
    // We intentionally prefer a sorted Vec over the theoretically better O(T) hash path here.
    // Fringe extraction is a one-shot batch during tessellation cache misses, not an online
    // lookup-heavy workload, so contiguous records plus sort/scan win on cache locality and
    // avoid repeated hash/probe overhead in our measured release benchmarks.
    edge_records.sort_unstable_by_key(|record| record.key);
    boundary_edges_from_sorted_records(vertices, &edge_records)
}

fn valid_triangle_indices(vertices: &[PathVertex], triangle: [u32; 3]) -> bool {
    triangle
        .into_iter()
        .all(|index| (index as usize) < vertices.len())
}

fn packed_edge_key(a: u32, b: u32) -> u64 {
    let min = a.min(b);
    let max = a.max(b);
    ((min as u64) << 32) | max as u64
}

fn collect_edge_records(vertices: &[PathVertex], indices: &[u32]) -> Vec<EdgeRecord> {
    let mut edge_records = Vec::with_capacity(indices.len());

    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [triangle[0], triangle[1], triangle[2]];
        if !valid_triangle_indices(vertices, [a, b, c]) {
            debug_assert!(
                false,
                "path fringe generation received an out-of-range index"
            );
            continue;
        }
        append_triangle_edges(&mut edge_records, a, b, c);
    }

    edge_records
}

fn append_triangle_edges(edge_records: &mut Vec<EdgeRecord>, a: u32, b: u32, c: u32) {
    for &(from, to, third) in &[(a, b, c), (b, c, a), (c, a, b)] {
        if from == to || from == third || to == third {
            continue;
        }
        edge_records.push(EdgeRecord {
            key: packed_edge_key(from, to),
            from,
            to,
            third,
        });
    }
}

fn boundary_edges_from_sorted_records(
    vertices: &[PathVertex],
    edge_records: &[EdgeRecord],
) -> Vec<BoundaryEdge> {
    let mut boundary_edges = Vec::new();
    let mut cursor = 0usize;
    while cursor < edge_records.len() {
        let group_start = cursor;
        let key = edge_records[cursor].key;
        cursor += 1;
        while cursor < edge_records.len() && edge_records[cursor].key == key {
            cursor += 1;
        }

        if cursor - group_start != 1 {
            continue;
        }

        let edge = edge_records[group_start];
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

#[cfg(test)]
pub(super) fn boundary_edges_for_test(
    vertices: &[PathVertex],
    indices: &[u32],
) -> Vec<BoundaryEdgeSnapshot> {
    snapshots_from_edges(collect_boundary_edges(vertices, indices))
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

struct BoundaryNormals {
    normal_sums: Vec<[f32; 2]>,
    fallback_normals: Vec<[f32; 2]>,
    boundary_vertex_mask: Vec<bool>,
}

impl BoundaryNormals {
    fn new(vertex_count: usize) -> Self {
        Self {
            normal_sums: vec![[0.0, 0.0]; vertex_count],
            fallback_normals: vec![[0.0, 0.0]; vertex_count],
            boundary_vertex_mask: vec![false; vertex_count],
        }
    }

    fn accumulate(&mut self, index: usize, weighted_normal: [f32; 2], fallback_normal: [f32; 2]) {
        self.normal_sums[index] = add(self.normal_sums[index], weighted_normal);
        self.fallback_normals[index] = add(self.fallback_normals[index], fallback_normal);
        self.boundary_vertex_mask[index] = true;
    }
}

#[derive(Clone, Copy, Debug)]
struct EdgeRecord {
    key: u64,
    from: u32,
    to: u32,
    third: u32,
}

#[derive(Clone, Copy, Debug)]
struct BoundaryEdge {
    from: u32,
    to: u32,
    normal: [f32; 2],
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) struct BoundaryEdgeSnapshot {
    pub(super) from: u32,
    pub(super) to: u32,
    pub(super) normal_bits: [u32; 2],
}

#[cfg(test)]
fn snapshots_from_edges(mut edges: Vec<BoundaryEdge>) -> Vec<BoundaryEdgeSnapshot> {
    let mut snapshots = edges
        .drain(..)
        .map(|edge| BoundaryEdgeSnapshot {
            from: edge.from,
            to: edge.to,
            normal_bits: [edge.normal[0].to_bits(), edge.normal[1].to_bits()],
        })
        .collect::<Vec<_>>();
    snapshots.sort_unstable();
    snapshots
}
