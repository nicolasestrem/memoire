//! Static file serving with embedded files
//!
//! Files are embedded at compile time so the viewer works from any directory.

use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

/// Embedded static files (included at compile time)
const INDEX_HTML: &str = include_str!("../../static/index.html");
const STYLE_CSS: &str = include_str!("../../static/style.css");
const APP_JS: &str = include_str!("../../static/app.js");

/// Serve the main HTML page
pub async fn serve_index() -> impl IntoResponse {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(INDEX_HTML.to_string())
        .unwrap()
}

/// Serve the CSS stylesheet
pub async fn serve_style() -> impl IntoResponse {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
        .body(STYLE_CSS.to_string())
        .unwrap()
}

/// Serve the JavaScript application
pub async fn serve_app_js() -> impl IntoResponse {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/javascript; charset=utf-8")
        .body(APP_JS.to_string())
        .unwrap()
}
