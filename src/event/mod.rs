//! Winit-side event routing and redraw throttling.

#[cfg(test)]
pub(crate) mod handlers;
#[cfg(not(test))]
mod handlers;
mod router;
mod throttle;

pub(crate) use handlers::ViewportSnapshot;
pub(crate) use router::{EventRouter, RouteAction};
pub(crate) use throttle::RedrawThrottle;
