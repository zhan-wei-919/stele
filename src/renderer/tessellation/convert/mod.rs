//! Low-level conversion from `PathCmd` verbs into lyon tessellation output.

mod fringe;
mod lower;
mod tessellate;

#[cfg(test)]
mod tests;

pub(super) use tessellate::tessellate_path;
