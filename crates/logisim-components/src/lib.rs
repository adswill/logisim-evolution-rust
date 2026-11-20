//! Component libraries. To add one: create `<lib>.rs` (or `<lib>/mod.rs`) exposing
//! `pub fn library() -> Library`, then add its name to the list below. See CONTRIBUTING.md.
pub mod registry;
pub use logisim_core::Value;
pub use registry::{Library, PartDesc};

libraries!(gates, wiring, ttl, bfh, plexers);
