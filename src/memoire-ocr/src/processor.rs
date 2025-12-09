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

        // Convert RGBA to BGRA format
        // Windows expects BGRA, our input is RGBA, so we need to swap R and B channels
        let mut bgra_data = Vec::with_capacity(frame.data.len());
        for chunk in frame.data.chunks_exact(4) {
            bgra_data.push(chunk[2]); // B
            bgra_data.push(chunk[1]); // G
            bgra_data.push(chunk[0]); // R
            bgra_data.push(chunk[3]); // A
        }

        // Create SoftwareBitmap using image crate as intermediate
        // This is a workaround for the lack of direct buffer access in windows-rs 0.58
        use image::{ImageBuffer, Rgba};

        // Create image from BGRA data
        let img = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
            frame.width,
            frame.height,
            bgra_data,
        ).ok_or_else(|| OcrError::ConversionError("failed to create image buffer".to_string()))?;

        // Save to temporary in-memory PNG
        let mut png_data = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut png_data),
            image::ImageFormat::Png
        )?;

        // Create SoftwareBitmap from PNG data using Windows BitmapDecoder
        use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
        use windows::Graphics::Imaging::BitmapDecoder;

        let stream = InMemoryRandomAccessStream::new()
            .map_err(|e| OcrError::ConversionError(format!("failed to create stream: {}", e)))?;

        let writer = DataWriter::CreateDataWriter(&stream)
            .map_err(|e| OcrError::ConversionError(format!("failed to create writer: {}", e)))?;

        writer.WriteBytes(&png_data)
            .map_err(|e| OcrError::ConversionError(format!("failed to write bytes: {}", e)))?;

        writer.StoreAsync()
            .map_err(|e| OcrError::ConversionError(format!("failed to store: {}", e)))?
            .get()
            .map_err(|e| OcrError::ConversionError(format!("failed to get: {}", e)))?;

        stream.Seek(0)
            .map_err(|e| OcrError::ConversionError(format!("failed to seek: {}", e)))?;

        let decoder = BitmapDecoder::CreateAsync(&stream)
            .map_err(|e| OcrError::ConversionError(format!("failed to create decoder: {}", e)))?
            .get()
            .map_err(|e| OcrError::ConversionError(format!("failed to get decoder: {}", e)))?;

        let bitmap = decoder.GetSoftwareBitmapAsync()
            .map_err(|e| OcrError::ConversionError(format!("failed to get bitmap async: {}", e)))?
            .get()
            .map_err(|e| OcrError::ConversionError(format!("failed to get bitmap: {}", e)))?;

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
