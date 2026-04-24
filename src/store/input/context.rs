//! Input context selection for store-owned default mappings.

use super::super::model::Model;
use super::super::types::InteractionState;

/// Store-owned target that receives default input command resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InputContext {
    Viewport,
    // Reserved until focus plumbing can produce text-input contexts.
    #[cfg_attr(not(test), allow(dead_code))]
    TextInput(TextInputId),
}

/// Stable store-local identity for a text input target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TextInputId(u64);

impl TextInputId {
    /// Creates a text input identity for tests and future focus plumbing.
    #[cfg(test)]
    pub(crate) fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Resolves the active store input context from model and interaction state.
pub(crate) fn resolve_input_context(
    _model: &Model,
    _interaction: &InteractionState,
) -> InputContext {
    InputContext::Viewport
}

#[cfg(test)]
mod tests {
    use crate::layout::tree::{BlockNode, BlockStyle, DocumentTree, FlowDirection, StackNode};
    use crate::store::model::Model;
    use crate::store::types::InteractionState;

    use super::*;

    #[test]
    fn current_context_resolves_to_viewport() {
        let model = Model::new(empty_document());
        let interaction = InteractionState::default();

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
}
