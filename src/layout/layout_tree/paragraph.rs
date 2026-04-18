//! Paragraph line breaking and run placement for the tree layout path.

use crate::draw_list::{PositionedGlyph, RectCmd, RenderLayer};
use crate::font::SubpixelBin;
use crate::layout::line_break::BreakOpportunity;
use crate::layout::prepare::PreparedGlyph;
use crate::layout::prepare_tree::{
    PreparedAtomPayload, PreparedInlineAtom, PreparedParagraph, PreparedParagraphItem,
};
use crate::layout::tree::{AtomBaseline, TextAlign, WrapMode};

use super::types::{
    LayoutAtomPayload, LayoutAtomRun, LayoutLine, LayoutParagraph, LayoutRect, LayoutRun,
    LayoutTextRun,
};

const SUBPIXEL_BIN_COUNT: f32 = 4.0;

pub(crate) fn measure_paragraph_content_width(
    paragraph: &PreparedParagraph,
    available_width: f32,
) -> f32 {
    let max_width = paragraph_max_width(paragraph, available_width);
    break_paragraph_lines(paragraph, max_width)
        .into_iter()
        .map(|line| line.line_width)
        .fold(0.0, f32::max)
}

pub(crate) fn layout_paragraph(paragraph: &PreparedParagraph, rect: LayoutRect) -> LayoutParagraph {
    if rect.width() <= 0.0 {
        return LayoutParagraph {
            rect: LayoutRect::new(rect.x(), rect.y(), rect.width(), 0.0),
            lines: Vec::new(),
        };
    }

    let max_width = paragraph_max_width(paragraph, rect.width());
    let raw_lines = break_paragraph_lines(paragraph, max_width);
    let lines = place_lines(&raw_lines, paragraph, rect);
    let height = lines
        .last()
        .map(|line| line.y + line.line_height - rect.y())
        .unwrap_or(0.0);
    LayoutParagraph {
        rect: LayoutRect::new(rect.x(), rect.y(), rect.width(), height),
        lines,
    }
}

#[derive(Clone)]
enum PendingLineItem {
    Glyph(PreparedGlyph),
    Atom {
        atom_index: usize,
        break_after: BreakOpportunity,
    },
}

impl PendingLineItem {
    fn width(&self, paragraph: &PreparedParagraph) -> f32 {
        match self {
            Self::Glyph(glyph) => glyph.advance.max(0.0),
            Self::Atom { atom_index, .. } => paragraph_atom(paragraph, *atom_index).outer_width(),
        }
    }

    fn break_after(&self) -> BreakOpportunity {
        match self {
            Self::Glyph(glyph) => glyph.break_after,
            Self::Atom { break_after, .. } => *break_after,
        }
    }
}

#[derive(Clone)]
struct BrokenLine {
    items: Vec<PendingLineItem>,
    line_width: f32,
    terminated_by_mandatory_break: bool,
    justify_opportunity_count: usize,
}

fn break_paragraph_lines(
    paragraph: &PreparedParagraph,
    max_width: f32,
) -> Vec<BrokenLine> {
    let mut lines = Vec::new();
    let mut current = PendingLine::default();

    for item in &paragraph.items {
        match item {
            PreparedParagraphItem::Break(BreakOpportunity::Mandatory) => {
                lines.push(current.take_line(true));
            }
            PreparedParagraphItem::Break(_) => {}
            PreparedParagraphItem::Glyph(glyph) => {
                current.push(PendingLineItem::Glyph(glyph.clone()), paragraph);
                if should_keep_current_line(&current, max_width) {
                    continue;
                }
                wrap_pending_line(&mut lines, &mut current, paragraph);
            }
            PreparedParagraphItem::Atom {
                atom_index,
                break_after,
            } => {
                current.push(
                    PendingLineItem::Atom {
                        atom_index: *atom_index,
                        break_after: *break_after,
                    },
                    paragraph,
                );
                if should_keep_current_line(&current, max_width) {
                    continue;
                }
                wrap_pending_line(&mut lines, &mut current, paragraph);
            }
        }
    }

    if !current.is_empty() {
        lines.push(current.take_line(false));
    }
    lines
}

