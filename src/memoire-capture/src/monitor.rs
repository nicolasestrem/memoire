//! Monitor enumeration and management

use anyhow::Result;
use tracing::{debug, info};
use windows::{
    core::Interface,
    Win32::Graphics::Dxgi::*,
};

use crate::error::CaptureError;

/// Information about a display monitor
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub adapter_index: u32,
    pub output_index: u32,
    pub is_primary: bool,
}

/// Monitor wrapper for capture operations
pub struct Monitor {
    pub info: MonitorInfo,
    pub output: IDXGIOutput1,
    pub adapter: IDXGIAdapter1,
}

impl Monitor {
    /// Enumerate all available monitors
    pub fn enumerate_all() -> Result<Vec<MonitorInfo>> {
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
        let mut monitors = Vec::new();
        let mut adapter_index = 0;

        loop {
            let adapter = match unsafe { factory.EnumAdapters1(adapter_index) } {
                Ok(a) => a,
                Err(_) => break,
            };

            let mut output_index = 0;
            loop {
                let output = match unsafe { adapter.EnumOutputs(output_index) } {
                    Ok(o) => o,
                    Err(_) => break,
                };

                let desc = unsafe { output.GetDesc()? };
                let name = String::from_utf16_lossy(
                    &desc.DeviceName[..desc.DeviceName.iter().position(|&c| c == 0).unwrap_or(desc.DeviceName.len())]
                );

                let width = (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left) as u32;
                let height = (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top) as u32;
                let is_primary = desc.DesktopCoordinates.left == 0 && desc.DesktopCoordinates.top == 0;

                debug!(
                    "found monitor: {} ({}x{}) primary={}",
                    name, width, height, is_primary
                );

                monitors.push(MonitorInfo {
                    name,
                    width,
                    height,
                    adapter_index,
                    output_index,
                    is_primary,
                });

                output_index += 1;
            }

            adapter_index += 1;
        }

        info!("enumerated {} monitors", monitors.len());
        Ok(monitors)
    }

    /// Get the primary monitor
    pub fn get_primary() -> Result<Monitor> {
        let monitors = Self::enumerate_all()?;
        let primary = monitors
            .into_iter()
            .find(|m| m.is_primary)
            .or_else(|| Self::enumerate_all().ok()?.into_iter().next())
            .ok_or(CaptureError::NoMonitors)?;

        Self::from_info(primary)
    }

    /// Create a Monitor from MonitorInfo
    pub fn from_info(info: MonitorInfo) -> Result<Monitor> {
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
        let adapter = unsafe { factory.EnumAdapters1(info.adapter_index)? };
        let output = unsafe { adapter.EnumOutputs(info.output_index)? };
        let output1: IDXGIOutput1 = output.cast()?;

        Ok(Monitor {
            info,
            output: output1,
            adapter,
        })
    }
}
