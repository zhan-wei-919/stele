//! Prepare-stage document traversal and font-backed measurement.

use log::{info, warn};

use crate::font::{FontSelection, FreeTypeRasterizer, LineMetrics};
use crate::scene::BlockId;

use super::super::document::Document;
use super::super::line_break::collect_breaks;
use super::stage::{stage_span_items, SpanPrepareContext};
use super::types::PreparedBlock;

const DEFAULT_FONT_SIZE: f32 = 14.0;
const DEFAULT_LINE_HEIGHT_FACTOR: f32 = 1.4;

/// Measures the whole document and caches per-block inline data for later layout passes.
pub(crate) fn prepare_document(
    document: &Document,
    rasterizer: &FreeTypeRasterizer,
) -> Vec<PreparedBlock> {
    document
        .blocks()
        .iter()
        .enumerate()
        .map(|(document_index, block)| {
            let prepared = prepare_block(block.id(), document_index, block.spans(), rasterizer);
            info!(
                "layout.prepare block_index={} item_count={}",
                document_index,
                prepared.items.len()
            );
            prepared
        })
        .collect()
}

fn prepare_block(
    block_id: BlockId,
    document_index: usize,
    spans: &[super::super::document::Span],
    rasterizer: &FreeTypeRasterizer,
) -> PreparedBlock {
    let mut full_text = String::new();
    let mut staged_items = Vec::new();
    let mut default_ascent = fallback_line_metrics(DEFAULT_FONT_SIZE).ascent;
    let mut default_line_height = fallback_line_metrics(DEFAULT_FONT_SIZE).line_height;

    for (span_index, span) in spans.iter().enumerate() {
        let style = span.style();
        let font_selection = rasterizer.resolve_font(style.font_id(), style.bold(), style.italic());
        log_font_fallback(font_selection);

        let metrics = rasterizer.line_metrics(font_selection.resolved_font_id, style.font_size());
        default_ascent = default_ascent.max(metrics.ascent);
        default_line_height = default_line_height.max(metrics.line_height);

        let measured_glyphs = rasterizer.measure_text(
            span.text(),
            font_selection.resolved_font_id,
            style.font_size(),
            style.letter_spacing(),
        );
        stage_span_items(
            SpanPrepareContext {
                span_index,
                text: span.text(),
                style,
                font_selection,
                metrics,
                measured_glyphs: &measured_glyphs,
            },
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
        block_id,
        document_index,
        items,
        default_ascent,
        default_line_height,
    }
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

#[cfg(test)]
mod tests {
    use super::fallback_line_metrics;

    #[test]
    fn fallback_line_metrics_scale_from_font_size() {
        let metrics = fallback_line_metrics(12.0);
        assert_eq!(metrics.ascent, 12.0);
        assert!((metrics.line_height - 16.8).abs() < 0.01);
    }
}
