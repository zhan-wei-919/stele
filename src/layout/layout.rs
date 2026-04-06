//! Pure arithmetic layout over prepared inline measurements.

use log::{info, warn};

use crate::font::SubpixelBin;
use crate::renderer::{PositionedGlyph, RectCmd, RenderLayer};

use super::document::{BlockRect, Document};
use super::line_break::BreakOpportunity;
use super::prepare::{PreparedBlock, PreparedGlyph, PreparedItem};

const SUBPIXEL_BIN_COUNT: f32 = 4.0;

/// One block's positioned lines and block-level drawing metadata.
#[derive(Clone, Debug)]
pub struct LayoutBlock {
    pub block_index: usize,
    pub z_order: u32,
    pub lines: Vec<LayoutLine>,
    pub background_rect: Option<RectCmd>,
    pub clip_rect: BlockRect,
}

/// One laid out line sharing the same baseline.
#[derive(Clone, Debug)]
pub struct LayoutLine {
    pub runs: Vec<LayoutRun>,
    pub y: f32,
    pub line_height: f32,
    pub baseline: f32,
}

/// One contiguous run from a single span on a single line.
#[derive(Clone, Debug)]
pub struct LayoutRun {
    pub glyphs: Vec<PositionedGlyph>,
    pub decoration_rects: Vec<RectCmd>,
}

/// Positions all prepared blocks using the current document geometry.
pub(crate) fn layout_document(
    document: &Document,
    prepared_blocks: &[PreparedBlock],
) -> Vec<LayoutBlock> {
    prepared_blocks
        .iter()
        .filter_map(|prepared| layout_block(document, prepared))
        .collect()
}

fn layout_block(document: &Document, prepared: &PreparedBlock) -> Option<LayoutBlock> {
    let Some(block) = document.blocks.get(prepared.block_index) else {
        warn!(
            "layout.warn.skip_block block_index={} reason=missing_document_block",
            prepared.block_index
        );
        return None;
    };
    if !block.rect.is_valid() {
        warn!(
            "layout.warn.skip_block block_index={} reason=invalid_rect",
            prepared.block_index
        );
        return None;
    }

    let content_rect = block.rect.inset(block.padding.max(0.0));
    let raw_lines = if content_rect.width > 0.0 && content_rect.width.is_finite() {
        greedy_line_break(prepared, content_rect.width)
    } else {
        Vec::new()
    };
    let lines = place_lines(
        &raw_lines,
        content_rect,
        prepared.default_ascent,
        prepared.default_line_height,
    );

    info!(
        "layout.layout block_index={} line_count={}",
        prepared.block_index,
        lines.len()
    );
    Some(LayoutBlock {
        block_index: prepared.block_index,
        z_order: block.z_order,
        lines,
        background_rect: block.background_color.map(|color| {
            RectCmd::new(
                [block.rect.x, block.rect.y],
                [block.rect.width, block.rect.height],
                color,
                RenderLayer::Background,
            )
        }),
        clip_rect: block.rect,
    })
}

fn greedy_line_break(prepared: &PreparedBlock, max_width: f32) -> Vec<Vec<PreparedGlyph>> {
    let mut lines = Vec::new();
    let mut current = PendingLine::default();

    for item in &prepared.items {
        match item {
            PreparedItem::Break(BreakOpportunity::Mandatory) => {
                lines.push(current.take());
            }
            PreparedItem::Break(_) => {}
            PreparedItem::Glyph(glyph) => {
                current.push(glyph.clone());
                if current.width <= max_width || current.glyphs.len() == 1 {
                    if current.width > max_width && current.glyphs.len() == 1 {
                        lines.push(current.take());
                    }
                    continue;
                }

                if let Some(break_len) = current
                    .last_allowed_break
                    .filter(|break_len| *break_len < current.glyphs.len())
                {
                    let remainder = current.split_off(break_len);
                    lines.push(current.take());
                    current = PendingLine::from_glyphs(remainder);
                } else if let Some(overflow) = current.pop_last() {
                    lines.push(current.take());
                    current = PendingLine::from_glyphs(vec![overflow]);
                }
            }
        }
    }

    if !current.is_empty() {
        lines.push(current.take());
    }
    lines
}

