use crate::engine::{Engine, OcrFrameResult};
use crate::error::{OcrError, Result};
use tracing::{debug, warn};
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap,
};

/// Frame data for OCR processing
pub struct FrameData {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>, // RGBA format
}

/// OCR processor that converts frames and performs recognition
pub struct Processor {
    engine: Engine,
}

impl Processor {
    /// Create a new processor with default OCR engine
    pub fn new() -> Result<Self> {
        let engine = Engine::english()?;
        Ok(Self { engine })
    }

    /// Create processor with custom language
    pub fn with_language(language_tag: &str) -> Result<Self> {
        let engine = Engine::new(Some(language_tag))?;
        Ok(Self { engine })
    }

    /// Process a single RGBA frame
    pub async fn process_frame(&self, frame: FrameData) -> Result<OcrFrameResult> {
        debug!("processing frame {}x{}", frame.width, frame.height);

        // Convert RGBA to SoftwareBitmap
        let bitmap = self.rgba_to_bitmap(frame)?;

        // Perform OCR
        let result = self.engine.recognize(&bitmap).await?;

        Ok(result)
    }

    /// Batch process multiple frames
    pub async fn process_frames(&self, frames: Vec<FrameData>) -> Vec<Result<OcrFrameResult>> {
        let mut results = Vec::with_capacity(frames.len());

        for frame in frames {
            let result = self.process_frame(frame).await;
            results.push(result);
        }

        results
    }

    /// Convert RGBA frame data to Windows SoftwareBitmap
    fn rgba_to_bitmap(&self, frame: FrameData) -> Result<SoftwareBitmap> {
        // Validate dimensions
        let expected_size = (frame.width * frame.height * 4) as usize;
        if frame.data.len() != expected_size {
            return Err(OcrError::ConversionError(format!(
                "invalid frame data size: expected {}, got {}",
                expected_size,
                frame.data.len()
            )));
        }

        // Create SoftwareBitmap with BGRA8 format (Windows native)
        let bitmap = SoftwareBitmap::Create(
            BitmapPixelFormat::Bgra8,
            frame.width as i32,
            frame.height as i32,
        ).map_err(|e| OcrError::ConversionError(format!("failed to create bitmap: {}", e)))?;

        // Get bitmap buffer
        let buffer = bitmap.LockBuffer(windows::Graphics::Imaging::BitmapBufferAccessMode::Write)
            .map_err(|e| OcrError::ConversionError(format!("failed to lock buffer: {}", e)))?;

        let plane = buffer.GetPlaneDescription(0)
            .map_err(|e| OcrError::ConversionError(format!("failed to get plane: {}", e)))?;

        // Get reference to buffer data
        let buffer_ref = buffer.CreateReference()
            .map_err(|e| OcrError::ConversionError(format!("failed to create buffer reference: {}", e)))?;

        let data_buffer: windows::Storage::Streams::IBuffer = buffer_ref.cast()
            .map_err(|e| OcrError::ConversionError(format!("failed to cast buffer: {}", e)))?;

        // Copy RGBA data to BGRA format
        // Windows expects BGRA, our input is RGBA, so we need to swap R and B channels
        let mut bgra_data = Vec::with_capacity(frame.data.len());

        for chunk in frame.data.chunks_exact(4) {
            bgra_data.push(chunk[2]); // B
            bgra_data.push(chunk[1]); // G
            bgra_data.push(chunk[0]); // R
            bgra_data.push(chunk[3]); // A
        }

        // Write data to bitmap buffer using unsafe Windows API
        unsafe {
            let data_interface: windows::Storage::Streams::IBufferByteAccess = data_buffer.cast()
                .map_err(|e| OcrError::ConversionError(format!("failed to get buffer access: {}", e)))?;

            let buffer_ptr = data_interface.Buffer()
                .map_err(|e| OcrError::ConversionError(format!("failed to get buffer pointer: {}", e)))?;

            std::ptr::copy_nonoverlapping(
                bgra_data.as_ptr(),
                buffer_ptr,
                bgra_data.len()
            );
        }

        data_buffer.SetLength(bgra_data.len() as u32)
            .map_err(|e| OcrError::ConversionError(format!("failed to set buffer length: {}", e)))?;

        debug!("converted RGBA frame to SoftwareBitmap");
        Ok(bitmap)
    }
}

impl Default for Processor {
    fn default() -> Self {
        Self::new().expect("failed to create default OCR processor")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_data_validation() {
        let processor = Processor::new().unwrap();

        // Invalid size
        let invalid_frame = FrameData {
            width: 100,
            height: 100,
            data: vec![0; 100], // Should be 100*100*4 = 40000
        };

        let result = processor.rgba_to_bitmap(invalid_frame);
        assert!(result.is_err());
    }
}
