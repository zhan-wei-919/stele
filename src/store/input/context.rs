//! Input context selection for store-owned default mappings.

use super::super::model::Model;
use super::super::types::InteractionState;

/// Store-owned target that receives default input command resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InputContext {
    Viewport,
    TextInput(TextInputId),
}

/// Stable store-local identity for a text input target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TextInputId(u64);

impl TextInputId {
    /// Creates a text input identity.
    // The current slice consumes explicit focus but does not yet include the production focus owner.
    // Keep construction crate-local so that owner can set focus without exposing the raw id.
    #[allow(dead_code)]
    pub(crate) fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Resolves the active store input context from model and interaction state.
pub(crate) fn resolve_input_context(
    _model: &Model,
    interaction: &InteractionState,
) -> InputContext {
    match interaction.focused_text_input {
        Some(text_input) => InputContext::TextInput(text_input),
        None => InputContext::Viewport,
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::tree::{BlockNode, BlockStyle, DocumentTree, FlowDirection, StackNode};
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
        let model = Model::new(empty_document());
        let text_input = TextInputId::new(1);
        let mut interaction = InteractionState::default();
        interaction.focused_text_input = Some(text_input);

        assert_eq!(
            resolve_input_context(&model, &interaction),
            InputContext::TextInput(text_input)
        );
    }

    fn empty_document() -> DocumentTree {
        let root = StackNode::new(FlowDirection::Vertical, Vec::new(), BlockStyle::default())
            .expect("empty stack must be valid");
        DocumentTree::new(BlockNode::Stack(root)).expect("empty document must be valid")
    }
}