fn paragraph_max_width(paragraph: &PreparedParagraph, available_width: f32) -> f32 {
    if matches!(paragraph.style.wrap, WrapMode::Wrap) {
        available_width
    } else {
        f32::INFINITY
    }
}

fn should_keep_current_line(current: &PendingLine, max_width: f32) -> bool {
    current.width <= max_width || current.items.len() == 1
}

fn wrap_pending_line(
    lines: &mut Vec<BrokenLine>,
    current: &mut PendingLine,
    paragraph: &PreparedParagraph,
) {
    if let Some(break_len) = current
        .last_allowed_break
        .filter(|break_len| *break_len < current.items.len())
    {
        let remainder = current.split_off(break_len, paragraph);
        lines.push(current.take_line(false));
        *current = PendingLine::from_items(remainder, paragraph);
    } else if let Some(overflow) = current.pop_last(paragraph) {
        lines.push(current.take_line(false));
        *current = PendingLine::from_items(vec![overflow], paragraph);
    }
}

#[derive(Default)]
struct PendingLine {
    items: Vec<PendingLineItem>,
    width: f32,
    last_allowed_break: Option<usize>,
}

impl PendingLine {
    fn from_items(items: Vec<PendingLineItem>, paragraph: &PreparedParagraph) -> Self {
        let mut pending = Self::default();
        for item in items {
            pending.push(item, paragraph);
        }
        pending
    }

    fn push(&mut self, item: PendingLineItem, paragraph: &PreparedParagraph) {
        self.width += item.width(paragraph);
        self.items.push(item);
        if self
            .items
            .last()
            .is_some_and(|item| item.break_after() == BreakOpportunity::Allowed)
        {
            self.last_allowed_break = Some(self.items.len());
        }
    }

    fn split_off(&mut self, at: usize, paragraph: &PreparedParagraph) -> Vec<PendingLineItem> {
        let remainder = self.items.split_off(at);
        self.recompute(paragraph);
        remainder
    }

    fn pop_last(&mut self, paragraph: &PreparedParagraph) -> Option<PendingLineItem> {
        let item = self.items.pop()?;
        self.recompute(paragraph);
        Some(item)
    }

    fn take_line(&mut self, terminated_by_mandatory_break: bool) -> BrokenLine {
        let line = BrokenLine {
            line_width: self.width,
            terminated_by_mandatory_break,
            justify_opportunity_count: justify_opportunity_count(&self.items),
            items: std::mem::take(&mut self.items),
        };
        self.width = 0.0;
        self.last_allowed_break = None;
        line
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn recompute(&mut self, paragraph: &PreparedParagraph) {
        self.width = self.items.iter().map(|item| item.width(paragraph)).sum();
        self.last_allowed_break = self
            .items
            .iter()
            .enumerate()
            .rev()
            .find(|(_, item)| item.break_after() == BreakOpportunity::Allowed)
            .map(|(index, _)| index + 1);
    }
}

fn justify_opportunity_count(items: &[PendingLineItem]) -> usize {
    items
        .iter()
        .take(items.len().saturating_sub(1))
        .filter(|item| item.break_after() == BreakOpportunity::Allowed)
        .count()
}

fn place_lines(
    raw_lines: &[BrokenLine],
    paragraph: &PreparedParagraph,
    rect: LayoutRect,
) -> Vec<LayoutLine> {
    let mut y = rect.y();
    raw_lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let (ascent, descent) = line_metrics(&line.items, paragraph);
            let line_height = paragraph.default_line_height.max(ascent + descent);
            let baseline = y + ascent;
            let free_width = rect.width() - line.line_width;
            let is_final_line = index + 1 == raw_lines.len();
            let alignment_offset = match paragraph.style.text_align {
                TextAlign::Start | TextAlign::Justify => 0.0,
                TextAlign::End => free_width,
                TextAlign::Center => free_width * 0.5,
            };
            let justify_gap = if paragraph.style.text_align == TextAlign::Justify
                && matches!(paragraph.style.wrap, WrapMode::Wrap)
                && !is_final_line
                && !line.terminated_by_mandatory_break
                && line.justify_opportunity_count > 0
                && free_width > 0.0
            {
                free_width / line.justify_opportunity_count as f32
            } else {
                0.0
            };
            let runs = build_runs(
                &line.items,
                paragraph,
                rect.x() + alignment_offset,
                y,
                baseline,
                line_height,
                justify_gap,
            );
            let line = LayoutLine {
                line_height,
                y,
                runs,
            };
            y += line_height;
            line
        })
        .collect()
}

