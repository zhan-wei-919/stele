//! Prepare-stage measurement that turns spans into FreeType-free layout data.

use log::{info, warn};

use crate::font::{FontSelection, FreeTypeRasterizer, LineMetrics, MeasuredGlyph};

use super::document::{Document, TextStyle};
use super::line_break::{collect_breaks, BreakOpportunity};

const DEFAULT_FONT_SIZE: f32 = 14.0;
const DEFAULT_LINE_HEIGHT_FACTOR: f32 = 1.4;

/// A block's prepared inline content plus cached metrics for later layout.
#[derive(Clone, Debug)]
pub struct PreparedBlock {
    pub(crate) block_index: usize,
    pub(crate) items: Vec<PreparedItem>,
    pub(crate) default_ascent: f32,
    pub(crate) default_line_height: f32,
}

/// A layout item that is either visible content or an explicit hard break.
#[derive(Clone, Debug)]
pub(crate) enum PreparedItem {
    Glyph(PreparedGlyph),
    Break(BreakOpportunity),
}

/// One measured glyph plus the styling needed to emit renderer primitives later.
#[derive(Clone, Debug)]
pub(crate) struct PreparedGlyph {
    pub(crate) span_index: usize,
    pub(crate) font_id: u32,
    pub(crate) glyph_id: u16,
    pub(crate) font_size: f32,
    pub(crate) advance: f32,
    pub(crate) ascent: f32,
    pub(crate) line_height: f32,
    pub(crate) color: [f32; 4],
    pub(crate) background_color: Option<[f32; 4]>,
    pub(crate) underline: bool,
    pub(crate) strikethrough: bool,
    pub(crate) break_after: BreakOpportunity,
}

impl PreparedGlyph {
    fn from_measurement(
        span_index: usize,
        style: TextStyle,
        font_id: u32,
        metrics: LineMetrics,
        measured: MeasuredGlyph,
    ) -> Self {
        Self {
            span_index,
            font_id,
            glyph_id: measured.glyph_id,
            font_size: style.font_size,
            advance: measured.advance,
            ascent: metrics.ascent,
            line_height: metrics.line_height,
            color: style.color,
            background_color: style.background_color,
            underline: style.underline,
            strikethrough: style.strikethrough,
            break_after: BreakOpportunity::Forbidden,
        }
    }
}

/// Measures the whole document and caches per-block inline data for later layout passes.
pub(crate) fn prepare_document(
    document: &Document,
    rasterizer: &FreeTypeRasterizer,
) -> Vec<PreparedBlock> {
    document
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(block_index, block)| {
            if !block.rect.is_valid() {
                warn!("layout.warn.skip_block block_index={block_index} reason=invalid_rect");
                return None;
            }

            let prepared = prepare_block(block_index, block.spans.iter(), rasterizer);
            info!(
                "layout.prepare block_index={} item_count={}",
                block_index,
                prepared.items.len()
            );
            Some(prepared)
        })
        .collect()
}

fn prepare_block<'a>(
    block_index: usize,
    spans: impl Iterator<Item = &'a super::document::Span>,
    rasterizer: &FreeTypeRasterizer,
) -> PreparedBlock {
    let mut full_text = String::new();
    let mut staged_items = Vec::new();
    let mut default_ascent = fallback_line_metrics(DEFAULT_FONT_SIZE).ascent;
    let mut default_line_height = fallback_line_metrics(DEFAULT_FONT_SIZE).line_height;

    for (span_index, span) in spans.enumerate() {
        let style = normalize_style(span.style);
        let font_selection = rasterizer.resolve_font(style.font_id, style.bold, style.italic);
        log_font_fallback(font_selection);

        let metrics = rasterizer.line_metrics(font_selection.resolved_font_id, style.font_size);
        default_ascent = default_ascent.max(metrics.ascent);
        default_line_height = default_line_height.max(metrics.line_height);

        let measured_glyphs = rasterizer.measure_text(
            &span.text,
            font_selection.resolved_font_id,
            style.font_size,
            style.letter_spacing,
        );
        stage_span_items(
            span_index,
            &span.text,
            style,
            font_selection,
            metrics,
            &measured_glyphs,
            &mut full_text,
            &mut staged_items,
        );
    }

    let break_map = collect_breaks(&full_text);
    let items = staged_items
        .into_iter()
        .map(|item| item.into_prepared(&break_map))
        .collect();

    PreparedBlock {
        block_index,
        items,
        default_ascent,
        default_line_height,
    }
}

fn stage_span_items(
    span_index: usize,
    text: &str,
    style: TextStyle,
    font_selection: FontSelection,
    metrics: LineMetrics,
    measured_glyphs: &[MeasuredGlyph],
    full_text: &mut String,
    staged_items: &mut Vec<StagedItem>,
) {
    let mut measured_index = 0usize;
    for ch in text.chars() {
        full_text.push(ch);
        if ch == '\n' {
            staged_items.push(StagedItem::Break);
            continue;
        }

        let measured = measured_glyphs
            .get(measured_index)
            .copied()
            .unwrap_or_else(|| MeasuredGlyph::fallback(style.font_size, style.letter_spacing));
        measured_index += 1;
        staged_items.push(StagedItem::Glyph {
            byte_end: full_text.len(),
            glyph: PreparedGlyph::from_measurement(
                span_index,
                style,
                font_selection.resolved_font_id,
                metrics,
                measured,
            ),
        });
    }
}

fn normalize_style(style: TextStyle) -> TextStyle {
    let font_size = if style.font_size.is_finite() && style.font_size > 0.0 {
        style.font_size
    } else {
        DEFAULT_FONT_SIZE
    };
    let letter_spacing = if style.letter_spacing.is_finite() {
        style.letter_spacing.max(0.0)
    } else {
        0.0
    };

    TextStyle {
        font_size,
        letter_spacing,
        color: clamp_color(style.color),
        background_color: style.background_color.map(clamp_color),
        ..style
    }
}

fn clamp_color(color: [f32; 4]) -> [f32; 4] {
    color.map(|component| {
        if component.is_finite() {
            component.clamp(0.0, 1.0)
        } else {
            0.0
        }
    })
}

fn log_font_fallback(selection: FontSelection) {
    if selection.requested_font_id != selection.resolved_font_id {
        warn!(
            "layout.warn.font_fallback requested_font_id={} resolved_font_id={}",
            selection.requested_font_id, selection.resolved_font_id
        );
    }
}

fn fallback_line_metrics(font_size: f32) -> LineMetrics {
    LineMetrics {
        ascent: font_size,
        line_height: font_size * DEFAULT_LINE_HEIGHT_FACTOR,
    }
}

#[derive(Clone, Debug)]
enum StagedItem {
    Glyph {
        byte_end: usize,
        glyph: PreparedGlyph,
    },
    Break,
}

impl StagedItem {
    fn into_prepared(
        self,
        break_map: &std::collections::HashMap<usize, BreakOpportunity>,
    ) -> PreparedItem {
        match self {
            Self::Glyph {
                byte_end,
                mut glyph,
            } => {
                glyph.break_after = break_map
                    .get(&byte_end)
                    .copied()
                    .unwrap_or(BreakOpportunity::Forbidden);
                PreparedItem::Glyph(glyph)
            }
            Self::Break => PreparedItem::Break(BreakOpportunity::Mandatory),
        }
    }
}
