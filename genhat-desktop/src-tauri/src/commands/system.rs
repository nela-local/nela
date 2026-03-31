//! Tauri commands for system information and device compatibility.

use crate::system::{
    check_model_compatibility, get_device_specs, DeviceSpecs, ModelCompatibility, ModelTier,
};

/// Get device specifications (RAM, CPU, OS)
#[tauri::command]
pub fn get_system_specs() -> DeviceSpecs {
    get_device_specs()
}

/// Check if a model is compatible with the current device
/// 
/// Args:
///   - file_size_mb: Size of the model file in megabytes
///   - memory_mb: Optional known memory requirement (overrides estimation)
#[tauri::command]
pub fn check_compatibility(file_size_mb: u64, memory_mb: Option<u32>) -> ModelCompatibility {
    let specs = get_device_specs();
    check_model_compatibility(&specs, file_size_mb, memory_mb)
}

/// Get the model tier classification based on file size
#[tauri::command]
pub fn get_model_tier(file_size_mb: u64) -> ModelTier {
    ModelTier::from_file_size(file_size_mb)
}

/// Estimate memory requirements for a model based on its file size
#[tauri::command]
pub fn estimate_model_memory(file_size_mb: u64) -> u32 {
    crate::system::estimate_memory_from_file_size(file_size_mb)
}
