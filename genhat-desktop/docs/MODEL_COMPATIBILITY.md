# Model Compatibility Estimation System

This document explains how the model compatibility system estimates whether a GGUF model will run efficiently on your system.

## Overview

The system analyzes three key factors before downloading a model:
1. **RAM Requirements** - Will the model fit in memory?
2. **CPU Performance** - How fast will inference be?
3. **Disk Space** - Is there enough storage?

---

## 1. Model Detection

### Parameter Size Detection

The system detects model parameter count from the filename:

| Pattern in Filename | Detected Size | Base FP16 Size |
|---------------------|---------------|----------------|
| `1b`, `1.5b`, `2b`  | 1B            | ~2 GB          |
| `3b`, `4b`          | 3B            | ~6 GB          |
| `7b`, `8b`, `9b`    | 7B            | ~13 GB         |
| `13b`-`20b`         | 13B           | ~26 GB         |
| `30b`, `33b`, `34b` | 30B           | ~60 GB         |
| `70b`, `65b`, `72b` | 70B           | ~140 GB        |

#### Decimal and Complex Names

The detection handles:
- **Decimal params**: `1.5b`, `18.4B` → parsed correctly
- **MoE notation**: In `8X3B-MOE-18.4B`, the `8X3B` is skipped (MoE expert count), and `18.4B` is matched as the total params
- **Case insensitive**: `7B`, `7b`, `7B-instruct` all work

If the parameter size cannot be detected from the filename, it is estimated from the file size and quantization level.

### Quantization Detection

Quantization level is detected from filename patterns:

| Pattern            | Level | Size Multiplier | Performance Boost |
|--------------------|-------|-----------------|-------------------|
| `q2_k`, `q2-k`     | Q2    | 0.25×           | 1.6×              |
| `q3_k`, `q3-k`     | Q3    | 0.35×           | 1.3×              |
| `q4_k`, `q4_0`     | Q4    | 0.50×           | 1.0× (baseline)   |
| `q5_k`, `q5_0`     | Q5    | 0.65×           | 0.85×             |
| `q6_k`             | Q6    | 0.75×           | 0.75×             |
| `q8_0`, `q8_k`     | Q8    | 1.00×           | 0.60×             |
| `f16`, `fp16`      | F16   | 1.00×           | 0.50×             |
| `f32`, `fp32`      | F32   | 2.00×           | 0.30×             |

#### I-Quant (Importance Quantization)

The system also detects I-quant formats and maps them to equivalent Q levels:

| I-Quant Pattern    | Maps To | Notes                          |
|--------------------|---------|--------------------------------|
| `iq1_s`, `iq2_xxs` | Q2      | Very compressed                |
| `iq3_xs`, `iq3_s`  | Q3      | Compressed                     |
| `iq4_xs`, `iq4_nl` | Q4      | Balanced (most common I-quant) |

---

## 2. File Size Estimation

### Calculation

```
estimated_file_size = base_fp16_size × quantization_multiplier
```

### Example

For a **13B Q4** model:
```
Base FP16 size (13B): 26 GB
Quantization multiplier (Q4): 0.5
Estimated file size: 26 × 0.5 = 13 GB
```

### Why Estimated vs Actual May Differ

The estimated file size is calculated from lookup tables and may differ from the actual file size because:

1. **Model architecture variations** - Different architectures (LLaMA, Mistral, Qwen) have slightly different parameter counts even at the same "size"
2. **Vocabulary size** - Models with larger vocabularies have larger embedding layers
3. **Quantization variants** - Q4_K_M vs Q4_K_S have different sizes
4. **Additional layers** - Some models include extra components

The **actual file size** (from HuggingFace) is always used for RAM estimation. The estimated size is shown for reference only.

---

## 3. RAM Requirement Estimation

### Calculation

```
required_ram = actual_file_size × ram_multiplier
```

### RAM Multiplier

| Context Length | Multiplier | Reason                           |
|----------------|------------|----------------------------------|
| < 8192 tokens  | 1.3×       | Standard KV cache overhead       |
| ≥ 8192 tokens  | 1.5×       | Large context requires more cache|

### Example

For a **7 GB model file** with **4k context**:
```
File size: 7 GB
RAM multiplier: 1.3 (standard context)
Required RAM: 7 × 1.3 = 9.1 GB
```

### RAM Decision Logic

```
if total_ram < required_ram:
    "DO NOT DOWNLOAD" - Model will not fit in memory

else if total_ram < required_ram × 1.25:
    "NOT RECOMMENDED" - Will run but system may be unstable

else:
    "OK" - Sufficient RAM available
```

---

## 4. Disk Space Requirement

### Calculation

```
required_disk = estimated_file_size
```

The estimated file size (from step 2) is used for the pre-download disk space check.

### Download Location

The disk space is calculated based on the **models directory**:
- The drive letter is extracted from the models directory path
- Space is checked on that specific drive, not system drive
- The path is shown in the UI header as `D:` or similar

### Example

For a **13B Q4 model**:
```
Estimated file size: 13 GB (from base FP16 26GB × 0.5 Q4 multiplier)
Required disk: 13 GB
```

### Disk Decision Logic

```
if available_disk < estimated_file_size:
    "INSUFFICIENT" - Cannot download, not enough space

else:
    "SUFFICIENT" - OK to download
```

---

## 5. CPU Performance Estimation

### CPU Score Calculation

```
cpu_score = cpu_cores × avx2_factor
```

