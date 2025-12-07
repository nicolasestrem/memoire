//! Memoire web viewer - REST API and validation interface

pub mod error;
pub mod routes;
pub mod server;
pub mod state;

pub use error::ApiError;
pub use server::serve;
pub use state::AppState;
