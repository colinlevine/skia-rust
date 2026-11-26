#![forbid(unsafe_code)]

pub mod add_intersections;
pub mod angle;
pub mod arena;
pub mod as_winding;
pub mod coincidence;
pub mod common;
pub mod conic;
pub mod contour;
pub mod cubic;
pub mod curve;
pub mod edge_builder;
pub mod intersections;
pub mod line;
pub mod op;
pub mod op_builder;
pub mod path_writer;
pub mod quad;
pub mod segment;
pub mod simplify;
pub mod span;
pub mod tight_bounds;
pub mod tsect;
pub mod types;
pub mod winding;

#[cfg(test)]
mod tests;

pub use as_winding::as_winding;
pub use op::op;
pub use op_builder::OpBuilder;
pub use simplify::simplify;
pub use tight_bounds::compute_tight_bounds;
pub use types::PathOp;
