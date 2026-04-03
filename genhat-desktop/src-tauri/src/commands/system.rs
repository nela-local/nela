//! Tauri commands for system information and device compatibility.

use crate::system::{
    check_model_compatibility, check_model_compatibility_with_context, get_device_specs, 
    DeviceSpecs, ModelCompatibility, ModelTier, QuantLevel, ModelParams,
};

/// Get device specifications (RAM, CPU, OS, AVX2 support)
#[tauri::command]
pub fn get_system_specs() -> DeviceSpecs {
    get_device_specs()
}

/// Check if a model is compatible with the current device
/// 
/// Args:
///   - file_size_mb: Size of the model file in megabytes
///   - memory_mb: Optional known memory requirement (overrides estimation)
///   - quantization: Optional quantization type (for logging/future use)
///   - filename: Optional filename for better model detection
///   - context_length: Optional context length (default 4096)
#[tauri::command]
pub fn check_compatibility(
    file_size_mb: u64, 
    memory_mb: Option<u32>, 
    quantization: Option<String>,
    filename: Option<String>,
    context_length: Option<u32>,
) -> ModelCompatibility {
    let specs = get_device_specs();
    
    // Log details if provided
    if let Some(ref quant) = quantization {
        log::debug!("Checking compatibility for {} quantization", quant);
    }
    if let Some(ref name) = filename {
        log::debug!("Model filename: {}", name);
    }
    
    let ctx = context_length.unwrap_or(4096);
    
    check_model_compatibility_with_context(
        &specs, 
        file_size_mb, 
        memory_mb, 
        filename.as_deref(),
        ctx,
    )
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

/// Detect quantization level from filename
#[tauri::command]
pub fn detect_quantization(filename: String) -> String {
    let quant = QuantLevel::from_filename(&filename);
    quant.display_name().to_string()
}

/// Detect model parameter size from filename
#[tauri::command]
pub fn detect_model_params(filename: String) -> String {
    let params = ModelParams::from_filename(&filename);
    format!("{:?}", params)
}
