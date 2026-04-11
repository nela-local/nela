//! System information detection for device capability assessment.
//!
//! This module provides functions to detect system RAM, CPU, and estimate
//! whether models will run well on the device.
//!
//! ## Estimation Algorithm (v2)
//! 
//! 1. **Base FP16 Sizes** (lookup table):
//!    - 3B → ~6 GB, 7B → ~13 GB, 13B → ~26 GB, 30B → ~60 GB, 70B → ~140 GB
//! 
//! 2. **Quantization Multipliers**:
//!    - Q2 → 0.25, Q3 → 0.35, Q4 → 0.5, Q5 → 0.65, Q8 → 1.0
//! 
//! 3. **RAM Estimation**:
//!    - required_ram = file_size × 1.3 (or ×1.5 if context ≥ 8k)
//! 
//! 4. **CPU Performance Score**:
//!    - cpu_score = cpu_cores × (1.0 if AVX2 else 0.5)
//!    - perf_score = cpu_score / model_factor × quant_boost

use serde::{Deserialize, Serialize};
use sysinfo::System;

/// Device hardware specifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSpecs {
    pub total_ram_mb: u64,
    pub available_ram_mb: u64,
    pub total_ram_gb: f64,
    pub available_ram_gb: f64,
    pub cpu_cores: usize,
    pub cpu_model: String,
    pub cpu_has_avx2: bool,
    pub os: String,
    pub available_disk_gb: f64,
    pub total_disk_gb: f64,
    /// The models directory path being used for disk space calculation
    pub models_dir: String,
}

/// Model compatibility rating
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CompatibilityRating {
    /// Model will run smoothly with optimal performance
    Efficient,
    /// Model will run but with acceptable performance - usable
    Usable,
    /// Model will be very slow but may work
    VerySlow,
    /// Model is not recommended - may have poor performance or fail
    NotRecommended,
    /// Model won't run at all - insufficient resources
    WontRun,
    /// Cannot determine compatibility
    Unknown,
}

/// Model parameter size classification
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModelParams {
    /// ~1B parameters
    Params1B,
    /// ~3B parameters
    Params3B,
    /// ~7B parameters
    Params7B,
    /// ~13B parameters
    Params13B,
    /// ~30B parameters
    Params30B,
    /// ~70B parameters
    Params70B,
    /// Unknown size
    Unknown,
}

impl ModelParams {
    /// Get base FP16 size in GB for this model size
    pub fn base_fp16_size_gb(&self) -> f64 {
        match self {
            ModelParams::Params1B => 2.0,
            ModelParams::Params3B => 6.0,
            ModelParams::Params7B => 13.0,
            ModelParams::Params13B => 26.0,
            ModelParams::Params30B => 60.0,
            ModelParams::Params70B => 140.0,
            ModelParams::Unknown => 0.0,
        }
    }
    
    /// Get model scaling factor for CPU performance calculation
    pub fn cpu_scaling_factor(&self) -> f64 {
        match self {
            ModelParams::Params1B => 0.5,
            ModelParams::Params3B => 1.0,
            ModelParams::Params7B => 2.0,
            ModelParams::Params13B => 4.0,
            ModelParams::Params30B => 8.0,
            ModelParams::Params70B => 16.0,
            ModelParams::Unknown => 2.0, // Default to 7B scaling
        }
    }
    
