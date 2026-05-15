//! Output formatters for lint results.

pub mod sarif;

pub use sarif::{generate_sarif, generate_sarif_to};