fn line_metrics(items: &[PendingLineItem], paragraph: &PreparedParagraph) -> (f32, f32) {
    let default_descent = (paragraph.default_line_height - paragraph.default_ascent).max(0.0);
    let mut ascent = paragraph.default_ascent;
    let mut descent = default_descent;

    for item in items {
        match item {
            PendingLineItem::Glyph(glyph) => {
                ascent = ascent.max(glyph.ascent);
                descent = descent.max((glyph.line_height - glyph.ascent).max(0.0));
            }
            PendingLineItem::Atom { atom_index, .. } => {
                let atom = paragraph_atom(paragraph, *atom_index);
                ascent = ascent.max(atom_ascent(atom));
                descent = descent.max(atom_descent(atom));
            }
        }
    }
    (ascent, descent)
}

fn build_runs(
    items: &[PendingLineItem],
    paragraph: &PreparedParagraph,
    line_x: f32,
    line_y: f32,
    baseline: f32,
    line_height: f32,
    justify_gap: f32,
) -> Vec<LayoutRun> {
    let mut runs = Vec::new();
    let mut x = line_x;
    let mut current: Option<TextRunAccumulator> = None;

    for (index, item) in items.iter().enumerate() {
        match item {
            PendingLineItem::Glyph(glyph) => {
                let positioned = position_glyph(glyph, x, baseline);
                if current
                    .as_ref()
                    .is_some_and(|run| run.run_index != glyph.span_index)
                {
                    runs.push(LayoutRun::Text(
                        current
                            .take()
                            .expect("text run accumulator must exist")
                            .finish(line_y, baseline, line_height),
                    ));
                }

                match current.as_mut() {
                    Some(run) => run.push(glyph, positioned),
                    None => {
                        current = Some(TextRunAccumulator::new(glyph, positioned, x));
                    }
                }
                x += glyph.advance.max(0.0);
            }
            PendingLineItem::Atom { atom_index, .. } => {
                if let Some(run) = current.take() {
                    runs.push(LayoutRun::Text(run.finish(line_y, baseline, line_height)));
                }
                let atom = paragraph_atom(paragraph, *atom_index);
                let outer_top = atom_outer_top(atom, line_y, baseline, line_height);
                let rect = LayoutRect::new(
                    x + atom.style.margin.left,
                    outer_top + atom.style.margin.top,
                    atom.intrinsic_size[0],
                    atom.intrinsic_size[1],
                );
                let content_rect = atom_content_rect(atom, rect);
                runs.push(LayoutRun::Atom(LayoutAtomRun {
                    rect,
                    content_rect,
                    background: atom.style.background,
                    border: atom.style.border,
                    payload: layout_atom_payload(atom, content_rect),
                }));
                x += atom.outer_width();
            }
        }

        if justify_gap > 0.0
            && index + 1 < items.len()
            && item.break_after() == BreakOpportunity::Allowed
        {
            x += justify_gap;
            if let Some(run) = current.as_mut() {
                run.extend_to(x);
            }
        }
    }

    if let Some(run) = current {
        runs.push(LayoutRun::Text(run.finish(line_y, baseline, line_height)));
    }
    runs
}

fn atom_content_rect(atom: &PreparedInlineAtom, rect: LayoutRect) -> LayoutRect {
    let border_inset = atom.style.border.map_or(0.0, |border| {
        border
            .width
            .min(rect.width() * 0.5)
            .min(rect.height() * 0.5)
    });
    inset_rect(
        rect,
        atom.style.padding.left + border_inset,
        atom.style.padding.top + border_inset,
        atom.style.padding.right + border_inset,
        atom.style.padding.bottom + border_inset,
    )
}

