use std::sync::Arc;

use stele::ui::{
    BlockNode, BlockStyle, DocumentTree, FlowDirection, FontDiscovery, FreeTypeRasterizer,
    InlineNode, Model, ParagraphNode, ParagraphStyle, StackNode, Store, StoreBootstrap,
    StoreDelegate, SubpixelLayout, TextRun, TextStyle, ViewportState,
};

#[test]
fn ui_facade_builds_document_and_store() {
    let rasterizer = build_rasterizer();
    let viewport = ViewportState::new(320, 240, 1.0, 0, None);
    let store = Store::new(rasterizer, viewport, Arc::new(SmokeDelegate));

    let _ = std::mem::size_of_val(&store);
}

struct SmokeDelegate;

impl StoreDelegate for SmokeDelegate {
    fn bootstrap(
        &self,
        rasterizer: &FreeTypeRasterizer,
        _logical_viewport: [f32; 2],
    ) -> StoreBootstrap {
        StoreBootstrap::new(build_document(rasterizer.default_font_id()), rasterizer)
    }

    fn resize(&self, _model: &mut Model, _logical_viewport: [f32; 2]) {}
}

fn build_document(font_id: u32) -> DocumentTree {
    let text_style =
        TextStyle::new(font_id, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
    let paragraph = BlockNode::Paragraph(
        ParagraphNode::new(
            vec![InlineNode::Text(TextRun::new(
                "hello from stele",
                text_style,
            ))],
            ParagraphStyle::default(),
        )
        .expect("paragraph must be valid"),
    );
    let root = BlockNode::Stack(
        StackNode::new(
            FlowDirection::Vertical,
            vec![paragraph],
            BlockStyle::default(),
        )
        .expect("stack must be valid"),
    );
    DocumentTree::new(root).expect("document must be valid")
}

fn build_rasterizer() -> FreeTypeRasterizer {
    FreeTypeRasterizer::new(
        FontDiscovery::new().expect("system fonts must be available"),
        SubpixelLayout::None,
    )
    .expect("rasterizer must initialize")
}
