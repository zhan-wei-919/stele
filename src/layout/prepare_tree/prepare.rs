//! Prepare-stage traversal for the rich-text layout tree.

use std::collections::HashMap;

use log::{info, warn};

use crate::font::{FontSelection, FreeTypeRasterizer, LineMetrics, MeasuredGlyph};
use crate::layout::line_break::{collect_breaks, BreakOpportunity};
use crate::layout::prepare::PreparedGlyph;
use crate::layout::tree::{
    BlockEmbedKind, BlockEmbedNode, BlockNode, DocumentTree, FlowDirection, InlineAtom,
    InlineAtomKind, InlineNode, OverlayNode, ParagraphNode, StackNode, TextStyle,
};

use super::super::tree::{AnchorKey, NodeId};
use super::embed::{PreparedEmbed, PreparedEmbedPayload};
use super::paragraph::{
    PreparedAtomPayload, PreparedInline, PreparedInlineAtom, PreparedInlineText, PreparedParagraph,
    PreparedParagraphItem,
};
use super::{PreparedBlockNode, PreparedOverlay, PreparedStack, PreparedTree};

const DEFAULT_FONT_SIZE: f32 = 14.0;
const DEFAULT_LINE_HEIGHT_FACTOR: f32 = 1.4;
const OBJECT_REPLACEMENT_CHAR: char = '\u{FFFC}';

#[derive(Default)]
struct PrepareStats {
    node_count: usize,
    paragraph_count: usize,
    atom_count: usize,
    embed_count: usize,
    default_ascent: f32,
    default_line_height: f32,
}

/// Measures the full rich-text tree once and returns the cached cold-path data.
pub(crate) fn prepare_tree(
    document: &DocumentTree,
    rasterizer: &FreeTypeRasterizer,
) -> PreparedTree {
    let mut stats = PrepareStats {
        default_ascent: fallback_line_metrics(DEFAULT_FONT_SIZE).ascent,
        default_line_height: fallback_line_metrics(DEFAULT_FONT_SIZE).line_height,
        ..PrepareStats::default()
    };
    let root = prepare_block(document.root(), rasterizer, &mut stats);
    info!(
        "layout.tree.prepare node_count={} paragraph_count={} atom_count={} embed_count={}",
        stats.node_count, stats.paragraph_count, stats.atom_count, stats.embed_count
    );
    PreparedTree {
        root,
        anchor_index: document.anchor_index().clone(),
        default_ascent: stats.default_ascent,
        default_line_height: stats.default_line_height,
    }
}

fn prepare_block(
    node: &BlockNode,
    rasterizer: &FreeTypeRasterizer,
    stats: &mut PrepareStats,
) -> PreparedBlockNode {
    stats.node_count += 1;
    match node {
        BlockNode::Stack(stack) => {
            PreparedBlockNode::Stack(prepare_stack(stack, rasterizer, stats))
        }
        BlockNode::Paragraph(paragraph) => {
            stats.paragraph_count += 1;
            PreparedBlockNode::Paragraph(prepare_paragraph(paragraph, rasterizer, stats))
        }
        BlockNode::Embed(embed) => {
            stats.embed_count += 1;
            PreparedBlockNode::Embed(prepare_embed(embed))
        }
        BlockNode::Overlay(overlay) => {
            PreparedBlockNode::Overlay(prepare_overlay(overlay, rasterizer, stats))
        }
    }
}

fn prepare_stack(
    stack: &StackNode,
    rasterizer: &FreeTypeRasterizer,
    stats: &mut PrepareStats,
) -> PreparedStack {
    PreparedStack {
        node_id: stack.node_id,
        anchor_key: stack.anchor_key.clone(),
        direction: stack.direction,
        children: stack
            .children
            .iter()
            .map(|child| prepare_block(child, rasterizer, stats))
            .collect(),
        style: stack.style,
    }
}

fn prepare_overlay(
    overlay: &OverlayNode,
    rasterizer: &FreeTypeRasterizer,
    stats: &mut PrepareStats,
) -> PreparedOverlay {
    PreparedOverlay {
        node_id: overlay.node_id,
        anchor: overlay.anchor.clone(),
        child: Box::new(prepare_block(overlay.child.as_ref(), rasterizer, stats)),
    }
}

