//! Input context selection for store-owned default mappings.

use super::super::model::Model;
use super::super::types::InteractionState;
use crate::layout::tree::TextInputId;

/// Store-owned target that receives default input command resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InputContext {
    Viewport,
    TextInput(TextInputId),
}

/// Resolves the active store input context from model and interaction state.
pub(crate) fn resolve_input_context(model: &Model, interaction: &InteractionState) -> InputContext {
    match interaction.focused_text_input {
        Some(text_input) if model.text_inputs().contains(text_input) => {
            InputContext::TextInput(text_input)
        }
        None => InputContext::Viewport,
        Some(_) => InputContext::Viewport,
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::tree::{
        BlockNode, BlockStyle, DocumentTree, FlowDirection, StackNode, TextInputId, TextInputNode,
        TextInputStyle, TextStyle,
    };
    use crate::store::model::Model;
    use crate::store::types::InteractionState;

    use super::*;

    #[test]
    fn default_context_resolves_to_viewport() {
        let model = Model::new(empty_document());
        let interaction = InteractionState::default();

        assert_eq!(
            resolve_input_context(&model, &interaction),
            InputContext::Viewport
        );
    }

    #[test]
    fn focused_text_input_resolves_to_text_input_context() {
        let text_input = TextInputId::new(1);
        let model = Model::new(document_with_text_input(text_input));
        let mut interaction = InteractionState::default();
        interaction.focused_text_input = Some(text_input);

        assert_eq!(
            resolve_input_context(&model, &interaction),
            InputContext::TextInput(text_input)
        );
    }

    #[test]
    fn stale_focused_text_input_resolves_to_viewport() {
        let model = Model::new(empty_document());
        let text_input = TextInputId::new(1);
        let mut interaction = InteractionState::default();
        interaction.focused_text_input = Some(text_input);

        assert_eq!(
            resolve_input_context(&model, &interaction),
            InputContext::Viewport
        );
    }

    fn empty_document() -> DocumentTree {
        let root = StackNode::new(FlowDirection::Vertical, Vec::new(), BlockStyle::default())
            .expect("empty stack must be valid");
        DocumentTree::new(BlockNode::Stack(root)).expect("empty document must be valid")
    }

    fn document_with_text_input(text_input: TextInputId) -> DocumentTree {
        let text_style =
            TextStyle::new(0, 14.0, [1.0, 1.0, 1.0, 1.0]).expect("style must be valid");
        let input = TextInputNode::new(
            text_input,
            "placeholder",
            text_style,
            TextInputStyle::default(),
        )
        .expect("text input must be valid");
        let root = StackNode::new(
            FlowDirection::Vertical,
            vec![BlockNode::TextInput(input)],
            BlockStyle::default(),
        )
        .expect("stack must be valid");
        DocumentTree::new(BlockNode::Stack(root)).expect("document must be valid")
    }
}
