//! System information detection for device capability assessment.
//!
//! This module provides functions to detect system RAM, CPU, and estimate
//! whether models will run well on the device.

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
    pub os: String,
}

/// Model compatibility rating
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CompatibilityRating {
    /// Model will run smoothly with good performance
    Good,
    /// Model will run but may be slow or use most available RAM
    Medium,
    /// Model is unlikely to run well, may cause system instability
    Bad,
    /// Cannot determine compatibility
    Unknown,
}

/// Compatibility check result for a specific model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCompatibility {
    pub rating: CompatibilityRating,
    pub reason: String,
    pub estimated_memory_mb: u32,
    pub available_memory_mb: u64,
    pub can_run: bool,
}

/// Get current device specifications
pub fn get_device_specs() -> DeviceSpecs {
    let mut sys = System::new_all();
    sys.refresh_all();

    let total_ram_kb = sys.total_memory();
    let available_ram_kb = sys.available_memory();

    // Convert from KB to MB and GB
    let total_ram_mb = total_ram_kb / 1024;
    let available_ram_mb = available_ram_kb / 1024;
    let total_ram_gb = total_ram_mb as f64 / 1024.0;
    let available_ram_gb = available_ram_mb as f64 / 1024.0;

    let cpu_model = sys
        .cpus()
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    DeviceSpecs {
        total_ram_mb,
        available_ram_mb,
        total_ram_gb,
        available_ram_gb,
        cpu_cores: sys.cpus().len(),
        cpu_model,
        os: std::env::consts::OS.to_string(),
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

/// Check if a model is compatible with the current device
/// 
/// Rating thresholds based on percentage of TOTAL RAM:
/// - Good: Model requires < 75% of total RAM
/// - Medium: Model requires 75-90% of total RAM  
/// - Bad: Model requires > 90% of total RAM
pub fn check_model_compatibility(
    specs: &DeviceSpecs,
    model_file_size_mb: u64,
    model_memory_mb: Option<u32>,
) -> ModelCompatibility {
    // Use provided memory estimate or calculate from file size
    let estimated_memory = model_memory_mb.unwrap_or_else(|| {
        estimate_memory_from_file_size(model_file_size_mb)
    });

    let total_ram = specs.total_ram_mb;
    let available = specs.available_ram_mb;
    
    // Calculate percentage of total RAM this model would use
    let usage_percent = (estimated_memory as f64 / total_ram as f64) * 100.0;
    
    // Determine rating based on percentage of TOTAL RAM
    let (rating, can_run, reason) = if usage_percent > 90.0 {
        (
            CompatibilityRating::Bad,
            estimated_memory as u64 <= available,
            format!(
                "Model requires ~{:.1}GB ({:.0}% of total RAM). May cause system instability.",
                estimated_memory as f64 / 1024.0, usage_percent
            ),
        )
    } else if usage_percent > 75.0 {
        (
            CompatibilityRating::Medium,
            true,
            format!(
                "Model requires ~{:.1}GB ({:.0}% of total RAM). Should work but system may slow down.",
                estimated_memory as f64 / 1024.0, usage_percent
            ),
        )
    } else {
        (
            CompatibilityRating::Good,
            true,
            format!(
                "Model requires ~{:.1}GB ({:.0}% of total RAM). Should run smoothly.",
                estimated_memory as f64 / 1024.0, usage_percent
            ),
        )
    };

    ModelCompatibility {
        rating,
        reason,
        estimated_memory_mb: estimated_memory,
        available_memory_mb: available,
        can_run,
    }
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
    fn test_compatibility_rating() {
        // Create a mock device with 8GB RAM
        let specs = DeviceSpecs {
            total_ram_mb: 8192,
            available_ram_mb: 8192,
            total_ram_gb: 8.0,
            available_ram_gb: 8.0,
            cpu_cores: 4,
            cpu_model: "Test CPU".to_string(),
            os: "Linux".to_string(),
        };

        // Test 1: Small model (~1000 MB file → ~1800 MB estimated) 
        // 1800 / 8192 = 22% → Good
        let compat = check_model_compatibility(&specs, 1000, None);
        assert_eq!(compat.rating, CompatibilityRating::Good, "Small model should be Good");

        // Test 2: Model that uses 75-90% of RAM → Medium
        // 8192 * 0.80 = 6554 MB needed
        // To get ~6500 MB estimate: (size * 1.3 + 500) = 6500 → size ≈ 4615 MB
        let compat = check_model_compatibility(&specs, 4600, None);
        assert_eq!(compat.rating, CompatibilityRating::Medium, "Medium model should be Medium");

        // Test 3: Model that uses >90% of RAM → Bad
        // 8192 * 0.92 = 7537 MB needed
        // To get ~7500 MB estimate: (size * 1.3 + 500) = 7500 → size ≈ 5385 MB
        let compat = check_model_compatibility(&specs, 5400, None);
        assert_eq!(compat.rating, CompatibilityRating::Bad, "Large model should be Bad");

        // Test 4: Explicit memory requirement over 90% - should be Bad
        let compat = check_model_compatibility(&specs, 2000, Some(8000));
        assert_eq!(compat.rating, CompatibilityRating::Bad, "Model exceeding 90% RAM should be Bad");

        // Test 5: Explicit memory requirement well within limit (<75%) - should be Good
        let compat = check_model_compatibility(&specs, 2000, Some(2000));
        assert_eq!(compat.rating, CompatibilityRating::Good, "Small model with explicit memory should be Good");
    }
}
