//! Path command lowering from draw-list verbs into lyon path/tessellation inputs.

use lyon::math::point;
use lyon::path::Path;
use lyon::tessellation::{LineCap as LyonLineCap, LineJoin as LyonLineJoin, StrokeOptions};

use crate::renderer::draw_list::{LineCap, LineJoin, PathVerb, StrokeStyle};

pub(super) const PATH_TESSELLATION_TOLERANCE: f32 = 0.02;

pub(super) fn build_path(verbs: &[PathVerb], scale_factor: f32) -> Option<Path> {
    let mut builder = Path::builder();
    let mut has_current_point = false;

    for verb in verbs {
        if !apply_path_verb(&mut builder, &mut has_current_point, verb, scale_factor) {
            return None;
        }
    }

    if has_current_point {
        builder.end(false);
    }

    Some(builder.build())
}

pub(super) fn stroke_options(stroke: StrokeStyle, scale_factor: f32) -> StrokeOptions {
    let line_cap = lyon_line_cap(stroke.line_cap);
    StrokeOptions::tolerance(PATH_TESSELLATION_TOLERANCE)
        .with_line_width(stroke.width * scale_factor)
        .with_start_cap(line_cap)
        .with_end_cap(line_cap)
        .with_line_join(lyon_line_join(stroke.line_join))
}

fn apply_path_verb(
    builder: &mut lyon::path::Builder,
    has_current_point: &mut bool,
    verb: &PathVerb,
    scale_factor: f32,
) -> bool {
    match *verb {
        PathVerb::MoveTo { to } => start_subpath(builder, has_current_point, to, scale_factor),
        PathVerb::LineTo { to } => {
            if !require_current_point(
                *has_current_point,
                "PathCmd LineTo requires a previous MoveTo",
            ) {
                return false;
            }
            builder.line_to(scale_point(to, scale_factor));
        }
        PathVerb::QuadTo { ctrl, to } => {
            if !require_current_point(
                *has_current_point,
                "PathCmd QuadTo requires a previous MoveTo",
            ) {
                return false;
            }
            builder.quadratic_bezier_to(
                scale_point(ctrl, scale_factor),
                scale_point(to, scale_factor),
            );
        }
        PathVerb::CubicTo { ctrl1, ctrl2, to } => {
            if !require_current_point(
                *has_current_point,
                "PathCmd CubicTo requires a previous MoveTo",
            ) {
                return false;
            }
            builder.cubic_bezier_to(
                scale_point(ctrl1, scale_factor),
                scale_point(ctrl2, scale_factor),
                scale_point(to, scale_factor),
            );
        }
        PathVerb::Close => {
            if !require_current_point(*has_current_point, "PathCmd Close requires an open subpath")
            {
                return false;
            }
            builder.close();
            *has_current_point = false;
        }
    }

    true
}

fn start_subpath(
    builder: &mut lyon::path::Builder,
    has_current_point: &mut bool,
    to: [f32; 2],
    scale_factor: f32,
) {
    if *has_current_point {
        builder.end(false);
    }
    builder.begin(scale_point(to, scale_factor));
    *has_current_point = true;
}

fn require_current_point(has_current_point: bool, message: &str) -> bool {
    if has_current_point {
        true
    } else {
        debug_assert!(false, "{message}");
        false
    }
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