fn inset_rect(rect: LayoutRect, left: f32, top: f32, right: f32, bottom: f32) -> LayoutRect {
    LayoutRect::new(
        rect.x() + left,
        rect.y() + top,
        (rect.width() - left - right).max(0.0),
        (rect.height() - top - bottom).max(0.0),
    )
}

fn layout_atom_payload(atom: &PreparedInlineAtom, content_rect: LayoutRect) -> LayoutAtomPayload {
    match &atom.payload {
        PreparedAtomPayload::Chip { measured_text } => {
            let mut x = content_rect.x();
            let inner_baseline = content_rect.y()
                + measured_text.iter().map(|glyph| glyph.ascent).fold(0.0, f32::max);
            let glyphs = measured_text
                .iter()
                .map(|glyph| {
                    let positioned = position_glyph(glyph, x, inner_baseline);
                    x += glyph.advance.max(0.0);
                    positioned
                })
                .collect();
            LayoutAtomPayload::Chip { glyphs }
        }
        PreparedAtomPayload::Icon { glyph } => LayoutAtomPayload::Icon {
            glyph: position_glyph(glyph, content_rect.x(), content_rect.y() + glyph.ascent),
        },
        PreparedAtomPayload::Image { data_ref } => LayoutAtomPayload::Image {
            data_ref: data_ref.clone(),
        },
        PreparedAtomPayload::Custom { paint } => LayoutAtomPayload::Custom {
            paint: paint.clone(),
        },
    }
}

fn position_glyph(glyph: &PreparedGlyph, x: f32, baseline: f32) -> PositionedGlyph {
    PositionedGlyph {
        font_id: glyph.font_id,
        glyph_id: glyph.glyph_id,
        font_size: glyph.font_size,
        pos: [x, baseline],
        color: glyph.color,
        subpixel_offset: SubpixelBin::new(subpixel_bin(x), subpixel_bin(baseline)),
    }
}

fn paragraph_atom(paragraph: &PreparedParagraph, atom_index: usize) -> &PreparedInlineAtom {
    paragraph
        .atoms
        .get(atom_index)
        .expect("atom referenced by paragraph items must exist")
}

fn atom_ascent(atom: &PreparedInlineAtom) -> f32 {
    let outer_height = atom.outer_height();
    match atom.baseline {
        AtomBaseline::AlphabeticAlignedToLine | AtomBaseline::Bottom => outer_height,
        AtomBaseline::MiddleOfLine => outer_height * 0.5,
        AtomBaseline::Top => 0.0,
    }
}

fn atom_descent(atom: &PreparedInlineAtom) -> f32 {
    let outer_height = atom.outer_height();
    match atom.baseline {
        AtomBaseline::AlphabeticAlignedToLine => 0.0,
        AtomBaseline::MiddleOfLine => outer_height * 0.5,
        AtomBaseline::Top => outer_height,
        AtomBaseline::Bottom => 0.0,
    }
}

fn atom_outer_top(atom: &PreparedInlineAtom, line_y: f32, baseline: f32, line_height: f32) -> f32 {
    let outer_height = atom.outer_height();
    match atom.baseline {
        AtomBaseline::AlphabeticAlignedToLine => baseline - outer_height,
        AtomBaseline::MiddleOfLine => line_y + (line_height - outer_height) * 0.5,
        AtomBaseline::Top => line_y,
        AtomBaseline::Bottom => line_y + line_height - outer_height,
    }
}

fn subpixel_bin(value: f32) -> u8 {
    let fraction = value.rem_euclid(1.0);
    let bin = (fraction * SUBPIXEL_BIN_COUNT).round() as i32;
    bin.clamp(0, 3) as u8
}

#[derive(Clone)]
struct TextRunAccumulator {
    run_index: usize,
    font_size: f32,
    color: [f32; 4],
    background_color: Option<[f32; 4]>,
    underline: bool,
    strikethrough: bool,
    start_x: f32,
    end_x: f32,
    glyphs: Vec<PositionedGlyph>,
}

