//! Linking evidence, commits and ADRs to features.

pub mod commit_link;
pub mod ownership;
pub mod refs;
pub mod test_link;
pub mod test_map;

pub use ownership::PathOwnership;
pub use refs::FeatureIdIndex;
