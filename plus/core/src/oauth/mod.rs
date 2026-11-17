//! Fork logic for upstream's OAuth flow, reached through `seam.rs`.

pub mod consent;
mod resource;

pub use resource::same_resource;
