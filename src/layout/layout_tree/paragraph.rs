//! Paragraph line breaking and run placement for the tree layout path.

use crate::draw_list::{PositionedGlyph, RectCmd, RenderLayer};
use crate::font::SubpixelBin;
use crate::layout::line_break::BreakOpportunity;
use crate::layout::prepare::PreparedGlyph;
use crate::layout::prepare_tree::{
    PreparedAtomPayload, PreparedInlineAtom, PreparedParagraph, PreparedParagraphItem,
};
use crate::layout::tree::AtomBaseline;

use super::types::{
    LayoutAtomPayload, LayoutAtomRun, LayoutLine, LayoutParagraph, LayoutRect, LayoutRun,
    LayoutTextRun,
};

const SUBPIXEL_BIN_COUNT: f32 = 4.0;

pub(crate) fn layout_paragraph(paragraph: &PreparedParagraph, rect: LayoutRect) -> LayoutParagraph {
    if rect.width() <= 0.0 {
        return LayoutParagraph {
            rect: LayoutRect::new(rect.x(), rect.y(), rect.width(), 0.0),
            lines: Vec::new(),
        };
    }

    let max_width = if matches!(paragraph.style.wrap, crate::layout::tree::WrapMode::Wrap) {
        rect.width()
    } else {
        f32::INFINITY
    };
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
    Atom { atom_index: usize },
}

impl PendingLineItem {
    fn width(&self, paragraph: &PreparedParagraph) -> f32 {
        match self {
            Self::Glyph(glyph) => glyph.advance.max(0.0),
            Self::Atom { atom_index } => paragraph_atom(paragraph, *atom_index).outer_width(),
        }
    }

    fn break_after(&self, paragraph: &PreparedParagraph) -> BreakOpportunity {
        match self {
            Self::Glyph(glyph) => glyph.break_after,
            Self::Atom { atom_index } => paragraph
                .items
                .iter()
                .find_map(|item| match item {
                    PreparedParagraphItem::Atom {
                        atom_index: current,
                        break_after,
                    } if *current == *atom_index => Some(*break_after),
                    _ => None,
                })
                .unwrap_or(BreakOpportunity::Forbidden),
        }
    }
}

fn break_paragraph_lines(
    paragraph: &PreparedParagraph,
    max_width: f32,
) -> Vec<Vec<PendingLineItem>> {
    let mut lines = Vec::new();
    let mut current = PendingLine::default();

    for item in &paragraph.items {
        match item {
            PreparedParagraphItem::Break(BreakOpportunity::Mandatory) => {
                lines.push(current.take());
            }
            PreparedParagraphItem::Break(_) => {}
            PreparedParagraphItem::Glyph(glyph) => {
                current.push(PendingLineItem::Glyph(glyph.clone()), paragraph);
                if should_keep_current_line(&current, max_width) {
                    continue;
                }
                wrap_pending_line(&mut lines, &mut current, paragraph);
            }
            PreparedParagraphItem::Atom { atom_index, .. } => {
                current.push(
                    PendingLineItem::Atom {
                        atom_index: *atom_index,
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
        lines.push(current.take());
    }
    lines
}

fn should_keep_current_line(current: &PendingLine, max_width: f32) -> bool {
    current.width <= max_width || current.items.len() == 1
}

fn wrap_pending_line(
    lines: &mut Vec<Vec<PendingLineItem>>,
    current: &mut PendingLine,
    paragraph: &PreparedParagraph,
) {
    if let Some(break_len) = current
        .last_allowed_break
        .filter(|break_len| *break_len < current.items.len())
    {
        let remainder = current.split_off(break_len, paragraph);
        lines.push(current.take());
        *current = PendingLine::from_items(remainder, paragraph);
    } else if let Some(overflow) = current.pop_last(paragraph) {
        lines.push(current.take());
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
            .is_some_and(|item| item.break_after(paragraph) == BreakOpportunity::Allowed)
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

    fn take(&mut self) -> Vec<PendingLineItem> {
        self.width = 0.0;
        self.last_allowed_break = None;
        std::mem::take(&mut self.items)
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
            .find(|(_, item)| item.break_after(paragraph) == BreakOpportunity::Allowed)
            .map(|(index, _)| index + 1);
    }
}

fn place_lines(
    raw_lines: &[Vec<PendingLineItem>],
    paragraph: &PreparedParagraph,
    rect: LayoutRect,
) -> Vec<LayoutLine> {
    let mut y = rect.y();
    raw_lines
        .iter()
        .map(|items| {
            let (ascent, descent) = line_metrics(items, paragraph);
            let line_height = paragraph.default_line_height.max(ascent + descent);
            let baseline = y + ascent;
            let runs = build_runs(items, paragraph, rect.x(), y, baseline, line_height);
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
            PendingLineItem::Atom { atom_index } => {
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
) -> Vec<LayoutRun> {
    let mut runs = Vec::new();
    let mut x = line_x;
    let mut current: Option<TextRunAccumulator> = None;

    for item in items {
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
            PendingLineItem::Atom { atom_index } => {
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
                runs.push(LayoutRun::Atom(LayoutAtomRun {
                    rect,
                    payload: layout_atom_payload(atom, rect),
                }));
                x += atom.outer_width();
            }
        }
    }

    if let Some(run) = current {
        runs.push(LayoutRun::Text(run.finish(line_y, baseline, line_height)));
    }
    runs
}

fn layout_atom_payload(atom: &PreparedInlineAtom, rect: LayoutRect) -> LayoutAtomPayload {
    match &atom.payload {
        PreparedAtomPayload::Chip {
            background,
            measured_text,
            ..
        } => {
            let mut x = rect.x() + atom.style.padding.left;
            let inner_baseline = rect.y()
                + atom.style.padding.top
                + measured_text
                    .iter()
                    .map(|glyph| glyph.ascent)
                    .fold(0.0, f32::max);
            let glyphs = measured_text
                .iter()
                .map(|glyph| {
                    let positioned = position_glyph(glyph, x, inner_baseline);
                    x += glyph.advance.max(0.0);
                    positioned
                })
                .collect();
            LayoutAtomPayload::Chip {
                background: background.or(atom.style.background),
                glyphs,
            }
        }
        PreparedAtomPayload::Icon { glyph } => LayoutAtomPayload::Icon {
            glyph: position_glyph(
                glyph,
                rect.x() + atom.style.padding.left,
                rect.y() + atom.style.padding.top + glyph.ascent,
            ),
        },
        PreparedAtomPayload::Image { data_ref } => LayoutAtomPayload::Image {
            data_ref: data_ref.clone(),
        },
        PreparedAtomPayload::Custom => LayoutAtomPayload::Custom,
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
            end_x: start_x + glyph.advance.max(0.0),
            glyphs: vec![positioned],
        }
    }

    fn push(&mut self, glyph: &PreparedGlyph, positioned: PositionedGlyph) {
        self.end_x += glyph.advance.max(0.0);
        self.glyphs.push(positioned);
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
    use crate::layout::tree::{AtomBaseline, InlineAtomStyle, ParagraphStyle};

    use super::{layout_paragraph, LayoutRect};

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
