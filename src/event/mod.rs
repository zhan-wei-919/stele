//! Winit-side event routing for the Redux view layer.

#[cfg(test)]
pub(crate) mod clipboard;
#[cfg(not(test))]
mod clipboard;
#[cfg(test)]
pub(crate) mod handlers;
#[cfg(not(test))]
mod handlers;
mod router;

pub(crate) use handlers::ViewportSnapshot;
pub(crate) use router::{EventRouter, RouteAction};
