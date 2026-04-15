//! Vector path draw commands and hashing.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::layer::RenderLayer;
use super::validation::color_is_valid;

/// High-level path verbs that are later lowered into lyon path events.
///
/// The shared scene schema already needs the full verb set so future vector producers
/// do not fork the draw-list API.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug)]
pub(crate) enum PathVerb {
    MoveTo {
        to: [f32; 2],
    },
    LineTo {
        to: [f32; 2],
    },
    QuadTo {
        ctrl: [f32; 2],
        to: [f32; 2],
    },
    CubicTo {
        ctrl1: [f32; 2],
        ctrl2: [f32; 2],
        to: [f32; 2],
    },
    Close,
}

/// Stroke line-cap style forwarded to lyon.
///
/// These variants are kept in the schema even before a path-producing store bridge exists.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum LineCap {
    Butt,
    Round,
    Square,
}

/// Stroke line-join style forwarded to lyon.
///
/// These variants are kept in the schema even before a path-producing store bridge exists.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum LineJoin {
    Miter,
    Round,
    Bevel,
}

/// Stroke style shared by path commands.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StrokeStyle {
    color: [f32; 4],
    width: f32,
    line_cap: LineCap,
    line_join: LineJoin,
}

impl StrokeStyle {
    /// Creates a stroke style whose width and color are validated at construction time.
    ///
    /// The constructor is currently exercised by tests while the scene bridge still emits
    /// rectangles only, so keep it available without forcing downstream ad hoc builders.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(color: [f32; 4], width: f32, line_cap: LineCap, line_join: LineJoin) -> Self {
        debug_assert!(width > 0.0, "StrokeStyle width must stay positive");
        debug_assert!(
            color_is_valid(color),
            "StrokeStyle color must stay within [0, 1]"
        );
        Self {
            color,
            width,
            line_cap,
            line_join,
        }
    }

    /// Returns the normalized RGBA color used for the stroke.
    pub(crate) fn color(&self) -> [f32; 4] {
        self.color
    }

    /// Returns the logical stroke width.
    pub(crate) fn width(&self) -> f32 {
        self.width
    }

    /// Returns the line-cap style forwarded to lyon.
    pub(crate) fn line_cap(&self) -> LineCap {
        self.line_cap
    }

    /// Returns the line-join style forwarded to lyon.
    pub(crate) fn line_join(&self) -> LineJoin {
        self.line_join
    }

    /// Writes the style into a hasher without relying on `f32: Hash`.
    pub(crate) fn hash_into(&self, hasher: &mut impl Hasher) {
        hash_color(hasher, self.color);
        hasher.write_u32(self.width.to_bits());
        self.line_cap.hash(hasher);
        self.line_join.hash(hasher);
    }
}

/// Vector path command carrying fill and/or stroke styling.
#[derive(Clone, Debug)]
pub(crate) struct PathCmd {
    verbs: Vec<PathVerb>,
    fill: Option<[f32; 4]>,
    stroke: Option<StrokeStyle>,
    layer: RenderLayer,
}

impl PathCmd {
    /// Creates a path command whose fill and stroke invariants are checked once.
    ///
    /// The constructor is part of the shared draw-list surface even before runtime scene
    /// production starts emitting paths outside unit tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(
        verbs: Vec<PathVerb>,
        fill: Option<[f32; 4]>,
        stroke: Option<StrokeStyle>,
        layer: RenderLayer,
    ) -> Self {
        debug_assert!(
            fill.is_some() || stroke.is_some(),
            "PathCmd must define a fill or stroke"
        );
        debug_assert!(
            fill.map(color_is_valid).unwrap_or(true),
            "PathCmd fill color must stay within [0, 1]"
        );
        Self {
            verbs,
            fill,
            stroke,
            layer,
        }
    }

    /// Returns the high-level path verbs that define this command.
    pub(crate) fn verbs(&self) -> &[PathVerb] {
        &self.verbs
    }

    /// Returns the optional normalized RGBA fill color.
    pub(crate) fn fill(&self) -> Option<[f32; 4]> {
        self.fill
    }

    /// Returns the optional stroke style.
    pub(crate) fn stroke(&self) -> Option<StrokeStyle> {
        self.stroke
    }

    /// Returns the layer bucket that should contain this path.
    pub(crate) fn layer(&self) -> RenderLayer {
        self.layer
    }

    /// Computes the cache key used by the tessellation cache.
    pub(crate) fn content_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        hash_verbs(&mut hasher, &self.verbs);
        match self.fill {
            Some(fill) => {
                true.hash(&mut hasher);
                hash_color(&mut hasher, fill);
            }
            None => false.hash(&mut hasher),
        }
        match self.stroke {
            Some(stroke) => {
                true.hash(&mut hasher);
                stroke.hash_into(&mut hasher);
            }
            None => false.hash(&mut hasher),
        }
        hasher.finish()
    }
}

fn hash_verbs(hasher: &mut impl Hasher, verbs: &[PathVerb]) {
    verbs.len().hash(hasher);
    for verb in verbs {
        match verb {
            PathVerb::MoveTo { to } => {
                0u8.hash(hasher);
                hash_point(hasher, *to);
            }
            PathVerb::LineTo { to } => {
                1u8.hash(hasher);
                hash_point(hasher, *to);
            }
            PathVerb::QuadTo { ctrl, to } => {
                2u8.hash(hasher);
                hash_point(hasher, *ctrl);
                hash_point(hasher, *to);
            }
            PathVerb::CubicTo { ctrl1, ctrl2, to } => {
                3u8.hash(hasher);
                hash_point(hasher, *ctrl1);
                hash_point(hasher, *ctrl2);
                hash_point(hasher, *to);
            }
            PathVerb::Close => 4u8.hash(hasher),
        }
    }
}

fn hash_color(hasher: &mut impl Hasher, color: [f32; 4]) {
    for component in color {
        hasher.write_u32(component.to_bits());
    }
}

fn hash_point(hasher: &mut impl Hasher, point: [f32; 2]) {
    hasher.write_u32(point[0].to_bits());
    hasher.write_u32(point[1].to_bits());
}
