//! Unicode line-break helpers for prepared inline content.

use std::collections::HashMap;

use unicode_linebreak::{linebreaks, BreakOpportunity as UnicodeBreakOpportunity};

/// Break opportunity attached to an inline boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BreakOpportunity {
    Mandatory,
    Allowed,
    Forbidden,
}

/// Collects Unicode line-break opportunities keyed by byte boundary.
pub(crate) fn collect_breaks(text: &str) -> HashMap<usize, BreakOpportunity> {
    let mut breaks = HashMap::new();
    for (byte_index, opportunity) in linebreaks(text) {
        let mapped = match opportunity {
            UnicodeBreakOpportunity::Mandatory => BreakOpportunity::Mandatory,
            UnicodeBreakOpportunity::Allowed => BreakOpportunity::Allowed,
        };
        breaks.insert(byte_index, mapped);
    }
    breaks
}