fn place_lines(
    raw_lines: &[Vec<PreparedGlyph>],
    content_rect: BlockRect,
    default_ascent: f32,
    default_line_height: f32,
) -> Vec<LayoutLine> {
    let mut y = content_rect.y;
    raw_lines
        .iter()
        .map(|glyphs| {
            let max_ascent = glyphs
                .iter()
                .map(|glyph| glyph.ascent)
                .fold(default_ascent, f32::max);
            let line_height = glyphs
                .iter()
                .map(|glyph| glyph.line_height)
                .fold(default_line_height, f32::max);
            let baseline = y + max_ascent;
            let runs = build_runs(glyphs, content_rect.x, y, baseline, line_height);
            let line = LayoutLine {
                runs,
                y,
                line_height,
                baseline,
            };
            y += line_height;
            line
        })
        .collect()
}

fn build_runs(
    glyphs: &[PreparedGlyph],
    line_x: f32,
    line_y: f32,
    baseline: f32,
    line_height: f32,
) -> Vec<LayoutRun> {
    let mut runs = Vec::new();
    let mut x = line_x;
    let mut current: Option<RunAccumulator> = None;

    for glyph in glyphs {
        let positioned = PositionedGlyph {
            font_id: glyph.font_id,
            glyph_id: glyph.glyph_id,
            font_size: glyph.font_size,
            pos: [x, baseline],
            color: glyph.color,
            subpixel_offset: SubpixelBin::new(subpixel_bin(x), subpixel_bin(baseline)),
        };

        if current
            .as_ref()
            .is_some_and(|run| run.span_index != glyph.span_index)
        {
            runs.push(
                current
                    .take()
                    .expect("run accumulator must exist before switching spans")
                    .finish(line_y, baseline, line_height),
            );
        }

        match current.as_mut() {
            Some(run) => run.push(glyph, positioned),
            None => current = Some(RunAccumulator::new(glyph, positioned, x)),
        }
        x += glyph.advance.max(0.0);
    }

    if let Some(run) = current {
        runs.push(run.finish(line_y, baseline, line_height));
    }
    runs
}

fn subpixel_bin(value: f32) -> u8 {
    let fraction = value.rem_euclid(1.0);
    let bin = (fraction * SUBPIXEL_BIN_COUNT).round() as i32;
    bin.clamp(0, 3) as u8
}

#[derive(Clone, Debug, Default)]
struct PendingLine {
    glyphs: Vec<PreparedGlyph>,
    width: f32,
    last_allowed_break: Option<usize>,
}

impl PendingLine {
    fn from_glyphs(glyphs: Vec<PreparedGlyph>) -> Self {
        let mut pending = Self::default();
        for glyph in glyphs {
            pending.push(glyph);
        }
        pending
    }

    fn push(&mut self, glyph: PreparedGlyph) {
        self.width += glyph.advance.max(0.0);
        self.glyphs.push(glyph);
        if self
            .glyphs
            .last()
            .is_some_and(|glyph| glyph.break_after == BreakOpportunity::Allowed)
        {
            self.last_allowed_break = Some(self.glyphs.len());
        }
    }

    fn pop_last(&mut self) -> Option<PreparedGlyph> {
        let glyph = self.glyphs.pop()?;
        self.width = (self.width - glyph.advance.max(0.0)).max(0.0);
        self.last_allowed_break = self
            .glyphs
            .iter()
            .enumerate()
            .rev()
            .find(|(_, glyph)| glyph.break_after == BreakOpportunity::Allowed)
            .map(|(index, _)| index + 1);
        Some(glyph)
    }

