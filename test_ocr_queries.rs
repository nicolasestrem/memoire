// Temporary syntax check file for OCR query functions
// This verifies the code compiles without running the full workspace build

use memoire_db::queries::*;
use memoire_db::schema::*;
use rusqlite::Connection;
use chrono::Utc;

#[allow(dead_code)]
fn test_compilation() {
    let conn = Connection::open_in_memory().unwrap();

    // Test get_frames_without_ocr
    let _frames: Vec<Frame> = get_frames_without_ocr(&conn, 100).unwrap();

    // Test get_ocr_count
    let _count: i64 = get_ocr_count(&conn).unwrap();

    // Test get_frame_with_ocr
    let _frame_ocr: Option<FrameWithOcr> = get_frame_with_ocr(&conn, 1).unwrap();

    // Test get_frames_with_ocr_in_range
    let start = Utc::now();
    let end = Utc::now();
    let _frames_ocr: Vec<FrameWithOcr> = get_frames_with_ocr_in_range(
        &conn,
        start,
        end,
        100,
        0
    ).unwrap();
}