| AVX2 Support | Factor | Notes                              |
|--------------|--------|------------------------------------|
| Yes          | 1.0    | Full SIMD optimization available   |
| No           | 0.5    | ~50% slower inference              |

### Model Scaling Factor

Larger models require more CPU resources per token:

| Model Size | Factor | Relative Load |
|------------|--------|---------------|
| 1B         | 0.5    | Very light    |
| 3B         | 1.0    | Light         |
| 7B         | 2.0    | Moderate      |
| 13B        | 4.0    | Heavy         |
| 30B        | 8.0    | Very heavy    |
| 70B        | 16.0   | Extreme       |

### Quantization Performance Boost

Lower quantization = faster inference (but lower quality):

| Quantization | Boost  | Quality Impact       |
|--------------|--------|----------------------|
| Q2           | 1.6×   | Significant loss     |
| Q3           | 1.3×   | Noticeable loss      |
| Q4           | 1.0×   | Balanced (baseline)  |
| Q5           | 0.85×  | Minimal loss         |
| Q6           | 0.75×  | Very minimal loss    |
| Q8           | 0.60×  | Near-lossless        |
| F16          | 0.50×  | Lossless             |
| F32          | 0.30×  | Lossless (slowest)   |

### Final Performance Score

```
perf_score = (cpu_score / model_factor) × quant_boost
```

### Example

**System:** 8 cores with AVX2  
**Model:** 13B Q4

```
CPU score: 8 × 1.0 = 8.0
Model factor (13B): 4.0
Quant boost (Q4): 1.0

Performance score: (8.0 / 4.0) × 1.0 = 2.0
```

---

## 6. Final Rating

### Rating Thresholds

| Condition                      | Rating          | Icon    |
|--------------------------------|-----------------|---------|
| RAM insufficient               | Won't Run       | ⊘       |
| perf_score < 0.5               | Not Recommended | ⊘       |
| perf_score 0.5 - 1.0           | Very Slow       | ▲       |
| perf_score 1.0 - 2.0           | Usable          | ▲       |
| perf_score ≥ 2.0               | Efficient       | ●       |

### Rating Descriptions

| Rating          | Description                                              |
|-----------------|----------------------------------------------------------|
| Efficient       | Fast inference, smooth performance                       |
| Usable          | Acceptable performance, some delays                      |
| Very Slow       | Works but expect long wait times                         |
| Not Recommended | Poor performance, may cause system issues                |
| Won't Run       | Insufficient resources, will fail to load                |

---

## 7. Alternative Suggestions

When a model is rated poorly, the system suggests alternatives:

| Current Model | System RAM | Suggestion        | Reason                           |
|---------------|------------|-------------------|----------------------------------|
| 70B any       | ≥ 16 GB    | 13B Q4            | Similar quality, much faster     |
| 70B any       | < 16 GB    | 7B Q4             | Good quality, runs on most systems|
| 30B any       | ≥ 12 GB    | 13B Q4            | Better performance               |
| 30B any       | < 12 GB    | 7B Q4             | Good balance                     |
| 13B Q8/F16    | any        | 13B Q4            | Same model, smaller footprint    |
| 13B Q4        | < 12 GB    | 7B Q4             | Fits your RAM                    |
| 7B Q8/F16     | any        | 7B Q4             | Same model, better performance   |
| 7B Q4         | < 8 GB     | 3B Q4             | Fits your RAM                    |

---

## 8. Complete Example

### Input

- **Model:** `llama-3-13b-instruct-q4_k_m.gguf`
- **File Size:** 7.87 GB (actual)
- **System:** 16 GB RAM, 8 cores, AVX2

### Calculations

**Step 1: Detection**
```
Detected params: 13B (from "13b" in filename)
Detected quant: Q4 (from "q4_k_m" in filename)
Base FP16 size: 26 GB
```

**Step 2: File Size Estimation**
```
Estimated: 26 × 0.5 = 13.0 GB
Actual: 7.87 GB
(Difference due to architecture optimizations)
```

**Step 3: RAM Requirement**
```
Required: 7.87 × 1.3 = 10.2 GB
Available: 16 GB
Usage: 64%
Decision: OK
```

**Step 4: CPU Performance**
```
CPU score: 8 × 1.0 = 8.0
Model factor: 4.0
Quant boost: 1.0
Perf score: (8.0 / 4.0) × 1.0 = 2.0
Classification: Fast
```

**Step 5: Final Rating**
```
RAM: OK (64% usage)
Performance: 2.0 (≥ 2.0)
Rating: EFFICIENT
```

---

## 8. Limitations

1. **Estimation accuracy** - Values are approximations based on typical models
2. **System variability** - Actual performance depends on background processes
3. **Architecture differences** - Some models may use more/less memory than estimated
4. **Context length** - Longer contexts significantly increase memory usage
5. **Batch size** - Running multiple requests increases memory requirements

---

## 9. Recommendations

### For Best Performance

1. Choose Q4 quantization for best quality/speed balance
2. Ensure at least 25% RAM headroom above requirements
3. Use models ≤ 7B on systems with < 16 GB RAM
4. Enable AVX2 in BIOS if supported by your CPU

### Model Size Guidelines

| System RAM | Recommended Max Model |
|------------|----------------------|
| 8 GB       | 7B Q4                |
| 16 GB      | 13B Q4               |
| 32 GB      | 30B Q4 or 13B Q8     |
| 64 GB      | 70B Q4               |
