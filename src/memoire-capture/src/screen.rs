//! DXGI Desktop Duplication screen capture

use anyhow::Result;
use chrono::{DateTime, Utc};
use image::{ImageBuffer, Rgba};
use std::time::Duration;
use tracing::{debug, trace, warn};
use windows::{
    core::Interface,
    Win32::Graphics::{
        Direct3D::*,
        Direct3D11::*,
        Dxgi::Common::*,
        Dxgi::*,
    },
};

use crate::error::CaptureError;
use crate::monitor::Monitor;

/// Captured frame data
pub struct CapturedFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub timestamp: DateTime<Utc>,
}

impl CapturedFrame {
    /// Convert to RGBA image buffer
    pub fn to_image(&self) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        ImageBuffer::from_raw(self.width, self.height, self.data.clone())
            .expect("buffer size mismatch")
    }

    /// Save as PNG file
    pub fn save_png(&self, path: &str) -> Result<()> {
        let img = self.to_image();
        img.save(path)?;
        Ok(())
    }

    /// Compute a 64-bit perceptual hash (average hash) for deduplication.
    /// Downsamples the image to 8x8, converts to grayscale, and compares
    /// each pixel to the mean brightness to produce a 64-bit fingerprint.
    pub fn compute_perceptual_hash(&self) -> u64 {
        const HASH_SIZE: usize = 8;

        // Calculate block sizes
        let block_w = self.width as usize / HASH_SIZE;
        let block_h = self.height as usize / HASH_SIZE;

        if block_w == 0 || block_h == 0 {
            // Image too small, return hash of raw data
            return self.data.iter().fold(0u64, |acc, &b| acc.wrapping_add(b as u64));
        }

        // Compute average grayscale value for each 8x8 block
        let mut block_values = [0u64; HASH_SIZE * HASH_SIZE];

        for by in 0..HASH_SIZE {
            for bx in 0..HASH_SIZE {
                let mut sum = 0u64;
                let mut count = 0u32;

                // Sample pixels in this block
                let start_y = by * block_h;
                let start_x = bx * block_w;
                let end_y = std::cmp::min(start_y + block_h, self.height as usize);
                let end_x = std::cmp::min(start_x + block_w, self.width as usize);

                for y in start_y..end_y {
                    for x in start_x..end_x {
                        let idx = (y * self.width as usize + x) * 4;
                        if idx + 2 < self.data.len() {
                            // Convert RGB to grayscale using luminance formula
                            let r = self.data[idx] as u64;
                            let g = self.data[idx + 1] as u64;
                            let b = self.data[idx + 2] as u64;
                            sum += (r * 299 + g * 587 + b * 114) / 1000;
                            count += 1;
                        }
                    }
                }

                block_values[by * HASH_SIZE + bx] = if count > 0 { sum / count as u64 } else { 0 };
            }
        }

        // Calculate mean of all blocks
        let total: u64 = block_values.iter().sum();
        let mean = total / (HASH_SIZE * HASH_SIZE) as u64;

        // Build hash: 1 if above mean, 0 if below
        let mut hash = 0u64;
        for (i, &value) in block_values.iter().enumerate() {
            if value >= mean {
                hash |= 1u64 << i;
            }
        }

        hash
    }

    /// Calculate the Hamming distance between two hashes (number of differing bits).
    /// A distance of 0 means identical, lower values mean more similar frames.
    pub fn hash_distance(hash1: u64, hash2: u64) -> u32 {
        (hash1 ^ hash2).count_ones()
    }
}

/// Screen capture using DXGI Desktop Duplication API
pub struct ScreenCapture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    width: u32,
    height: u32,
    staging_texture: Option<ID3D11Texture2D>,
}

impl ScreenCapture {
    /// Create a new screen capture for the given monitor
    pub fn new(monitor: &Monitor) -> Result<Self> {
        debug!(
            "initializing screen capture for monitor: {}",
            monitor.info.name
        );

        // Create D3D11 device
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        let mut feature_level = D3D_FEATURE_LEVEL_11_0;

        unsafe {
            D3D11CreateDevice(
                &monitor.adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut feature_level),
                Some(&mut context),
            )?;
        }

        let device = device.ok_or(CaptureError::NotInitialized)?;
        let context = context.ok_or(CaptureError::NotInitialized)?;

        // Create output duplication
        let duplication = unsafe {
            monitor.output.DuplicateOutput(&device)?
        };

        // Get output description - cast to IDXGIOutput first
        let output: IDXGIOutput = monitor.output.cast()?;
        let desc = unsafe { output.GetDesc()? };
        let width = (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left) as u32;
        let height = (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top) as u32;

        debug!("screen capture initialized: {}x{}", width, height);

