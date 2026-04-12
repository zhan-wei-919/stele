use std::sync::Arc;

use crate::font::SubpixelBin;

use super::{
    ImageCmd, ImageData, LineCap, LineJoin, PathCmd, PathVerb, PositionedGlyph, RenderLayer,
    StrokeStyle,
};

#[test]
fn positioned_glyph_builds_scale_sensitive_glyph_key() {
    let glyph = PositionedGlyph {
        font_id: 3,
        glyph_id: 42,
        font_size: 14.0,
        pos: [1.25, 2.5],
        color: [1.0, 1.0, 1.0, 1.0],
        subpixel_offset: SubpixelBin::new(1, 2),
    };

    let key = glyph.glyph_key(2.0);
    assert_eq!(key.font_id, 3);
    assert_eq!(key.glyph_id, 42);
    assert_eq!(key.font_size(), 14.0);
    assert_eq!(key.scale_factor(), 2.0);
    assert_eq!(key.subpixel_offset, SubpixelBin::new(1, 2));
}

#[test]
fn path_hash_changes_with_geometry_and_style() {
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

    assert_ne!(curved_path.content_hash(), cubic_path.content_hash());
}

#[test]
fn image_cmd_preserves_validated_payload_and_layer() {
    let image_data = Arc::new(ImageData::new(vec![255, 0, 0, 255], 1, 1));
    let image = ImageCmd::new(
        [2.0, 3.0],
        [4.0, 5.0],
        image_data.clone(),
        RenderLayer::Overlay,
    );

    assert!(image_data.is_valid());
    assert_eq!(image.layer(), RenderLayer::Overlay);
    assert_eq!(image.data().width(), 1);
    assert_eq!(image.data().height(), 1);
}