    fn split_off(&mut self, at: usize) -> Vec<PreparedGlyph> {
        let remainder = self.glyphs.split_off(at);
        self.width = self.glyphs.iter().map(|glyph| glyph.advance.max(0.0)).sum();
        self.last_allowed_break = self
            .glyphs
            .iter()
            .enumerate()
            .rev()
            .find(|(_, glyph)| glyph.break_after == BreakOpportunity::Allowed)
            .map(|(index, _)| index + 1);
        remainder
    }

    fn take(&mut self) -> Vec<PreparedGlyph> {
        self.width = 0.0;
        self.last_allowed_break = None;
        std::mem::take(&mut self.glyphs)
    }

    fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }
}

#[derive(Clone, Debug)]
struct RunAccumulator {
    span_index: usize,
    font_size: f32,
    color: [f32; 4],
    background_color: Option<[f32; 4]>,
    underline: bool,
    strikethrough: bool,
    start_x: f32,
    end_x: f32,
    glyphs: Vec<PositionedGlyph>,
}

impl RunAccumulator {
    fn new(glyph: &PreparedGlyph, positioned: PositionedGlyph, start_x: f32) -> Self {
        Self {
            span_index: glyph.span_index,
            font_size: glyph.font_size,
            color: glyph.color,
            background_color: glyph.background_color,
            underline: glyph.underline,
            strikethrough: glyph.strikethrough,
            start_x,
            end_x: start_x + glyph.advance.max(0.0),
            glyphs: vec![positioned],
        }
    }

    fn push(&mut self, glyph: &PreparedGlyph, positioned: PositionedGlyph) {
        self.end_x += glyph.advance.max(0.0);
        self.glyphs.push(positioned);
    }

    fn finish(self, line_y: f32, baseline: f32, line_height: f32) -> LayoutRun {
        let width = (self.end_x - self.start_x).max(0.0);
        let mut decoration_rects = Vec::new();

        if width > 0.0 {
            if let Some(color) = self.background_color {
                decoration_rects.push(RectCmd::new(
                    [self.start_x, line_y],
                    [width, line_height],
                    color,
                    RenderLayer::Background,
                ));
            }
            if self.underline {
                decoration_rects.push(RectCmd::new(
                    [self.start_x, baseline + underline_offset(self.font_size)],
                    [width, decoration_thickness(self.font_size)],
                    self.color,
                    RenderLayer::Foreground,
                ));
            }
            if self.strikethrough {
                decoration_rects.push(RectCmd::new(
                    [self.start_x, baseline - self.font_size * 0.3],
                    [width, decoration_thickness(self.font_size)],
                    self.color,
                    RenderLayer::Foreground,
                ));
            }
        }

        LayoutRun {
            glyphs: self.glyphs,
            decoration_rects,
        }
    }
}

fn underline_offset(font_size: f32) -> f32 {
    (font_size * 0.1).max(1.0)
}