fn prepare_paragraph(
    paragraph: &ParagraphNode,
    rasterizer: &FreeTypeRasterizer,
    stats: &mut PrepareStats,
) -> PreparedParagraph {
    let mut pending_inlines = Vec::with_capacity(paragraph.inlines.len());
    let mut staged_items = Vec::new();
    let mut full_text = String::new();
    let mut default_ascent = fallback_line_metrics(DEFAULT_FONT_SIZE).ascent;
    let mut default_line_height = fallback_line_metrics(DEFAULT_FONT_SIZE).line_height;

    for (inline_index, inline) in paragraph.inlines.iter().enumerate() {
        match inline {
            InlineNode::Text(text) => {
                let glyphs = stage_text_inline(
                    inline_index,
                    text.text.as_str(),
                    text.style,
                    rasterizer,
                    &mut full_text,
                    &mut staged_items,
                    &mut default_ascent,
                    &mut default_line_height,
                );
                pending_inlines.push(PendingInline::Text {
                    run_index: inline_index,
                    style: text.style,
                    glyph_count: glyphs,
                });
            }
            InlineNode::Atom(atom) => {
                stats.atom_count += 1;
                let prepared_atom = prepare_inline_atom(inline_index, atom, rasterizer);
                default_ascent = default_ascent.max(atom_ascent(&prepared_atom));
                default_line_height = default_line_height
                    .max(atom_ascent(&prepared_atom) + atom_descent(&prepared_atom));
                full_text.push(OBJECT_REPLACEMENT_CHAR);
                staged_items.push(StagedParagraphItem::Atom {
                    byte_end: full_text.len(),
                    atom_index: inline_index,
                });
                pending_inlines.push(PendingInline::Atom(prepared_atom));
            }
        }
    }

    let break_map = collect_breaks(&full_text);
    let items = staged_items
        .into_iter()
        .map(|item| item.into_prepared(&break_map))
        .collect::<Vec<_>>();
    let glyphs_by_run = collect_glyphs_by_run(&items);
    let inlines = pending_inlines
        .into_iter()
        .map(|inline| inline.finish(&glyphs_by_run))
        .collect::<Vec<_>>();
    let resolved_line_height = paragraph.style.line_height.resolve(default_line_height);

    stats.default_ascent = stats.default_ascent.max(default_ascent);
    stats.default_line_height = stats.default_line_height.max(resolved_line_height);

    PreparedParagraph {
        node_id: paragraph.node_id,
        anchor_key: paragraph.anchor_key.clone(),
        inlines,
        items,
        break_map,
        default_ascent,
        default_line_height: resolved_line_height,
        style: paragraph.style,
    }
}

fn stage_text_inline(
    run_index: usize,
    text: &str,
    style: TextStyle,
    rasterizer: &FreeTypeRasterizer,
    full_text: &mut String,
    staged_items: &mut Vec<StagedParagraphItem>,
    default_ascent: &mut f32,
    default_line_height: &mut f32,
) -> usize {
    let font_selection = rasterizer.resolve_font(style.font_id(), style.bold(), style.italic());
    log_font_fallback(font_selection);
    let metrics = rasterizer.line_metrics(font_selection.resolved_font_id, style.font_size());
    *default_ascent = default_ascent.max(metrics.ascent);
    *default_line_height = default_line_height.max(metrics.line_height);
    let measured_glyphs = rasterizer.measure_text(
        text,
        font_selection.resolved_font_id,
        style.font_size(),
        style.letter_spacing(),
    );

    let mut measured_index = 0usize;
    for ch in text.chars() {
        full_text.push(ch);
        if ch == '\n' {
            staged_items.push(StagedParagraphItem::Break);
            continue;
        }
        let measured = measured_glyphs
            .get(measured_index)
            .copied()
            .unwrap_or_else(|| MeasuredGlyph::fallback(style.font_size(), style.letter_spacing()));
        measured_index += 1;
        staged_items.push(StagedParagraphItem::Glyph {
            byte_end: full_text.len(),
            glyph: PreparedGlyph::from_measurement(
                run_index,
                style,
                font_selection,
                metrics,
                measured,
            ),
        });
    }

    measured_index
}

