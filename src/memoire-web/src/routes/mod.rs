//! HTTP route handlers

pub mod api;
pub mod audio;
pub mod static_files;
pub mod video;

pub use api::*;
pub use audio::*;
pub use static_files::*;
pub use video::*;
