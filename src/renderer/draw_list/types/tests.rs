use std::sync::Arc;

use crate::font::SubpixelBin;

use super::{
    BlockDrawGroup, ClipRect, ImageCmd, ImageData, LineCap, LineJoin, PathCmd, PathVerb,
    PositionedGlyph, RenderLayer, StrokeStyle,
};

#[test]
fn block_draw_group_routes_paths_and_images_by_layer() {
    let mut group = BlockDrawGroup::new(2, 1, Some(ClipRect::new(0.0, 0.0, 20.0, 20.0)));
    let curved_path = PathCmd::new(
        vec![
            PathVerb::MoveTo { to: [0.0, 0.0] },
            PathVerb::QuadTo {
                ctrl: [4.0, 8.0],
                to: [8.0, 0.0],
            },
            PathVerb::Close,
        ],
        Some([1.0, 0.0, 0.0, 1.0]),
        Some(StrokeStyle::new(
            [1.0, 1.0, 1.0, 1.0],
            2.0,
            LineCap::Round,
            LineJoin::Round,
        )),
        RenderLayer::Foreground,
    );
    let cubic_path = PathCmd::new(
        vec![
            PathVerb::MoveTo { to: [1.0, 1.0] },
            PathVerb::CubicTo {
                ctrl1: [2.0, 6.0],
                ctrl2: [6.0, 6.0],
                to: [8.0, 1.0],
            },
            PathVerb::Close,
        ],
        None,
        Some(StrokeStyle::new(
            [0.0, 1.0, 0.0, 1.0],
            3.0,
            LineCap::Square,
            LineJoin::Bevel,
        )),
        RenderLayer::Overlay,
    );
    let image_data = Arc::new(ImageData::new(vec![255, 0, 0, 255], 1, 1));
    let image = ImageCmd::new(
        [2.0, 3.0],
        [4.0, 5.0],
        image_data.clone(),
        RenderLayer::Overlay,
    );

    assert!(image_data.is_valid());
    assert_ne!(curved_path.content_hash(), cubic_path.content_hash());

    group.push_path(curved_path);
    group.push_path(cubic_path);
    group.push_image(image.clone());

    assert_eq!(image.layer(), RenderLayer::Overlay);
    assert_eq!(image.data().width(), 1);
    assert_eq!(image.data().height(), 1);
    assert_eq!(group.layer(RenderLayer::Foreground).paths().len(), 1);
    assert_eq!(group.layer(RenderLayer::Overlay).paths().len(), 1);
    assert_eq!(group.layer(RenderLayer::Overlay).images().len(), 1);
}

#[test]
fn block_draw_group_keeps_glyphs_in_requested_layer() {
    let mut group = BlockDrawGroup::new(0, 0, Some(ClipRect::new(0.0, 0.0, 10.0, 10.0)));
    let glyph = PositionedGlyph {
        font_id: 0,
        glyph_id: 7,
        font_size: 14.0,
        pos: [1.0, 2.0],
        color: [1.0, 1.0, 1.0, 1.0],
        subpixel_offset: SubpixelBin::new(0, 0),
    };

    group.extend_glyphs(RenderLayer::Content, vec![glyph]);
    assert_eq!(group.layer(RenderLayer::Content).glyphs()[0].glyph_id, 7);
}