    /// Detect model params from filename
    /// 
    /// Handles various naming conventions:
    /// - Standard: "llama-7b", "mistral-7B"
    /// - Decimal: "18.4B", "1.5B"
    /// - With suffixes: "7B-Instruct", "13b-chat"
    /// - MoE notation excluded: "8X3B" (not 8B or 3B, use total params)
    pub fn from_filename(filename: &str) -> Self {
        let lower = filename.to_lowercase();
        
        // Use regex-like matching to find parameter patterns
        // Pattern: optional dash/underscore, then digits with optional decimal, then 'b'
        // We need to avoid matching MoE patterns like "8x3b" (expert notation)
        
        // Find all potential parameter matches and pick the largest/most relevant
        let mut best_match: Option<(usize, f64)> = None; // (position, billions)
        
        let chars: Vec<char> = lower.chars().collect();
        let len = chars.len();
        let mut i = 0;
        
        while i < len {
            // Skip MoE patterns: digit followed by 'x' followed by digits
            // e.g., "8x3b" - we don't want to match "3b" here
            if i + 2 < len && chars[i].is_ascii_digit() && chars[i + 1] == 'x' && chars[i + 2].is_ascii_digit() {
                // Skip past the MoE pattern
                while i < len && (chars[i].is_ascii_digit() || chars[i] == 'x') {
                    i += 1;
                }
                continue;
            }
            
            // Look for pattern: [separator]digits[.digits]b
            // Valid separators: start of string, '-', '_', space
            let is_valid_start = i == 0 || 
                matches!(chars[i.saturating_sub(1)], '-' | '_' | ' ' | '.');
            
            if is_valid_start && chars[i].is_ascii_digit() {
                // Parse the number (potentially with decimal)
                let start = i;
                let mut num_str = String::new();
                
                // Collect integer part
                while i < len && chars[i].is_ascii_digit() {
                    num_str.push(chars[i]);
                    i += 1;
                }
                
                // Check for decimal part
                if i + 1 < len && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
                    num_str.push('.');
                    i += 1;
                    while i < len && chars[i].is_ascii_digit() {
                        num_str.push(chars[i]);
                        i += 1;
                    }
                }
                
                // Check if followed by 'b' (case insensitive already)
                if i < len && chars[i] == 'b' {
                    // Skip if this is actually "gb" (like in file size "13gb")
                    // Check: is there a digit before our number start that would make this "Xgb"?
                    // Also skip if followed by characters that suggest it's not a param count
                    let prev_char = if start > 0 { Some(chars[start - 1]) } else { None };
                    let next_char = if i + 1 < len { Some(chars[i + 1]) } else { None };
                    
                    // Skip if previous char is 'g' (part of "gb")
                    let is_gb_suffix = prev_char == Some('g');
                    // Skip if it's followed by 's' or 'i' (could be "bs" or "bi" unit)
                    let bad_suffix = matches!(next_char, Some('s') | Some('i'));
                    
                    if !is_gb_suffix && !bad_suffix {
                        if let Ok(billions) = num_str.parse::<f64>() {
                            // Only accept reasonable model sizes (0.5B to 200B)
                            if billions >= 0.5 && billions <= 200.0 {
                                // Prefer later matches (usually the actual param count)
                                // unless an earlier match is significantly larger
                                if let Some((_, prev_billions)) = best_match {
                                    if billions > prev_billions * 0.9 {
                                        best_match = Some((start, billions));
                                    }
                                } else {
                                    best_match = Some((start, billions));
                                }
                            }
                        }
                    }
                }
                continue;
            }
            i += 1;
        }
        
        // Convert the detected parameter count to enum
        if let Some((_, billions)) = best_match {
            if billions >= 50.0 {
                ModelParams::Params70B
            } else if billions >= 20.0 {
                ModelParams::Params30B
            } else if billions >= 10.0 {
                ModelParams::Params13B
            } else if billions >= 5.0 {
                ModelParams::Params7B
            } else if billions >= 2.5 {
                ModelParams::Params3B
            } else {
                ModelParams::Params1B
            }
        } else {
            ModelParams::Unknown
        }
    }
    
    /// Estimate model params from file size and quantization
    pub fn from_file_size(file_size_gb: f64, quant_multiplier: f64) -> Self {
        // Reverse the quantization to get estimated FP16 size
        let estimated_fp16_gb = file_size_gb / quant_multiplier;
        
        // Match to closest model size
        if estimated_fp16_gb < 4.0 {
            ModelParams::Params1B
        } else if estimated_fp16_gb < 9.5 {
            ModelParams::Params3B
        } else if estimated_fp16_gb < 19.5 {
            ModelParams::Params7B
        } else if estimated_fp16_gb < 43.0 {
            ModelParams::Params13B
        } else if estimated_fp16_gb < 100.0 {
            ModelParams::Params30B
        } else {
            ModelParams::Params70B
        }
    }
}

/// Quantization level classification
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum QuantLevel {
    Q2,
    Q3,
    Q4,
    Q5,
    Q6,
    Q8,
    F16,
    F32,
    Unknown,
}

impl QuantLevel {
    /// Get file size multiplier (relative to FP16)
    pub fn size_multiplier(&self) -> f64 {
        match self {
            QuantLevel::Q2 => 0.25,
            QuantLevel::Q3 => 0.35,
            QuantLevel::Q4 => 0.50,
            QuantLevel::Q5 => 0.65,
            QuantLevel::Q6 => 0.75,
            QuantLevel::Q8 => 1.0,
            QuantLevel::F16 => 1.0,
            QuantLevel::F32 => 2.0,
            QuantLevel::Unknown => 0.5, // Default to Q4
        }
    }
    
    /// Get CPU performance boost factor (lower quant = faster inference)
    pub fn perf_boost(&self) -> f64 {
        match self {
            QuantLevel::Q2 => 1.6,
            QuantLevel::Q3 => 1.3,
            QuantLevel::Q4 => 1.0,
            QuantLevel::Q5 => 0.85,
            QuantLevel::Q6 => 0.75,
            QuantLevel::Q8 => 0.6,
            QuantLevel::F16 => 0.5,
            QuantLevel::F32 => 0.3,
            QuantLevel::Unknown => 1.0,
        }
    }
    
