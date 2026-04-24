//! Store invalidation flags that drive prepare and compose work.

/// Rendering and prepare work required after one state transition.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Invalidation {
    needs_compose: bool,
    needs_reprepare: bool,
    resets_atlas: bool,
}

impl Invalidation {
    pub(crate) const NONE: Self = Self {
        needs_compose: false,
        needs_reprepare: false,
        resets_atlas: false,
    };
    pub(crate) const RECOMPOSE: Self = Self {
        needs_compose: true,
        needs_reprepare: false,
        resets_atlas: false,
    };
    pub(crate) const REPREPARE_AND_COMPOSE: Self = Self {
        needs_compose: true,
        needs_reprepare: true,
        resets_atlas: false,
    };
    pub(crate) const RESET_ATLAS_AND_COMPOSE: Self = Self {
        needs_compose: true,
        needs_reprepare: false,
        resets_atlas: true,
    };
    #[cfg(test)]
    pub(crate) const REPREPARE_RESET_ATLAS_AND_COMPOSE: Self = Self {
        needs_compose: true,
        needs_reprepare: true,
        resets_atlas: true,
    };

    /// Returns the union of work requested by two pending transitions.
    pub(crate) fn merge(self, other: Self) -> Self {
        Self {
            needs_compose: self.needs_compose || other.needs_compose,
            needs_reprepare: self.needs_reprepare || other.needs_reprepare,
            resets_atlas: self.resets_atlas || other.resets_atlas,
        }
    }

    /// Returns the remaining compose work after prepared layout data has been refreshed early.
    pub(crate) fn with_reprepare_consumed(self) -> Self {
        Self {
            needs_compose: self.needs_compose,
            needs_reprepare: false,
            resets_atlas: self.resets_atlas,
        }
    }

    /// Returns whether this transition needs a fresh scene composition.
    pub(crate) fn needs_compose(self) -> bool {
        self.needs_compose
    }

    /// Returns whether cold prepared layout data must be rebuilt before compose.
    pub(crate) fn needs_reprepare(self) -> bool {
        self.needs_reprepare
    }

    /// Returns whether scale-sensitive atlas and tessellation caches must be reset.
    pub(crate) fn resets_atlas(self) -> bool {
        self.resets_atlas
    }
}

#[cfg(test)]
mod tests {
    use super::Invalidation;

    #[test]
    fn merge_unions_independent_invalidation_flags() {
        assert_eq!(
            Invalidation::NONE.merge(Invalidation::RECOMPOSE),
            Invalidation::RECOMPOSE
        );
        assert_eq!(
            Invalidation::REPREPARE_AND_COMPOSE.merge(Invalidation::RECOMPOSE),
            Invalidation::REPREPARE_AND_COMPOSE
        );
        assert_eq!(
            Invalidation::REPREPARE_AND_COMPOSE.merge(Invalidation::RESET_ATLAS_AND_COMPOSE),
            Invalidation::REPREPARE_RESET_ATLAS_AND_COMPOSE
        );
    }

    #[test]
    fn only_non_empty_invalidations_need_compose() {
        assert!(!Invalidation::NONE.needs_compose());
        assert!(Invalidation::RECOMPOSE.needs_compose());
        assert!(Invalidation::REPREPARE_AND_COMPOSE.needs_compose());
        assert!(Invalidation::RESET_ATLAS_AND_COMPOSE.needs_compose());
    }

    #[test]
    fn reprepare_is_independent_from_atlas_reset() {
        assert!(!Invalidation::NONE.needs_reprepare());
        assert!(!Invalidation::RECOMPOSE.needs_reprepare());
        assert!(Invalidation::REPREPARE_AND_COMPOSE.needs_reprepare());
        assert!(!Invalidation::RESET_ATLAS_AND_COMPOSE.needs_reprepare());
        assert!(Invalidation::REPREPARE_RESET_ATLAS_AND_COMPOSE.needs_reprepare());
    }

    #[test]
    fn consumed_reprepare_preserves_remaining_compose_work() {
        assert_eq!(
            Invalidation::REPREPARE_AND_COMPOSE.with_reprepare_consumed(),
            Invalidation::RECOMPOSE
        );
        assert_eq!(
            Invalidation::REPREPARE_RESET_ATLAS_AND_COMPOSE.with_reprepare_consumed(),
            Invalidation::RESET_ATLAS_AND_COMPOSE
        );
    }

    #[test]
    fn reset_atlas_is_independent_from_reprepare() {
        assert!(!Invalidation::NONE.resets_atlas());
        assert!(!Invalidation::RECOMPOSE.resets_atlas());
        assert!(!Invalidation::REPREPARE_AND_COMPOSE.resets_atlas());
        assert!(Invalidation::RESET_ATLAS_AND_COMPOSE.resets_atlas());
        assert!(Invalidation::REPREPARE_RESET_ATLAS_AND_COMPOSE.resets_atlas());
    }
}