fn prepare_inline_atom(
    atom_index: usize,
    atom: &InlineAtom,
    rasterizer: &FreeTypeRasterizer,
) -> PreparedInlineAtom {
    match &atom.kind {
        InlineAtomKind::Chip { label, text_style } => {
            let font_selection = rasterizer.resolve_font(
                text_style.font_id(),
                text_style.bold(),
                text_style.italic(),
            );
            log_font_fallback(font_selection);
            let metrics =
                rasterizer.line_metrics(font_selection.resolved_font_id, text_style.font_size());
            let measured = rasterizer.measure_text(
                label,
                font_selection.resolved_font_id,
                text_style.font_size(),
                text_style.letter_spacing(),
            );
            let measured_text = measured
                .into_iter()
                .map(|glyph| {
                    PreparedGlyph::from_measurement(
                        atom_index,
                        *text_style,
                        font_selection,
                        metrics,
                        glyph,
                    )
                })
                .collect::<Vec<_>>();
            let label_width = measured_text
                .iter()
                .map(|glyph| glyph.advance.max(0.0))
                .sum::<f32>();
            PreparedInlineAtom {
                atom_index,
                intrinsic_size: [
                    label_width + atom.style.padding.horizontal(),
                    metrics.line_height + atom.style.padding.vertical(),
                ],
                baseline: atom.style.baseline,
                style: atom.style,
                payload: PreparedAtomPayload::Chip {
                    foreground: text_style.color(),
                    background: atom.style.background,
                    measured_text,
                },
            }
        }
        InlineAtomKind::Icon {
            glyph_id,
            font_id,
            size,
            color,
        } => {
            let metrics = rasterizer.line_metrics(*font_id, *size);
            let glyph = PreparedGlyph {
                span_index: atom_index,
                font_id: *font_id,
                glyph_id: *glyph_id,
                font_size: *size,
                advance: (*size).max(1.0),
                ascent: metrics.ascent,
                line_height: metrics.line_height,
                color: *color,
                background_color: None,
                underline: false,
                strikethrough: false,
                break_after: BreakOpportunity::Forbidden,
            };
            PreparedInlineAtom {
                atom_index,
                intrinsic_size: [
                    glyph.advance + atom.style.padding.horizontal(),
                    metrics.line_height + atom.style.padding.vertical(),
                ],
                baseline: atom.style.baseline,
                style: atom.style,
                payload: PreparedAtomPayload::Icon { glyph },
            }
        }
        InlineAtomKind::Image { data_ref } => PreparedInlineAtom {
            atom_index,
            intrinsic_size: [
                data_ref.width() as f32 + atom.style.padding.horizontal(),
                data_ref.height() as f32 + atom.style.padding.vertical(),
            ],
            baseline: atom.style.baseline,
            style: atom.style,
            payload: PreparedAtomPayload::Image {
                data_ref: data_ref.clone(),
            },
        },
        InlineAtomKind::Custom { measured_size } => PreparedInlineAtom {
            atom_index,
            intrinsic_size: [
                measured_size[0] + atom.style.padding.horizontal(),
                measured_size[1] + atom.style.padding.vertical(),
            ],
            baseline: atom.style.baseline,
            style: atom.style,
            payload: PreparedAtomPayload::Custom,
        },
    }
}

fn prepare_embed(embed: &BlockEmbedNode) -> PreparedEmbed {
    match &embed.kind {
        BlockEmbedKind::Image {
            data_ref,
            intrinsic_size,
        } => PreparedEmbed {
            node_id: embed.node_id,
            anchor_key: embed.anchor_key.clone(),
            intrinsic_size: *intrinsic_size,
            style: embed.style,
            payload: PreparedEmbedPayload::Image {
                data_ref: data_ref.clone(),
            },
        },
        BlockEmbedKind::Path {
            verbs,
            fill,
            stroke,
            intrinsic_size,
        } => PreparedEmbed {
            node_id: embed.node_id,
            anchor_key: embed.anchor_key.clone(),
            intrinsic_size: *intrinsic_size,
            style: embed.style,
            payload: PreparedEmbedPayload::Path {
                verbs: verbs.clone(),
                fill: *fill,
                stroke: *stroke,
            },
        },
        BlockEmbedKind::Custom { intrinsic_size } => PreparedEmbed {
            node_id: embed.node_id,
            anchor_key: embed.anchor_key.clone(),
            intrinsic_size: *intrinsic_size,
            style: embed.style,
            payload: PreparedEmbedPayload::Custom,
        },
    }
}

fn atom_ascent(atom: &PreparedInlineAtom) -> f32 {
    let outer_height = atom.outer_height();
    match atom.baseline {
        super::super::tree::AtomBaseline::AlphabeticAlignedToLine
        | super::super::tree::AtomBaseline::Bottom => outer_height,
        super::super::tree::AtomBaseline::MiddleOfLine => outer_height * 0.5,
        super::super::tree::AtomBaseline::Top => 0.0,
    }
}

fn atom_descent(atom: &PreparedInlineAtom) -> f32 {
    let outer_height = atom.outer_height();
    match atom.baseline {
        super::super::tree::AtomBaseline::AlphabeticAlignedToLine => 0.0,
        super::super::tree::AtomBaseline::MiddleOfLine => outer_height * 0.5,
        super::super::tree::AtomBaseline::Top => outer_height,
        super::super::tree::AtomBaseline::Bottom => 0.0,
    }
}

