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
    pub available_disk_gb: f64,
    pub total_disk_gb: f64,
}

/// Model compatibility rating
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CompatibilityRating {
    /// Model will run smoothly with optimal performance
    Efficient,
    /// Model will run but with acceptable performance
    Satisfies,
    /// Model is not recommended - may have poor performance or fail
    NotRecommended,
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
    pub disk_space_sufficient: bool,
    pub required_disk_gb: f64,
    pub available_disk_gb: f64,
    pub ram_usage_percent: f64,
    pub disk_usage_percent: f64,
    pub cpu_suitable: bool,
    pub details: CompatibilityDetails,
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

    // Get disk space for models directory
    let models_dir = crate::paths::resolve_models_dir();
    let (available_disk_gb, total_disk_gb) = get_disk_space(&models_dir);

    DeviceSpecs {
        total_ram_mb,
        available_ram_mb,
        total_ram_gb,
        available_ram_gb,
        cpu_cores: sys.cpus().len(),
        cpu_model,
        os: std::env::consts::OS.to_string(),
        available_disk_gb,
        total_disk_gb,
    }
}

/// Get disk space for a given path (returns available_gb, total_gb)
fn get_disk_space(path: &std::path::Path) -> (f64, f64) {
    use sysinfo::Disks;
    
    let disks = Disks::new_with_refreshed_list();
    
    // Find the disk containing this path
    for disk in disks.list() {
        if path.starts_with(disk.mount_point()) {
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

/// Check if a model is compatible with the current device
/// 
/// Rating thresholds:
/// - Efficient: < 60% RAM usage, >= 4 CPU cores, sufficient disk space (2x model size)
/// - Satisfies: 60-80% RAM usage, >= 2 CPU cores, sufficient disk space (1.5x model size)
/// - NotRecommended: > 80% RAM usage, < 2 CPU cores, or insufficient disk space
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
    let available_ram = specs.available_ram_mb;
    
    // Calculate percentage of total RAM this model would use
    let ram_usage_percent = (estimated_memory as f64 / total_ram as f64) * 100.0;
    
    // Disk space requirements: model file + overhead for unpacking/temp files
    let model_file_gb = model_file_size_mb as f64 / 1024.0;
    let required_disk_gb = model_file_gb * 2.0; // 2x for safety (unpacking, temp files)
    let disk_space_sufficient = specs.available_disk_gb >= required_disk_gb;
    let disk_usage_percent = (required_disk_gb / specs.total_disk_gb) * 100.0;
    
    // CPU suitability check
    let cpu_suitable = specs.cpu_cores >= 2;
    let cpu_optimal = specs.cpu_cores >= 4;
    
    // Build detailed checks
    let mut performance_notes = Vec::new();
    
    // RAM check
    let ram_check = if ram_usage_percent < 60.0 {
        format!("✓ Excellent - Model uses {:.1}% of total RAM ({:.1}GB / {:.1}GB)", 
                ram_usage_percent, estimated_memory as f64 / 1024.0, total_ram as f64 / 1024.0)
    } else if ram_usage_percent < 80.0 {
        performance_notes.push("System may slow down during inference".to_string());
        format!("⚠ Adequate - Model uses {:.1}% of total RAM ({:.1}GB / {:.1}GB)", 
                ram_usage_percent, estimated_memory as f64 / 1024.0, total_ram as f64 / 1024.0)
    } else {
        performance_notes.push("High memory usage may cause instability".to_string());
        format!("✗ Insufficient - Model uses {:.1}% of total RAM ({:.1}GB / {:.1}GB)", 
                ram_usage_percent, estimated_memory as f64 / 1024.0, total_ram as f64 / 1024.0)
    };
    
    // Disk check
    let disk_check = if disk_space_sufficient {
        if specs.available_disk_gb >= required_disk_gb * 2.0 {
            format!("✓ Excellent - {:.1}GB available (requires ~{:.1}GB)", 
                    specs.available_disk_gb, required_disk_gb)
        } else {
            format!("✓ Sufficient - {:.1}GB available (requires ~{:.1}GB)", 
                    specs.available_disk_gb, required_disk_gb)
        }
    } else {
        performance_notes.push(format!("Need {:.1}GB more disk space", required_disk_gb - specs.available_disk_gb));
        format!("✗ Insufficient - {:.1}GB available (requires ~{:.1}GB)", 
                specs.available_disk_gb, required_disk_gb)
    };
    
    // CPU check
    let cpu_check = if cpu_optimal {
        format!("✓ Optimal - {} cores ({})", specs.cpu_cores, specs.cpu_model)
    } else if cpu_suitable {
        performance_notes.push("More CPU cores recommended for better performance".to_string());
        format!("⚠ Adequate - {} cores ({})", specs.cpu_cores, specs.cpu_model)
    } else {
        performance_notes.push("CPU may struggle with this model".to_string());
        format!("✗ Limited - {} cores ({})", specs.cpu_cores, specs.cpu_model)
    };
    
    // Determine overall rating based on all factors
    let can_run = estimated_memory as u64 <= available_ram && disk_space_sufficient;
    
    let (rating, reason) = if !disk_space_sufficient {
        (
            CompatibilityRating::NotRecommended,
            format!("Insufficient disk space. Need {:.1}GB free, have {:.1}GB.", 
                    required_disk_gb, specs.available_disk_gb),
        )
    } else if ram_usage_percent > 80.0 || !cpu_suitable {
        (
            CompatibilityRating::NotRecommended,
            format!("Not recommended: High resource usage ({:.0}% RAM). May cause system instability.", 
                    ram_usage_percent),
        )
    } else if ram_usage_percent > 60.0 || !cpu_optimal {
        (
            CompatibilityRating::Satisfies,
            format!("Will work but performance may be limited ({:.0}% RAM, {} cores).", 
                    ram_usage_percent, specs.cpu_cores),
        )
    } else {
        (
            CompatibilityRating::Efficient,
            format!("Optimal performance expected ({:.0}% RAM, {} cores).", 
                    ram_usage_percent, specs.cpu_cores),
        )
    };
    
    // Add context-specific performance notes
    if estimated_memory as u64 > available_ram * 3 / 4 {
        performance_notes.push("Consider closing other applications before running".to_string());
    }
    if model_file_size_mb > 5000 {
        performance_notes.push("Large model - longer load times expected".to_string());
    }

    ModelCompatibility {
        rating,
        reason,
        estimated_memory_mb: estimated_memory,
        available_memory_mb: available_ram,
        can_run,
        disk_space_sufficient,
        required_disk_gb,
        available_disk_gb: specs.available_disk_gb,
        ram_usage_percent,
        disk_usage_percent,
        cpu_suitable: cpu_optimal,
        details: CompatibilityDetails {
            ram_check,
            disk_check,
            cpu_check,
            performance_notes,
        },
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
        // Create a mock device with 8GB RAM, 4 cores, 100GB disk
        let specs = DeviceSpecs {
            total_ram_mb: 8192,
            available_ram_mb: 8192,
            total_ram_gb: 8.0,
            available_ram_gb: 8.0,
            cpu_cores: 4,
            cpu_model: "Test CPU".to_string(),
            os: "Linux".to_string(),
            available_disk_gb: 100.0,
            total_disk_gb: 500.0,
        };

        // Test 1: Small model (~1000 MB file → ~1800 MB estimated) 
        // 1800 / 8192 = 22% → Efficient
        let compat = check_model_compatibility(&specs, 1000, None);
        assert_eq!(compat.rating, CompatibilityRating::Efficient, "Small model should be Efficient");

        // Test 2: Model that uses 60-80% of RAM → Satisfies
        // 8192 * 0.70 = 5734 MB needed
        // To get ~5700 MB estimate: (size * 1.3 + 500) = 5700 → size ≈ 4000 MB
        let compat = check_model_compatibility(&specs, 4000, None);
        assert_eq!(compat.rating, CompatibilityRating::Satisfies, "Medium model should be Satisfies");

        // Test 3: Model that uses >80% of RAM → NotRecommended
        // 8192 * 0.85 = 6963 MB needed
        // To get ~7000 MB estimate: (size * 1.3 + 500) = 7000 → size ≈ 5000 MB
        let compat = check_model_compatibility(&specs, 5000, None);
        assert_eq!(compat.rating, CompatibilityRating::NotRecommended, "Large model should be NotRecommended");

        // Test 4: Explicit memory requirement over 80% - should be NotRecommended
        let compat = check_model_compatibility(&specs, 2000, Some(7000));
        assert_eq!(compat.rating, CompatibilityRating::NotRecommended, "Model exceeding 80% RAM should be NotRecommended");

        // Test 5: Explicit memory requirement well within limit (<60%) - should be Efficient
        let compat = check_model_compatibility(&specs, 2000, Some(2000));
        assert_eq!(compat.rating, CompatibilityRating::Efficient, "Small model with explicit memory should be Efficient");
        
        // Test 6: Insufficient disk space - should be NotRecommended
        let low_disk_specs = DeviceSpecs {
            available_disk_gb: 1.0,
            ..specs.clone()
        };
        let compat = check_model_compatibility(&low_disk_specs, 1000, None);
        assert_eq!(compat.rating, CompatibilityRating::NotRecommended, "Insufficient disk should be NotRecommended");
        assert!(!compat.disk_space_sufficient);
    }
}
