//! Tree-level local paint payloads shared by custom atoms and embeds.

use std::sync::Arc;

use crate::draw_list::{ImageData, LineCap, LineJoin, PathVerb};
use crate::layout::document::{
    validation::{validate_color, validate_optional_color},
    DocumentError,
};

use super::validation::validate_dimension;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PathStroke {
    pub(crate) color: [f32; 4],
    pub(crate) width: f32,
    pub(crate) line_cap: LineCap,
    pub(crate) line_join: LineJoin,
}

// The semantic tree keeps all supported local paint primitives available even though current
// production demo content does not instantiate every variant yet.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) enum LocalPaintCommand {
    Rect {
        pos: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
    },
    Path {
        verbs: Vec<PathVerb>,
        fill: Option<[f32; 4]>,
        stroke: Option<PathStroke>,
    },
    Image {
        pos: [f32; 2],
        size: [f32; 2],
        data_ref: Arc<ImageData>,
    },
}

pub(crate) fn validate_local_paint_commands(
    paint: &[LocalPaintCommand],
) -> Result<(), DocumentError> {
    for command in paint {
        validate_local_paint_command(command)?;
    }
    Ok(())
}

fn validate_local_paint_command(command: &LocalPaintCommand) -> Result<(), DocumentError> {
    match command {
        LocalPaintCommand::Rect { pos, size, color } => {
            validate_point(*pos)?;
            validate_size(*size)?;
            validate_color(*color).map_err(|_| DocumentError::InvalidLocalPaint)
        }
        LocalPaintCommand::Path { verbs, fill, stroke } => {
            if fill.is_none() && stroke.is_none() {
                return Err(DocumentError::InvalidLocalPaint);
            }
            for verb in verbs {
                validate_path_verb(verb)?;
            }
            validate_optional_color(*fill).map_err(|_| DocumentError::InvalidLocalPaint)?;
            if let Some(stroke) = stroke {
                validate_path_stroke(*stroke)?;
            }
            Ok(())
        }
        LocalPaintCommand::Image {
            pos,
            size,
            data_ref,
        } => {
            validate_point(*pos)?;
            validate_size(*size)?;
            if data_ref.is_valid() {
                Ok(())
            } else {
                Err(DocumentError::InvalidLocalPaint)
            }
        }
    }
}

fn validate_path_verb(verb: &PathVerb) -> Result<(), DocumentError> {
    match verb {
        PathVerb::MoveTo { to } | PathVerb::LineTo { to } => validate_point(*to),
        PathVerb::QuadTo { ctrl, to } => {
            validate_point(*ctrl)?;
            validate_point(*to)
        }
        PathVerb::CubicTo { ctrl1, ctrl2, to } => {
            validate_point(*ctrl1)?;
            validate_point(*ctrl2)?;
            validate_point(*to)
        }
        PathVerb::Close => Ok(()),
    }
}

fn validate_path_stroke(stroke: PathStroke) -> Result<(), DocumentError> {
    validate_color(stroke.color).map_err(|_| DocumentError::InvalidLocalPaint)?;
    validate_dimension(stroke.width, false).map_err(|_| DocumentError::InvalidLocalPaint)?;
    Ok(())
}

fn validate_point(point: [f32; 2]) -> Result<(), DocumentError> {
    if point.into_iter().all(f32::is_finite) {
        Ok(())
    } else {
        Err(DocumentError::InvalidLocalPaint)
    }
}

fn validate_size(size: [f32; 2]) -> Result<(), DocumentError> {
    validate_dimension(size[0], false).map_err(|_| DocumentError::InvalidLocalPaint)?;
    validate_dimension(size[1], false).map_err(|_| DocumentError::InvalidLocalPaint)?;
    Ok(())
}
