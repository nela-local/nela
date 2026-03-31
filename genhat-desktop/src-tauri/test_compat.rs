// Test scenario: 8GB system, checking 2GB model

// 8GB = 8589934592 bytes = 8388608 KB = 8192 MB
let total_ram_mb = 8192;
let available_ram_mb = 8192 - 512; // 7680 MB available (512 MB reserved by system)
let safe_available = available_ram_mb - 1024; // 6656 MB safe

// 2GB model = 2048 MB
let model_file_size_mb = 2048;
let base_estimate = (model_file_size_mb as f64 * 1.3) as u32;  // 2662
let estimated_memory = base_estimate + 500; // 3162 MB
let estimated_memory_rounded = ((estimated_memory + 50) / 100) * 100; // 3200 MB

println!("Available RAM: {} MB", available_ram_mb);
println!("Safe available: {} MB", safe_available);
println!("Estimated memory: {} MB", estimated_memory_rounded);
println!("Can run: {}", estimated_memory_rounded as u64 <= available_ram_mb);
println!("Is good: {}", estimated_memory_rounded as u64 <= safe_available / 2);
println!("Is medium: {}", estimated_memory_rounded as u64 > safe_available / 2 && estimated_memory_rounded as u64 <= safe_available);

// Result should be:
// Available RAM: 7680 MB
// Safe available: 6656 MB
// Estimated memory: 3200 MB
// Can run: true
// Is good: true (3200 <= 3328)
// Is medium: false