fn collect_glyphs_by_run(items: &[PreparedParagraphItem]) -> HashMap<usize, Vec<PreparedGlyph>> {
    let mut glyphs_by_run = HashMap::<usize, Vec<PreparedGlyph>>::new();
    for item in items {
        if let PreparedParagraphItem::Glyph(glyph) = item {
            glyphs_by_run
                .entry(glyph.span_index)
                .or_default()
                .push(glyph.clone());
        }
    }
    glyphs_by_run
}

fn fallback_line_metrics(font_size: f32) -> LineMetrics {
    LineMetrics {
        ascent: font_size,
        line_height: font_size * DEFAULT_LINE_HEIGHT_FACTOR,
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

enum PendingInline {
    Text {
        run_index: usize,
        style: TextStyle,
        glyph_count: usize,
    },
    Atom(PreparedInlineAtom),
}

impl PendingInline {
    fn finish(self, glyphs_by_run: &HashMap<usize, Vec<PreparedGlyph>>) -> PreparedInline {
        match self {
            Self::Text {
                run_index,
                style,
                glyph_count,
            } => {
                let glyphs = glyphs_by_run.get(&run_index).cloned().unwrap_or_default();
                debug_assert_eq!(glyphs.len(), glyph_count);
                PreparedInline::Text(PreparedInlineText {
                    run_index,
                    glyphs,
                    style,
                })
            }
            Self::Atom(atom) => PreparedInline::Atom(atom),
        }
    }
}

enum StagedParagraphItem {
    Glyph {
        byte_end: usize,
        glyph: PreparedGlyph,
    },
    Atom {
        byte_end: usize,
        atom_index: usize,
    },
    Break,
}

impl StagedParagraphItem {
    fn into_prepared(self, break_map: &HashMap<usize, BreakOpportunity>) -> PreparedParagraphItem {
        match self {
            Self::Glyph {
                byte_end,
                mut glyph,
            } => {
                glyph.break_after = break_map
                    .get(&byte_end)
                    .copied()
                    .unwrap_or(BreakOpportunity::Forbidden);
                PreparedParagraphItem::Glyph(glyph)
            }
            Self::Atom {
                byte_end,
                atom_index,
            } => PreparedParagraphItem::Atom {
                atom_index,
                break_after: break_map
                    .get(&byte_end)
                    .copied()
                    .unwrap_or(BreakOpportunity::Forbidden),
            },
            Self::Break => PreparedParagraphItem::Break(BreakOpportunity::Mandatory),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::draw_list::ImageData;
    use crate::font::{FontDiscovery, FreeTypeRasterizer};
    use crate::layout::tree::{
        BlockNode, DocumentTree, FlowDirection, InlineAtom, InlineAtomKind, InlineAtomStyle,
        InlineNode, ParagraphNode, ParagraphStyle, StackNode, TextRun, TextStyle,
    };
    use crate::renderer::subpixel::detect_subpixel_layout;

    use super::prepare_tree;

    fn rasterizer() -> FreeTypeRasterizer {
        let font_discovery = FontDiscovery::new().expect("fonts must exist");
        FreeTypeRasterizer::new(font_discovery, detect_subpixel_layout())
            .expect("rasterizer must initialize")
    }

    #[test]
    fn prepares_paragraph_text_and_atoms_without_losing_order() {
        let body = TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        let atom = InlineAtom::new(
            InlineAtomKind::Image {
                data_ref: Arc::new(ImageData::new(vec![255; 4], 1, 1)),
            },
            InlineAtomStyle::default(),
        )
        .expect("atom must be valid");
        let paragraph = ParagraphNode::new(
            vec![
                InlineNode::Text(TextRun::new("a", body)),
                InlineNode::Atom(atom),
                InlineNode::Text(TextRun::new("b", body)),
            ],
            ParagraphStyle::default(),
        )
        .expect("paragraph must be valid");
        let tree = DocumentTree::new(BlockNode::Stack(
            StackNode::new(
                FlowDirection::Vertical,
                vec![BlockNode::Paragraph(paragraph)],
                crate::layout::tree::BlockStyle::default(),
            )
            .expect("stack must be valid"),
        ))
        .expect("tree must be valid");

        let prepared = prepare_tree(&tree, &rasterizer());
        let super::PreparedBlockNode::Stack(root) = prepared.root else {
            panic!("root must be stack");
        };
        let super::PreparedBlockNode::Paragraph(paragraph) = &root.children[0] else {
            panic!("child must be paragraph");
        };
        assert_eq!(paragraph.items.len(), 3);
    }
}