fn decoration_thickness(font_size: f32) -> f32 {
    (font_size * 0.06).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::{layout_document, LayoutLine};
    use crate::layout::{
        line_break::BreakOpportunity,
        prepare::{PreparedBlock, PreparedGlyph, PreparedItem},
        Block, BlockRect, Document, Span, TextStyle,
    };
    use crate::renderer::RenderLayer;

    #[test]
    fn layout_wraps_at_allowed_breaks_and_resets_x() {
        let document = Document::new(vec![Block::new(
            BlockRect::new(0.0, 0.0, 30.0, 120.0),
            0.0,
            None,
            vec![Span::new(
                "ignored",
                TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]),
            )],
            0,
        )]);
        let prepared = PreparedBlock {
            block_index: 0,
            items: vec![
                PreparedItem::Glyph(glyph(0, 1, 10.0, 14.0, BreakOpportunity::Forbidden)),
                PreparedItem::Glyph(glyph(0, 2, 10.0, 14.0, BreakOpportunity::Forbidden)),
                PreparedItem::Glyph(glyph(0, 3, 5.0, 14.0, BreakOpportunity::Allowed)),
                PreparedItem::Glyph(glyph(0, 4, 12.0, 14.0, BreakOpportunity::Forbidden)),
                PreparedItem::Glyph(glyph(0, 5, 12.0, 14.0, BreakOpportunity::Forbidden)),
            ],
            default_ascent: 10.0,
            default_line_height: 20.0,
        };

        let layout_blocks = layout_document(&document, &[prepared]);
        assert_eq!(layout_blocks[0].lines.len(), 2);
        assert_line_starts_at_zero(&layout_blocks[0].lines[0]);
        assert_line_starts_at_zero(&layout_blocks[0].lines[1]);
        assert!(layout_blocks[0].lines[1].y > layout_blocks[0].lines[0].y);
        assert_eq!(layout_blocks[0].lines[1].runs[0].glyphs.len(), 2);
    }

    #[test]
    fn layout_aligns_baseline_for_mixed_font_sizes() {
        let document = Document::new(vec![Block::new(
            BlockRect::new(0.0, 0.0, 200.0, 120.0),
            0.0,
            None,
            vec![],
            0,
        )]);
        let prepared = PreparedBlock {
            block_index: 0,
            items: vec![
                PreparedItem::Glyph(glyph(0, 1, 12.0, 24.0, BreakOpportunity::Forbidden)),
                PreparedItem::Glyph(glyph(1, 2, 8.0, 14.0, BreakOpportunity::Forbidden)),
            ],
            default_ascent: 18.0,
            default_line_height: 33.6,
        };

        let line = &layout_document(&document, &[prepared])[0].lines[0];
        assert_eq!(line.runs.len(), 2);
        assert_eq!(line.runs[0].glyphs[0].pos[1], line.runs[1].glyphs[0].pos[1]);
        assert!((line.line_height - 33.6).abs() < 0.01);
        assert!((line.baseline - 18.0).abs() < 0.01);
    }

    #[test]
    fn layout_emits_background_and_underline_rects() {
        let document = Document::new(vec![Block::new(
            BlockRect::new(0.0, 0.0, 120.0, 80.0),
            0.0,
            None,
            vec![],
            0,
        )]);
        let prepared = PreparedBlock {
            block_index: 0,
            items: vec![PreparedItem::Glyph(PreparedGlyph {
                span_index: 0,
                font_id: 0,
                glyph_id: 1,
                font_size: 16.0,
                advance: 20.0,
                ascent: 12.0,
                line_height: 22.4,
                color: [1.0, 1.0, 1.0, 1.0],
                background_color: Some([0.2, 0.2, 0.2, 1.0]),
                underline: true,
                strikethrough: false,
                break_after: BreakOpportunity::Forbidden,
            })],
            default_ascent: 12.0,
            default_line_height: 22.4,
        };

        let line = &layout_document(&document, &[prepared])[0].lines[0];
        let rects = &line.runs[0].decoration_rects;
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].layer(), RenderLayer::Background);
        assert_eq!(rects[1].layer(), RenderLayer::Foreground);
    }

    fn glyph(
        span_index: usize,
        glyph_id: u16,
        advance: f32,
        font_size: f32,
        break_after: BreakOpportunity,
    ) -> PreparedGlyph {
        PreparedGlyph {
            span_index,
            font_id: 0,
            glyph_id,
            font_size,
            advance,
            ascent: font_size * 0.75,
            line_height: font_size * 1.4,
            color: [1.0, 1.0, 1.0, 1.0],
            background_color: None,
            underline: false,
            strikethrough: false,
            break_after,
        }
    }

    fn assert_line_starts_at_zero(line: &LayoutLine) {
        assert_eq!(line.runs[0].glyphs[0].pos[0], 0.0);
    }
}