        Ok(Self {
            device,
            context,
            duplication,
            width,
            height,
            staging_texture: None,
        })
    }

    /// Capture a single frame
    pub fn capture_frame(&mut self, timeout: Duration) -> Result<Option<CapturedFrame>> {
        let timeout_ms = timeout.as_millis() as u32;
        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut desktop_resource: Option<IDXGIResource> = None;

        // Acquire next frame
        let result = unsafe {
            self.duplication.AcquireNextFrame(
                timeout_ms,
                &mut frame_info,
                &mut desktop_resource,
            )
        };

        match result {
            Ok(()) => {}
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                trace!("frame capture timeout (no new frame)");
                return Ok(None);
            }
            Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                warn!("desktop duplication access lost, needs reinitialization");
                return Err(CaptureError::DeviceRemoved.into());
            }
            Err(e) => return Err(CaptureError::Windows(e).into()),
        }

        let desktop_resource = desktop_resource.ok_or(CaptureError::FrameAcquisition(
            "no resource returned".to_string(),
        ))?;

        // Get the texture from the resource
        let desktop_texture: ID3D11Texture2D = desktop_resource.cast()?;

        // Create or reuse staging texture
        let staging = self.get_or_create_staging_texture()?;

        // Copy to staging texture
        unsafe {
            self.context.CopyResource(&staging, &desktop_texture);
        }

        // Map the staging texture to read pixels
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context.Map(
                &staging,
                0,
                D3D11_MAP_READ,
                0,
                Some(&mut mapped),
            )?;
        }

        // Copy pixel data (BGRA format) with bounds validation
        let row_pitch = mapped.RowPitch as usize;
        let width = self.width as usize;
        let height = self.height as usize;

        // Validate row_pitch is sufficient for width
        let min_row_pitch = width.checked_mul(4).ok_or_else(|| {
            unsafe { self.context.Unmap(&staging, 0); }
            CaptureError::FrameAcquisition("width overflow in row pitch calculation".to_string())
        })?;

        if row_pitch < min_row_pitch {
            unsafe { self.context.Unmap(&staging, 0); }
            return Err(CaptureError::FrameAcquisition(
                format!("invalid row_pitch: {} < minimum {}", row_pitch, min_row_pitch)
            ).into());
        }

        // Validate total buffer size won't overflow
        let total_size = width.checked_mul(height)
            .and_then(|wh| wh.checked_mul(4))
            .ok_or_else(|| {
                unsafe { self.context.Unmap(&staging, 0); }
                CaptureError::FrameAcquisition("buffer size overflow".to_string())
            })?;

        let mut data = Vec::with_capacity(total_size);

        unsafe {
            let src = mapped.pData as *const u8;

            // Validate pointer is not null
            if src.is_null() {
                self.context.Unmap(&staging, 0);
                return Err(CaptureError::FrameAcquisition("null pData pointer".to_string()).into());
            }

            for y in 0..height {
                // Validate row offset won't overflow
                let row_offset = y.checked_mul(row_pitch).ok_or_else(|| {
                    self.context.Unmap(&staging, 0);
                    CaptureError::FrameAcquisition("row offset overflow".to_string())
                })?;
                let row_start = src.add(row_offset);

                for x in 0..width {
                    let pixel_offset = x * 4; // Safe: x < width, width*4 validated above
                    let pixel = row_start.add(pixel_offset);
                    // Convert BGRA to RGBA
                    data.push(*pixel.add(2)); // R
                    data.push(*pixel.add(1)); // G
                    data.push(*pixel.add(0)); // B
                    data.push(*pixel.add(3)); // A
                }
            }

            self.context.Unmap(&staging, 0);
        }

        // Release the frame
        unsafe {
            self.duplication.ReleaseFrame()?;
        }

        Ok(Some(CapturedFrame {
            data,
            width: self.width,
            height: self.height,
            timestamp: Utc::now(),
        }))
    }

    /// Get screen dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn get_or_create_staging_texture(&mut self) -> Result<ID3D11Texture2D> {
        if let Some(ref texture) = self.staging_texture {
            return Ok(texture.clone());
        }

        let desc = D3D11_TEXTURE2D_DESC {
            Width: self.width,
            Height: self.height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };

        let texture = unsafe {
            let mut texture: Option<ID3D11Texture2D> = None;
            self.device.CreateTexture2D(&desc, None, Some(&mut texture))?;
            texture.ok_or(CaptureError::NotInitialized)?
        };

        self.staging_texture = Some(texture.clone());
        Ok(texture)
    }
}

impl Drop for ScreenCapture {
    fn drop(&mut self) {
        debug!("releasing screen capture resources");
    }
}
