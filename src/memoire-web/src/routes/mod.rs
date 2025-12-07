//! HTTP route handlers

pub mod api;
pub mod static_files;
pub mod video;

pub use api::*;
pub use static_files::*;
pub use video::*;
