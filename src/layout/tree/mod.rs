//! Semantic rich-text input tree for the new layout pipeline.

mod nodes;
mod render;
mod style;
mod text_input;
mod text_style;
mod validation;

pub use nodes::{
    AnchorKey, BlockEmbedKind, BlockEmbedNode, BlockNode, DocumentTree, FlowDirection, InlineAtom,
    InlineAtomKind, InlineNode, NodeId, OverlayAnchor, OverlayNode, ParagraphNode, StackNode,
    TextInputId, TextInputNode, TextRun,
};
pub(crate) use render::validate_local_paint_commands;
pub use render::{LocalPaintCommand, PathStroke};
pub use style::{
    Align, AtomBaseline, BlockStyle, BorderStyle, ClipMode, Edges, InlineAtomStyle, LineHeight,
    ParagraphStyle, TextAlign, TextInputStyle, WrapMode,
};
pub use text_input::{is_insertable_text_input_char, single_line_text};
pub use text_style::TextStyle;