impl TextRunAccumulator {
    fn new(glyph: &PreparedGlyph, positioned: PositionedGlyph, start_x: f32) -> Self {
        Self {
            run_index: glyph.span_index,
            font_size: glyph.font_size,
            color: glyph.color,
            background_color: glyph.background_color,
            underline: glyph.underline,
            strikethrough: glyph.strikethrough,
            start_x,
            end_x: positioned.pos[0] + glyph.advance.max(0.0),
            glyphs: vec![positioned],
        }
    }

    fn push(&mut self, glyph: &PreparedGlyph, positioned: PositionedGlyph) {
        self.end_x = positioned.pos[0] + glyph.advance.max(0.0);
        self.glyphs.push(positioned);
    }

    fn extend_to(&mut self, x: f32) {
        self.end_x = self.end_x.max(x);
    }

    fn finish(self, line_y: f32, baseline: f32, line_height: f32) -> LayoutTextRun {
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
        LayoutTextRun {
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
    use std::sync::Arc;

    use crate::draw_list::ImageData;
    use crate::layout::line_break::BreakOpportunity;
    use crate::layout::prepare::PreparedGlyph;
    use crate::layout::prepare_tree::{
        PreparedAtomPayload, PreparedInlineAtom, PreparedParagraph, PreparedParagraphItem,
    };
    use crate::layout::tree::{AtomBaseline, InlineAtomStyle, ParagraphStyle, TextAlign};

    use super::{layout_paragraph, LayoutRect, LayoutRun};

    #[test]
    fn paragraph_layout_keeps_atom_and_text_on_shared_line() {
        let paragraph = PreparedParagraph {
            node_id: crate::layout::tree::NodeId::new(1),
            atoms: vec![PreparedInlineAtom {
                intrinsic_size: [12.0, 12.0],
                baseline: AtomBaseline::MiddleOfLine,
                style: InlineAtomStyle::default(),
                payload: PreparedAtomPayload::Image {
                    data_ref: Arc::new(ImageData::new(vec![255; 4], 1, 1)),
                },
            }],
            items: vec![
                PreparedParagraphItem::Glyph(glyph(0, 1, 10.0, 14.0)),
                PreparedParagraphItem::Atom {
                    atom_index: 0,
                    break_after: BreakOpportunity::Forbidden,
                },
            ],
            default_ascent: 10.0,
            default_line_height: 20.0,
            style: ParagraphStyle::default(),
        };

        let laid_out = layout_paragraph(&paragraph, LayoutRect::new(0.0, 0.0, 100.0, 40.0));
        assert_eq!(laid_out.lines.len(), 1);
        assert_eq!(laid_out.lines[0].runs.len(), 2);
    }

    #[test]
    fn paragraph_end_alignment_shifts_short_line_to_the_right_edge() {
        let paragraph = PreparedParagraph {
            node_id: crate::layout::tree::NodeId::new(2),
            atoms: Vec::new(),
            items: vec![PreparedParagraphItem::Glyph(glyph(0, 1, 12.0, 14.0))],
            default_ascent: 10.0,
            default_line_height: 20.0,
            style: ParagraphStyle {
                text_align: TextAlign::End,
                ..ParagraphStyle::default()
            },
        };

        let laid_out = layout_paragraph(&paragraph, LayoutRect::new(0.0, 0.0, 40.0, 40.0));
        let LayoutRun::Text(run) = &laid_out.lines[0].runs[0] else {
            panic!("expected text run");
        };
        assert!((run.glyphs[0].pos[0] - 28.0).abs() < 0.01);
    }

    #[test]
    fn paragraph_justify_expands_intermediate_line_but_not_the_last_line() {
        let mut glyphs = vec![
            glyph(0, 1, 8.0, 14.0),
            glyph(0, 2, 8.0, 14.0),
            glyph(0, 3, 8.0, 14.0),
            glyph(0, 4, 8.0, 14.0),
        ];
        glyphs[0].break_after = BreakOpportunity::Allowed;
        glyphs[2].break_after = BreakOpportunity::Allowed;

        let paragraph = PreparedParagraph {
            node_id: crate::layout::tree::NodeId::new(3),
            atoms: Vec::new(),
            items: glyphs
                .into_iter()
                .map(PreparedParagraphItem::Glyph)
                .collect(),
            default_ascent: 10.0,
            default_line_height: 20.0,
            style: ParagraphStyle {
                text_align: TextAlign::Justify,
                ..ParagraphStyle::default()
            },
        };

        let laid_out = layout_paragraph(&paragraph, LayoutRect::new(0.0, 0.0, 26.0, 80.0));
        assert_eq!(laid_out.lines.len(), 2);

        let LayoutRun::Text(first_line) = &laid_out.lines[0].runs[0] else {
            panic!("expected first line text run");
        };
        let LayoutRun::Text(last_line) = &laid_out.lines[1].runs[0] else {
            panic!("expected second line text run");
        };
        assert!((first_line.glyphs[1].pos[0] - 10.0).abs() < 0.01);
        assert!((last_line.glyphs[0].pos[0] - 0.0).abs() < 0.01);
    }

    #[test]
    fn paragraph_justify_does_not_expand_a_line_ended_by_mandatory_break() {
        let mut first = glyph(0, 1, 8.0, 14.0);
        first.break_after = BreakOpportunity::Allowed;
        let second = glyph(0, 2, 8.0, 14.0);
        let third = glyph(0, 3, 8.0, 14.0);

        let paragraph = PreparedParagraph {
            node_id: crate::layout::tree::NodeId::new(4),
            atoms: Vec::new(),
            items: vec![
                PreparedParagraphItem::Glyph(first),
                PreparedParagraphItem::Glyph(second),
                PreparedParagraphItem::Break(BreakOpportunity::Mandatory),
                PreparedParagraphItem::Glyph(third),
            ],
            default_ascent: 10.0,
            default_line_height: 20.0,
            style: ParagraphStyle {
                text_align: TextAlign::Justify,
                ..ParagraphStyle::default()
            },
        };

        let laid_out = layout_paragraph(&paragraph, LayoutRect::new(0.0, 0.0, 30.0, 80.0));
        let LayoutRun::Text(first_line) = &laid_out.lines[0].runs[0] else {
            panic!("expected first line text run");
        };
        assert!((first_line.glyphs[1].pos[0] - 8.0).abs() < 0.01);
    }

    #[test]
    fn paragraph_justify_inserts_gap_after_atom_break_opportunity() {
        let mut first_glyph = glyph(0, 1, 8.0, 14.0);
        first_glyph.break_after = BreakOpportunity::Allowed;
        let paragraph = PreparedParagraph {
            node_id: crate::layout::tree::NodeId::new(5),
            atoms: vec![PreparedInlineAtom {
                intrinsic_size: [10.0, 10.0],
                baseline: AtomBaseline::MiddleOfLine,
                style: InlineAtomStyle::default(),
                payload: PreparedAtomPayload::Image {
                    data_ref: Arc::new(ImageData::new(vec![255; 4], 1, 1)),
                },
            }],
            items: vec![
                PreparedParagraphItem::Atom {
                    atom_index: 0,
                    break_after: BreakOpportunity::Allowed,
                },
                PreparedParagraphItem::Glyph(first_glyph),
                PreparedParagraphItem::Glyph(glyph(0, 2, 8.0, 14.0)),
            ],
            default_ascent: 10.0,
            default_line_height: 20.0,
            style: ParagraphStyle {
                text_align: TextAlign::Justify,
                ..ParagraphStyle::default()
            },
        };

        let laid_out = layout_paragraph(&paragraph, LayoutRect::new(0.0, 0.0, 20.0, 80.0));
        let LayoutRun::Atom(atom) = &laid_out.lines[0].runs[0] else {
            panic!("expected atom run");
        };
        let LayoutRun::Text(text) = &laid_out.lines[0].runs[1] else {
            panic!("expected text run");
        };
        assert!((atom.rect.width() - 10.0).abs() < 0.01);
        assert!((text.glyphs[0].pos[0] - 12.0).abs() < 0.01);
    }

    fn glyph(span_index: usize, glyph_id: u16, advance: f32, font_size: f32) -> PreparedGlyph {
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
            break_after: BreakOpportunity::Forbidden,
        }
    }
}
