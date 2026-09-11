//! Linking evidence, commits and ADRs to features.

pub mod commit_link;
pub mod ownership;
pub mod refs;

pub use ownership::PathOwnership;
pub use refs::FeatureIdIndex;