    /// Get human-readable name
    pub fn display_name(&self) -> &'static str {
        match self {
            QuantLevel::Q2 => "Q2 (Very compressed)",
            QuantLevel::Q3 => "Q3 (Compressed)",
            QuantLevel::Q4 => "Q4 (Balanced)",
            QuantLevel::Q5 => "Q5 (Good quality)",
            QuantLevel::Q6 => "Q6 (High quality)",
            QuantLevel::Q8 => "Q8 (Very high quality)",
            QuantLevel::F16 => "F16 (Half precision)",
            QuantLevel::F32 => "F32 (Full precision)",
            QuantLevel::Unknown => "Unknown",
        }
    }
    
    /// Detect quantization level from filename
    /// 
    /// Handles various quantization formats:
    /// - Standard GGML: Q2_K, Q3_K, Q4_K, Q5_K, Q6_K, Q8_0
    /// - K-quant variants: Q4_K_M, Q4_K_S, Q5_K_M, Q5_K_S
    /// - I-quant (importance): IQ1_S, IQ2_XXS, IQ3_XS, IQ4_XS, IQ4_NL
    /// - Precision: F16, FP16, F32, FP32
    pub fn from_filename(filename: &str) -> Self {
        let lower = filename.to_lowercase();
        
        // Check for I-quant formats first (they map to similar Q levels)
        // IQ1 and IQ2 are very compressed (similar to Q2)
        if lower.contains("iq1_") || lower.contains("iq2_") {
            return QuantLevel::Q2;
        }
        // IQ3 is similar to Q3
        if lower.contains("iq3_") {
            return QuantLevel::Q3;
        }
        // IQ4 is similar to Q4
        if lower.contains("iq4_") {
            return QuantLevel::Q4;
        }
        
        // Check for standard quantization patterns
        // Q2 variants
        if lower.contains("q2_k") || lower.contains("q2-k") || lower.contains("-q2.") || lower.contains("_q2.") {
            QuantLevel::Q2
        // Q3 variants
        } else if lower.contains("q3_k") || lower.contains("q3-k") || lower.contains("-q3.") || lower.contains("_q3.") {
            QuantLevel::Q3
        // Q4 variants (most common)
        } else if lower.contains("q4_k") || lower.contains("q4_0") || lower.contains("q4_1") 
               || lower.contains("q4-k") || lower.contains("-q4.") || lower.contains("_q4.") 
               || lower.contains("-q4_") {
            QuantLevel::Q4
        // Q5 variants
        } else if lower.contains("q5_k") || lower.contains("q5_0") || lower.contains("q5_1") 
               || lower.contains("q5-k") || lower.contains("-q5.") || lower.contains("_q5.") 
               || lower.contains("-q5_") {
            QuantLevel::Q5
        // Q6 variants
        } else if lower.contains("q6_k") || lower.contains("q6-k") || lower.contains("-q6.") || lower.contains("_q6.") {
            QuantLevel::Q6
        // Q8 variants
        } else if lower.contains("q8_0") || lower.contains("q8_k") || lower.contains("q8-k") 
               || lower.contains("-q8.") || lower.contains("_q8.") {
            QuantLevel::Q8
        // Precision formats
        } else if lower.contains("f16") || lower.contains("fp16") {
            QuantLevel::F16
        } else if lower.contains("f32") || lower.contains("fp32") {
            QuantLevel::F32
        } else {
            QuantLevel::Unknown
        }
    }
}

/// Detailed calculation breakdown for transparency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityCalculation {
    /// Detected model parameter size
    pub model_params: String,
    /// Detected quantization level
    pub quant_level: String,
    /// Base FP16 size used in calculation (GB)
    pub base_fp16_size_gb: f64,
    /// Quantization multiplier applied
    pub quant_multiplier: f64,
    /// Estimated GGUF file size (GB)
    pub estimated_file_size_gb: f64,
    /// Actual file size (GB)
    pub actual_file_size_gb: f64,
    /// RAM multiplier used (1.3 or 1.5 for large context)
    pub ram_multiplier: f64,
    /// Estimated required RAM (GB)
    pub required_ram_gb: f64,
    /// Available system RAM (GB)
    pub available_ram_gb: f64,
    /// Total system RAM (GB)
    pub total_ram_gb: f64,
    /// RAM decision: "OK", "NOT_RECOMMENDED", or "DO_NOT_DOWNLOAD"
    pub ram_decision: String,
    /// CPU core count
    pub cpu_cores: usize,
    /// Whether CPU has AVX2 support
    pub cpu_has_avx2: bool,
    /// Calculated CPU score
    pub cpu_score: f64,
    /// Model scaling factor
    pub model_factor: f64,
    /// Quantization performance boost
    pub quant_boost: f64,
    /// Final performance score
    pub perf_score: f64,
    /// Performance classification
    pub perf_classification: String,
    /// Context length assumed (default 4096)
    pub assumed_context: u32,
}

/// Suggested alternative model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeModel {
    pub suggestion: String,
    pub reason: String,
}

