//! Axum server setup and routing

use crate::routes;
use crate::state::AppState;
use axum::{
    routing::get,
    Router,
};
use std::net::SocketAddr;
use std::path::PathBuf;
use tower_http::cors::{CorsLayer, Any};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::info;

/// Start the web server
pub async fn serve(
    db: rusqlite::Connection,
    data_dir: PathBuf,
    port: u16,
) -> anyhow::Result<()> {
    let state = AppState::new(db, data_dir);

    // Build router
    let app = Router::new()
        // API routes
        .route("/api/chunks", get(routes::get_chunks))
        .route("/api/chunks/:id", get(routes::get_chunk))
        .route("/api/chunks/:id/frames", get(routes::get_chunk_frames))
        .route("/api/frames", get(routes::get_frames))
        .route("/api/frames/:id", get(routes::get_frame))
        .route("/api/stats", get(routes::get_stats))
        .route("/api/stats/ocr", get(routes::get_ocr_stats))
        .route("/api/monitors", get(routes::get_monitors))
        .route("/api/search", get(routes::search_ocr))
        // Video streaming
        .route("/video/:id", get(routes::stream_video))
        // Static files (HTML/CSS/JS)
        .nest_service("/", ServeDir::new("src/memoire-web/static"))
        // Add state
        .with_state(state)
        // Middleware
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http());

    // Bind to address
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!("Starting Memoire viewer on http://{}", addr);
    println!("\n🎥 Memoire Validation Viewer");
    println!("   → http://{}\n", addr);

    // Start server
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