/// Compatibility check result for a specific model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCompatibility {
    pub rating: CompatibilityRating,
    pub reason: String,
    pub estimated_memory_mb: u32,
    pub available_memory_mb: u64,
    pub can_run: bool,
    pub disk_space_sufficient: bool,
    pub required_disk_gb: f64,
    pub available_disk_gb: f64,
    pub ram_usage_percent: f64,
    pub disk_usage_percent: f64,
    pub cpu_suitable: bool,
    pub details: CompatibilityDetails,
    /// New: detailed calculation breakdown
    pub calculation: CompatibilityCalculation,
    /// New: suggested alternative if current model is not recommended
    pub alternative: Option<AlternativeModel>,
}

/// Detailed breakdown of compatibility factors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityDetails {
    pub ram_check: String,
    pub disk_check: String,
    pub cpu_check: String,
    pub performance_notes: Vec<String>,
}

/// Get current device specifications
pub fn get_device_specs() -> DeviceSpecs {
    let mut sys = System::new_all();
    sys.refresh_all();

    let total_ram_bytes = sys.total_memory();
    let available_ram_bytes = sys.available_memory();

    // Convert from bytes to MB and GB
    let total_ram_mb = total_ram_bytes / (1024 * 1024);
    let available_ram_mb = available_ram_bytes / (1024 * 1024);
    let total_ram_gb = total_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    let available_ram_gb = available_ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0);

    let cpu_model = sys
        .cpus()
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    // Detect AVX2 support
    let cpu_has_avx2 = detect_avx2_support();

    // Get disk space for models directory
    let models_dir = crate::paths::resolve_models_dir();
    // Clean up the path string - remove UNC prefix on Windows if present
    let models_dir_str = {
        let s = models_dir.to_string_lossy().to_string();
        // Remove \\?\ prefix if present (Windows extended path)
        if s.starts_with("\\\\?\\") {
            s[4..].to_string()
        } else {
            s
        }
    };
    let (available_disk_gb, total_disk_gb) = get_disk_space(&models_dir);

    DeviceSpecs {
        total_ram_mb,
        available_ram_mb,
        total_ram_gb,
        available_ram_gb,
        cpu_cores: sys.cpus().len(),
        cpu_model,
        cpu_has_avx2,
        os: std::env::consts::OS.to_string(),
        available_disk_gb,
        total_disk_gb,
        models_dir: models_dir_str,
    }
}

/// Detect AVX2 CPU support for performance estimation
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn detect_avx2_support() -> bool {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::__cpuid;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::__cpuid;
    
    // CPUID with EAX=7, ECX=0 returns AVX2 support in EBX bit 5
    unsafe {
        let result = __cpuid(7);
        (result.ebx & (1 << 5)) != 0
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn detect_avx2_support() -> bool {
    // Non-x86 architectures (ARM, etc.) don't have AVX2
    // But they may have NEON which provides similar benefits
    false
}

/// Get disk space for a given path (returns available_gb, total_gb)
fn get_disk_space(path: &std::path::Path) -> (f64, f64) {
    use sysinfo::Disks;
    
    let disks = Disks::new_with_refreshed_list();
    
    // Get the path as a string, handling UNC paths on Windows
    let _path_str = path.to_string_lossy();
    
    // On Windows, we need to match drive letters (e.g., "D:" matches "D:\")
    #[cfg(target_os = "windows")]
    {
        // Extract drive letter from path
        // Handle both regular paths (D:\...) and UNC paths (\\?\D:\...)
        let drive_letter = if path_str.starts_with("\\\\?\\") {
            // UNC path: \\?\D:\... -> extract D:
            path_str.chars().skip(4).take(2).collect::<String>().to_uppercase()
        } else {
            // Regular path: D:\... -> extract D:
            path_str.chars().take(2).collect::<String>().to_uppercase()
        };
        
        for disk in disks.list() {
            let mount_str = disk.mount_point().to_string_lossy();
            let mount_drive = mount_str.chars().take(2).collect::<String>().to_uppercase();
            
            if drive_letter == mount_drive {
                let available_bytes = disk.available_space();
                let total_bytes = disk.total_space();
                let available_gb = available_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
                let total_gb = total_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
                return (available_gb, total_gb);
            }
        }
    }
    
    #[cfg(not(target_os = "windows"))]
    {
        // Unix: use longest prefix matching
        // Try canonicalizing first for better matching
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        
        let mut best_match: Option<&sysinfo::Disk> = None;
        let mut best_len = 0;
        
        for disk in disks.list() {
            let mount = disk.mount_point();
            if canonical_path.starts_with(mount) {
                let mount_len = mount.to_string_lossy().len();
                if mount_len > best_len {
                    best_len = mount_len;
                    best_match = Some(disk);
                }
            }
        }
        
        if let Some(disk) = best_match {
            let available_bytes = disk.available_space();
            let total_bytes = disk.total_space();
            let available_gb = available_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
            let total_gb = total_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
            return (available_gb, total_gb);
        }
    }
    
    // Fallback: return the first disk or (0, 0)
    if let Some(disk) = disks.list().first() {
        let available_bytes = disk.available_space();
        let total_bytes = disk.total_space();
        let available_gb = available_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
        let total_gb = total_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
        (available_gb, total_gb)
    } else {
        (0.0, 0.0)
    }
}

/// Estimate memory requirements from model file size.
/// 
/// GGUF models typically require 1.2-1.5x their file size in RAM due to:
/// - Quantization overhead
/// - KV cache allocation
/// - Context embeddings
/// - Runtime inference buffers
pub fn estimate_memory_from_file_size(file_size_mb: u64) -> u32 {
    // Base estimate: 1.3x file size for quantization overhead
    let base_estimate = (file_size_mb as f64 * 1.3) as u32;
    
    // Add ~500 MB for inference overhead (KV cache, buffers, etc.)
    let with_overhead = base_estimate + 500;
    
    // Round to nearest 100 MB
    ((with_overhead + 50) / 100) * 100
}

/// Enhanced model compatibility check with detailed calculation breakdown
/// 
/// ## Algorithm (v2)
/// 
/// 1. **RAM Decision** (pre-download):
///    - required_ram = file_size × 1.3 (×1.5 if context ≥ 8k)
///    - if ram < required_ram: "DO NOT DOWNLOAD"
///    - elif ram < required_ram × 1.25: "NOT RECOMMENDED"
///    - else: "OK"
/// 
/// 2. **CPU Performance Score**:
///    - cpu_score = cpu_cores × (1.0 if AVX2 else 0.5)
///    - model_factor based on param size (3B=1, 7B=2, 13B=4, 30B=8, 70B=16)
///    - perf_score = (cpu_score / model_factor) × quant_boost
/// 
/// 3. **Final Rating**:
///    - RAM insufficient → "Won't Run"
///    - perf_score < 0.5 → "Not Recommended"
///    - perf_score < 1.0 → "Very Slow"
///    - perf_score < 2.0 → "Usable"
///    - else → "Efficient"
pub fn check_model_compatibility(
    specs: &DeviceSpecs,
    model_file_size_mb: u64,
    model_memory_mb: Option<u32>,
) -> ModelCompatibility {
    check_model_compatibility_with_context(specs, model_file_size_mb, model_memory_mb, None, 4096)
}

/// Enhanced compatibility check with filename for better detection
pub fn check_model_compatibility_with_context(
    specs: &DeviceSpecs,
    model_file_size_mb: u64,
    model_memory_mb: Option<u32>,
    filename: Option<&str>,
    context_length: u32,
) -> ModelCompatibility {
    let file_size_gb = model_file_size_mb as f64 / 1024.0;
    
    // Detect quantization level
    let quant_level = filename
        .map(QuantLevel::from_filename)
        .unwrap_or(QuantLevel::Unknown);
    
    // Detect model parameters
    let model_params = filename
        .map(ModelParams::from_filename)
        .unwrap_or(ModelParams::Unknown);
    
    // If model params unknown, estimate from file size
    let model_params = if model_params == ModelParams::Unknown {
        ModelParams::from_file_size(file_size_gb, quant_level.size_multiplier())
    } else {
        model_params
    };
    
    // Calculate base FP16 size and estimated file size
    let base_fp16_gb = model_params.base_fp16_size_gb();
    let quant_multiplier = quant_level.size_multiplier();
    let estimated_file_size_gb = base_fp16_gb * quant_multiplier;
    
    // RAM calculation
    // Use 1.3x for standard context, 1.5x for large context (≥8k)
    let ram_multiplier = if context_length >= 8192 { 1.5 } else { 1.3 };
    let required_ram_gb = file_size_gb * ram_multiplier;
    let required_ram_mb = (required_ram_gb * 1024.0) as u32;
    
    // Use provided memory estimate or our calculation
    let estimated_memory = model_memory_mb.unwrap_or(required_ram_mb);
    let required_ram_gb_final = estimated_memory as f64 / 1024.0;
    
    let total_ram_gb = specs.total_ram_gb;
    let available_ram_gb = specs.available_ram_gb;
    
    // RAM decision (pre-download)
    let ram_decision = if total_ram_gb < required_ram_gb_final {
        "DO_NOT_DOWNLOAD".to_string()
    } else if total_ram_gb < required_ram_gb_final * 1.25 {
        "NOT_RECOMMENDED".to_string()
    } else {
        "OK".to_string()
    };
    
    // Calculate RAM usage percentage
    let ram_usage_percent = (required_ram_gb_final / total_ram_gb) * 100.0;
    
    // CPU performance calculation
    let avx2_factor = if specs.cpu_has_avx2 { 1.0 } else { 0.5 };
    let cpu_score = specs.cpu_cores as f64 * avx2_factor;
    let model_factor = model_params.cpu_scaling_factor();
    let quant_boost = quant_level.perf_boost();
    let perf_score = (cpu_score / model_factor) * quant_boost;
    
    // Performance classification
    let perf_classification = if perf_score >= 2.0 {
        "Fast".to_string()
    } else if perf_score >= 1.0 {
        "Usable".to_string()
    } else if perf_score >= 0.5 {
        "Slow".to_string()
    } else {
        "Very Slow".to_string()
    };
    
    // Disk space check - use estimated file size for pre-download check
    let required_disk_gb = estimated_file_size_gb;
    let disk_space_sufficient = specs.available_disk_gb >= required_disk_gb;
    let disk_usage_percent = if specs.total_disk_gb > 0.0 {
        (required_disk_gb / specs.total_disk_gb) * 100.0
    } else {
        100.0
    };
    
    // Determine overall rating
    let ram_sufficient = total_ram_gb >= required_ram_gb_final;
    let can_run = ram_sufficient && disk_space_sufficient;
    
    let (rating, reason) = if !disk_space_sufficient {
        (
            CompatibilityRating::WontRun,
            format!("❌ Insufficient disk space. Need {:.1}GB, have {:.1}GB", 
                    required_disk_gb, specs.available_disk_gb),
        )
    } else if !ram_sufficient {
        (
            CompatibilityRating::WontRun,
            format!("❌ Insufficient RAM. Need {:.1}GB, have {:.1}GB", 
                    required_ram_gb_final, total_ram_gb),
        )
    } else if perf_score < 0.5 {
        (
            CompatibilityRating::NotRecommended,
            format!("❌ Performance too low (score: {:.2}). System will struggle.", perf_score),
        )
    } else if perf_score < 1.0 {
        (
            CompatibilityRating::VerySlow,
            format!("⚠️ Very slow inference expected (score: {:.2})", perf_score),
        )
    } else if perf_score < 2.0 {
        (
            CompatibilityRating::Usable,
            format!("⚠️ Usable but not optimal (score: {:.2})", perf_score),
        )
    } else {
        (
            CompatibilityRating::Efficient,
            format!("✅ Efficient performance expected (score: {:.2})", perf_score),
        )
    };
    
    // Build detailed checks
    let mut performance_notes = Vec::new();
    
    // RAM check message
    let ram_check = if ram_usage_percent < 60.0 {
        format!("✓ Excellent - {:.1}GB required / {:.1}GB available ({:.0}%)", 
                required_ram_gb_final, total_ram_gb, ram_usage_percent)
    } else if ram_usage_percent < 80.0 {
        performance_notes.push("Consider closing other applications".to_string());
        format!("⚠ Adequate - {:.1}GB required / {:.1}GB available ({:.0}%)", 
                required_ram_gb_final, total_ram_gb, ram_usage_percent)
    } else {
        performance_notes.push("High memory pressure expected".to_string());
        format!("✗ Tight - {:.1}GB required / {:.1}GB available ({:.0}%)", 
                required_ram_gb_final, total_ram_gb, ram_usage_percent)
    };
    
    // Disk check message
    let disk_check = if disk_space_sufficient {
        format!("✓ OK - {:.1}GB free (need {:.1}GB)", specs.available_disk_gb, required_disk_gb)
    } else {
        format!("✗ Insufficient - {:.1}GB free (need {:.1}GB)", specs.available_disk_gb, required_disk_gb)
    };
    
    // CPU check message with AVX2 info
    let cpu_check = format!(
        "{} {} cores, {} (score: {:.1})",
        if perf_score >= 1.0 { "✓" } else { "⚠" },
        specs.cpu_cores,
        if specs.cpu_has_avx2 { "AVX2 ✓" } else { "No AVX2" },
        cpu_score
    );
    
    // Add performance notes
    if !specs.cpu_has_avx2 {
        performance_notes.push("No AVX2 - inference will be ~50% slower".to_string());
    }
    if context_length >= 8192 {
        performance_notes.push(format!("Large context ({}k) increases memory usage", context_length / 1024));
    }
    if model_params == ModelParams::Params70B {
        performance_notes.push("70B model - expect long load times".to_string());
    }
    
    // Generate alternative suggestion if needed
    let alternative = if rating == CompatibilityRating::NotRecommended 
        || rating == CompatibilityRating::WontRun 
        || rating == CompatibilityRating::VerySlow 
    {
        suggest_alternative(&model_params, &quant_level, specs)
    } else {
        None
    };
    
    // Build calculation breakdown
    let calculation = CompatibilityCalculation {
        model_params: format!("{:?}", model_params),
        quant_level: quant_level.display_name().to_string(),
        base_fp16_size_gb: base_fp16_gb,
        quant_multiplier,
        estimated_file_size_gb,
        actual_file_size_gb: file_size_gb,
        ram_multiplier,
        required_ram_gb: required_ram_gb_final,
        available_ram_gb,
        total_ram_gb,
        ram_decision,
        cpu_cores: specs.cpu_cores,
        cpu_has_avx2: specs.cpu_has_avx2,
        cpu_score,
        model_factor,
        quant_boost,
        perf_score,
        perf_classification,
        assumed_context: context_length,
    };
    
    ModelCompatibility {
        rating,
        reason,
        estimated_memory_mb: estimated_memory,
        available_memory_mb: specs.available_ram_mb,
        can_run,
        disk_space_sufficient,
        required_disk_gb,
        available_disk_gb: specs.available_disk_gb,
        ram_usage_percent,
        disk_usage_percent,
        cpu_suitable: perf_score >= 0.5,
        details: CompatibilityDetails {
            ram_check,
            disk_check,
            cpu_check,
            performance_notes,
        },
        calculation,
        alternative,
    }
}

/// Suggest a better alternative model based on system constraints
fn suggest_alternative(
    current_params: &ModelParams,
    current_quant: &QuantLevel,
    specs: &DeviceSpecs,
) -> Option<AlternativeModel> {
    // Suggest smaller model or better quantization
    let suggestion = match current_params {
        ModelParams::Params70B => {
            if specs.total_ram_gb >= 16.0 {
                Some(("13B Q4".to_string(), "Similar quality for most tasks, much faster".to_string()))
            } else {
                Some(("7B Q4".to_string(), "Good quality, runs well on most systems".to_string()))
            }
        }
        ModelParams::Params30B => {
            if specs.total_ram_gb >= 12.0 {
                Some(("13B Q4".to_string(), "Better performance with similar capability".to_string()))
            } else {
                Some(("7B Q4".to_string(), "Good balance of quality and speed".to_string()))
            }
        }
        ModelParams::Params13B => {
            match current_quant {
                QuantLevel::Q8 | QuantLevel::F16 | QuantLevel::F32 => {
                    Some(("13B Q4".to_string(), "Same model, much smaller footprint".to_string()))
                }
                _ => {
                    if specs.total_ram_gb < 12.0 {
                        Some(("7B Q4".to_string(), "Lighter model that fits your RAM".to_string()))
                    } else {
                        None
                    }
                }
            }
        }
        ModelParams::Params7B => {
            match current_quant {
                QuantLevel::Q8 | QuantLevel::F16 | QuantLevel::F32 => {
                    Some(("7B Q4".to_string(), "Same model, better performance".to_string()))
                }
                _ => {
                    if specs.total_ram_gb < 8.0 {
                        Some(("3B Q4".to_string(), "Smaller model that fits your RAM".to_string()))
                    } else {
                        None
                    }
                }
            }
        }
        _ => None,
    };
    
    suggestion.map(|(suggestion, reason)| AlternativeModel { suggestion, reason })
}

/// Classify a model by its file size tier
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModelTier {
    /// <500 MB - Very small, runs on almost any device
    Tiny,
    /// 500-1500 MB - Small models, good for laptops
    Small,
    /// 1500-4000 MB - Medium models, need decent RAM
    Medium,
    /// 4000-8000 MB - Large models, need 16GB+ RAM
    Large,
    /// >8000 MB - Very large, need high-end systems
    VeryLarge,
}

impl ModelTier {
    pub fn from_file_size(file_size_mb: u64) -> Self {
        match file_size_mb {
            0..=500 => ModelTier::Tiny,
            501..=1500 => ModelTier::Small,
            1501..=4000 => ModelTier::Medium,
            4001..=8000 => ModelTier::Large,
            _ => ModelTier::VeryLarge,
        }
    }
    
    pub fn recommended_ram_gb(&self) -> u32 {
        match self {
            ModelTier::Tiny => 4,
            ModelTier::Small => 8,
            ModelTier::Medium => 16,
            ModelTier::Large => 32,
            ModelTier::VeryLarge => 64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_estimation() {
        // 1GB file should estimate to ~1.8GB
        let estimate = estimate_memory_from_file_size(1000);
        assert!(estimate >= 1700 && estimate <= 1900);
        
        // 500MB file should estimate to ~1.1GB
        let estimate = estimate_memory_from_file_size(500);
        assert!(estimate >= 1000 && estimate <= 1300);
    }

    #[test]
    fn test_model_tier() {
        assert_eq!(ModelTier::from_file_size(300), ModelTier::Tiny);
        assert_eq!(ModelTier::from_file_size(800), ModelTier::Small);
        assert_eq!(ModelTier::from_file_size(2000), ModelTier::Medium);
        assert_eq!(ModelTier::from_file_size(5000), ModelTier::Large);
        assert_eq!(ModelTier::from_file_size(10000), ModelTier::VeryLarge);
    }
    
    #[test]
    fn test_quant_detection() {
        assert_eq!(QuantLevel::from_filename("model-q4_k_m.gguf"), QuantLevel::Q4);
        assert_eq!(QuantLevel::from_filename("model-Q8_0.gguf"), QuantLevel::Q8);
        assert_eq!(QuantLevel::from_filename("model-f16.gguf"), QuantLevel::F16);
        assert_eq!(QuantLevel::from_filename("model-q2_k.gguf"), QuantLevel::Q2);
        // Test I-quant formats
        assert_eq!(QuantLevel::from_filename("model-IQ4_XS.gguf"), QuantLevel::Q4);
        assert_eq!(QuantLevel::from_filename("model-iq3_xs.gguf"), QuantLevel::Q3);
        assert_eq!(QuantLevel::from_filename("model-IQ2_XXS.gguf"), QuantLevel::Q2);
    }
    
    #[test]
    fn test_model_params_detection() {
        assert_eq!(ModelParams::from_filename("llama-7b-q4_k_m.gguf"), ModelParams::Params7B);
        assert_eq!(ModelParams::from_filename("mistral-13b-instruct.gguf"), ModelParams::Params13B);
        assert_eq!(ModelParams::from_filename("qwen-70b-chat.gguf"), ModelParams::Params70B);
        // Test decimal params
        assert_eq!(ModelParams::from_filename("phi-1.5b-q4_k_m.gguf"), ModelParams::Params1B);
        assert_eq!(ModelParams::from_filename("model-18.4B-q4_k.gguf"), ModelParams::Params13B); // 18.4B rounds to 13B tier
        // Test MoE naming - should pick up the total params, not expert count
        assert_eq!(
            ModelParams::from_filename("L3.2-8X3B-MOE-Dark-Champion-Inst-18.4B-uncen-ablit_D_AU-IQ4_XS.gguf"),
            ModelParams::Params13B // 18.4B falls into 13B tier (10-20B range)
        );
    }

    #[test]
    fn test_compatibility_rating() {
        // Create a mock device with 16GB RAM, 8 cores with AVX2, 100GB disk
        let specs = DeviceSpecs {
            total_ram_mb: 16384,
            available_ram_mb: 14000,
            total_ram_gb: 16.0,
            available_ram_gb: 14.0,
            cpu_cores: 8,
            cpu_model: "Test CPU".to_string(),
            cpu_has_avx2: true,
            os: "Linux".to_string(),
            available_disk_gb: 100.0,
            total_disk_gb: 500.0,
            models_dir: "/models".to_string(),
        };

        // Test 1: Small 7B Q4 model (~3.5GB file) on 16GB system
        // Required RAM: 3.5 * 1.3 = 4.55GB, perf_score = (8*1.0/2.0)*1.0 = 4.0 → Efficient
        let compat = check_model_compatibility_with_context(&specs, 3500, None, Some("llama-7b-q4_k_m.gguf"), 4096);
        assert_eq!(compat.rating, CompatibilityRating::Efficient, "7B Q4 on 16GB should be Efficient");

        // Test 2: Insufficient disk space - should be WontRun
        let low_disk_specs = DeviceSpecs {
            available_disk_gb: 1.0,
            ..specs.clone()
        };
        let compat = check_model_compatibility(&low_disk_specs, 5000, None);
        assert_eq!(compat.rating, CompatibilityRating::WontRun, "Insufficient disk should be WontRun");
        assert!(!compat.disk_space_sufficient);
        
        // Test 3: Model too large for RAM - should be WontRun
        let small_ram_specs = DeviceSpecs {
            total_ram_mb: 4096,
            available_ram_mb: 3500,
            total_ram_gb: 4.0,
            available_ram_gb: 3.5,
            ..specs.clone()
        };
        // 13B Q4 model (~7GB) needs ~9GB RAM
        let compat = check_model_compatibility_with_context(&small_ram_specs, 7000, None, Some("llama-13b-q4.gguf"), 4096);
        assert_eq!(compat.rating, CompatibilityRating::WontRun, "13B on 4GB RAM should be WontRun");
    }
    
    #[test]
    fn test_avx2_impact() {
        let base_specs = DeviceSpecs {
            total_ram_mb: 16384,
            available_ram_mb: 14000,
            total_ram_gb: 16.0,
            available_ram_gb: 14.0,
            cpu_cores: 4,
            cpu_model: "Test CPU".to_string(),
            cpu_has_avx2: true,
            os: "Linux".to_string(),
            available_disk_gb: 100.0,
            total_disk_gb: 500.0,
            models_dir: "/models".to_string(),
        };
        
        let no_avx2_specs = DeviceSpecs {
            cpu_has_avx2: false,
            ..base_specs.clone()
        };
        
        // With AVX2: cpu_score = 4 * 1.0 = 4
        // Without AVX2: cpu_score = 4 * 0.5 = 2
        let with_avx2 = check_model_compatibility(&base_specs, 3500, None);
        let without_avx2 = check_model_compatibility(&no_avx2_specs, 3500, None);
        
        assert!(with_avx2.calculation.cpu_score > without_avx2.calculation.cpu_score,
            "AVX2 should increase CPU score");
        assert_eq!(with_avx2.calculation.cpu_score, 4.0);
        assert_eq!(without_avx2.calculation.cpu_score, 2.0);
    }
}
