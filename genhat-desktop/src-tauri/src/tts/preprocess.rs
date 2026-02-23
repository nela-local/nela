//! Text preprocessing for TTS — normalizes numbers, currency, contractions,
//! ordinals, percentages, time expressions, and other special forms into
//! speakable English words.
//!
//! This is a faithful port of KittenTTS's `preprocess.py` (966 lines of Python)
//! into idiomatic Rust. Only the expansions relevant to TTS are included.

use regex::Regex;
use std::sync::LazyLock;

/// Wrapper for fancy_regex patterns that need look-around assertions.
/// Falls back to a simple find-and-replace loop since fancy_regex::replace_all
/// has a different API than regex::replace_all.
fn fancy_replace_all(text: &str, pattern: &fancy_regex::Regex, replacer: impl Fn(&fancy_regex::Captures) -> String) -> String {
    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;
    // Use find_iter + captures to replace
    let mut search_start = 0;
    while search_start <= text.len() {
        match pattern.captures_from_pos(text, search_start) {
            Ok(Some(caps)) => {
                let m = caps.get(0).unwrap();
                result.push_str(&text[last_end..m.start()]);
                result.push_str(&replacer(&caps));
                search_start = m.end();
                last_end = m.end();
                if m.start() == m.end() {
                    // Zero-length match: advance by one char to avoid infinite loop
                    if search_start < text.len() {
                        result.push_str(&text[search_start..search_start + text[search_start..].chars().next().map_or(1, |c| c.len_utf8())]);
                        let adv = text[search_start..].chars().next().map_or(1, |c| c.len_utf8());
                        search_start += adv;
                        last_end += adv;
                    } else {
                        break;
                    }
                }
            }
            _ => break,
        }
    }
    result.push_str(&text[last_end..]);
    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// Number → Words
// ═══════════════════════════════════════════════════════════════════════════════

const ONES: &[&str] = &[
    "", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
    "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen",
    "seventeen", "eighteen", "nineteen",
];

const TENS: &[&str] = &[
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

const SCALE: &[&str] = &["", "thousand", "million", "billion", "trillion"];

fn three_digits_to_words(n: u64) -> String {
    if n == 0 {
        return String::new();
    }
    let mut parts = Vec::new();
    let hundreds = n / 100;
    let remainder = n % 100;
    if hundreds > 0 {
        parts.push(format!("{} hundred", ONES[hundreds as usize]));
    }
    if remainder < 20 {
        if remainder > 0 {
            parts.push(ONES[remainder as usize].to_string());
        }
    } else {
        let tens_word = TENS[(remainder / 10) as usize];
        let ones_word = ONES[(remainder % 10) as usize];
        if ones_word.is_empty() {
            parts.push(tens_word.to_string());
        } else {
            parts.push(format!("{tens_word}-{ones_word}"));
        }
    }
    parts.join(" ")
}

/// Convert an integer to English words.
pub fn number_to_words(n: i64) -> String {
    if n == 0 {
        return "zero".to_string();
    }
    if n < 0 {
        return format!("negative {}", number_to_words(-n));
    }
    let n = n as u64;

    // X00–X999 read as "X hundred" (e.g. 1200 → "twelve hundred")
    if (100..=9999).contains(&n) && n % 100 == 0 && n % 1000 != 0 {
        let hundreds = n / 100;
        if hundreds < 20 {
            return format!("{} hundred", ONES[hundreds as usize]);
        }
    }

    let mut parts = Vec::new();
    let mut remaining = n;
    for (i, &scale) in SCALE.iter().enumerate() {
        let chunk = remaining % 1000;
        if chunk > 0 {
            let chunk_words = three_digits_to_words(chunk);
            if scale.is_empty() {
                parts.push(chunk_words);
            } else {
                parts.push(format!("{chunk_words} {scale}"));
            }
        }
        remaining /= 1000;
        if remaining == 0 {
            break;
        }
        let _ = i;
    }
    parts.reverse();
    parts.join(" ")
}

/// Convert a float to English words, reading decimal digits individually.
pub fn float_to_words(value: &str) -> String {
    let text = value.trim();
    let (negative, text) = if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else {
        (false, text)
    };

    let result = if let Some((int_part, dec_part)) = text.split_once('.') {
        let int_words = if int_part.is_empty() {
            "zero".to_string()
        } else {
            number_to_words(int_part.parse::<i64>().unwrap_or(0))
        };
        let digit_names = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
        let dec_words: Vec<&str> = dec_part
            .chars()
            .filter_map(|c| c.to_digit(10).map(|d| digit_names[d as usize]))
            .collect();
        format!("{int_words} point {}", dec_words.join(" "))
    } else {
        number_to_words(text.parse::<i64>().unwrap_or(0))
    };

    if negative {
        format!("negative {result}")
    } else {
        result
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Ordinals
// ═══════════════════════════════════════════════════════════════════════════════

fn ordinal_suffix(n: i64) -> String {
    let word = number_to_words(n);
    let exceptions: &[(&str, &str)] = &[
        ("one", "first"), ("two", "second"), ("three", "third"), ("four", "fourth"),
        ("five", "fifth"), ("six", "sixth"), ("seven", "seventh"), ("eight", "eighth"),
        ("nine", "ninth"), ("twelve", "twelfth"),
    ];

    let (prefix, last, joiner) = if let Some(pos) = word.rfind('-') {
        (&word[..pos], &word[pos + 1..], "-")
    } else if let Some(pos) = word.rfind(' ') {
        (&word[..pos], &word[pos + 1..], " ")
    } else {
        ("", word.as_str(), "")
    };

    let last_ord = exceptions
        .iter()
        .find(|&&(base, _)| base == last)
        .map(|&(_, ord)| ord.to_string())
        .unwrap_or_else(|| {
            if last.ends_with('t') {
                format!("{last}h")
            } else if last.ends_with('e') {
                format!("{}th", &last[..last.len() - 1])
            } else {
                format!("{last}th")
            }
        });

    if prefix.is_empty() {
        last_ord
    } else {
        format!("{prefix}{joiner}{last_ord}")
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Compiled regexes (lazy statics)
// ═══════════════════════════════════════════════════════════════════════════════

static RE_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://\S+|www\.\S+").unwrap());
static RE_EMAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b[\w.+-]+@[\w-]+\.[a-z]{2,}\b").unwrap());
static RE_HTML: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static RE_PUNCT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\w\s.,?!;:\x{2014}\x{2013}\x{2026}-]").unwrap());
static RE_SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
// Patterns with look-around use fancy_regex
static RE_NUMBER: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"(?<![a-zA-Z])-?[\d,]+(?:\.\d+)?").unwrap());
static RE_ORDINAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(\d+)(st|nd|rd|th)\b").unwrap());
static RE_PERCENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(-?[\d,]+(?:\.\d+)?)\s*%").unwrap());
static RE_CURRENCY: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"([$€£¥₹₩₿])\s*([\d,]+(?:\.\d+)?)\s*([KMBT])?(?![a-zA-Z\d])").unwrap());
static RE_TIME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(\d{1,2}):(\d{2})(?::(\d{2}))?\s*(am|pm)?\b").unwrap());
static RE_RANGE: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"(?<!\w)(\d+)-(\d+)(?!\w)").unwrap());
static RE_MODEL_VER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([a-zA-Z][a-zA-Z0-9]*)-(\d[\d.]*)(?:$|[^\d.])").unwrap());
static RE_UNIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(\d+(?:\.\d+)?)\s*(km|kg|mg|ml|gb|mb|kb|tb|hz|khz|mhz|ghz|mph|kph|°[cCfF]|[cCfF]°|ms|ns|µs)\b").unwrap()
});
static RE_SCALE: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"(?<![a-zA-Z])(\d+(?:\.\d+)?)\s*([KMBT])(?![a-zA-Z\d])").unwrap());
static RE_SCI: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"(?<![a-zA-Z\d])(-?\d+(?:\.\d+)?)[eE]([+-]?\d+)(?![a-zA-Z\d])").unwrap());
static RE_FRACTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(\d+)\s*/\s*(\d+)\b").unwrap());
static RE_DECADE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(\d{1,3})0s\b").unwrap());
static RE_PHONE_11: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"(?<!\d-)(?<!\d)\b(\d{1,2})-(\d{3})-(\d{3})-(\d{4})\b(?!-\d)").unwrap());
static RE_PHONE_10: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"(?<!\d-)(?<!\d)\b(\d{3})-(\d{3})-(\d{4})\b(?!-\d)").unwrap());
static RE_PHONE_7: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"(?<!\d-)\b(\d{3})-(\d{4})\b(?!-\d)").unwrap());
static RE_IP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b").unwrap());

// ═══════════════════════════════════════════════════════════════════════════════
// Expansion functions
// ═══════════════════════════════════════════════════════════════════════════════

fn expand_ordinals(text: &str) -> String {
    RE_ORDINAL.replace_all(text, |caps: &regex::Captures| {
        let n: i64 = caps[1].parse().unwrap_or(0);
        ordinal_suffix(n)
    }).to_string()
}

fn expand_percentages(text: &str) -> String {
    RE_PERCENT.replace_all(text, |caps: &regex::Captures| {
        let raw = caps[1].replace(',', "");
        if raw.contains('.') {
            format!("{} percent", float_to_words(&raw))
        } else {
            format!("{} percent", number_to_words(raw.parse().unwrap_or(0)))
        }
    }).to_string()
}

fn expand_currency(text: &str) -> String {
    let currency_map: &[(&str, &str)] = &[
        ("$", "dollar"), ("€", "euro"), ("£", "pound"), ("¥", "yen"),
        ("₹", "rupee"), ("₩", "won"), ("₿", "bitcoin"),
    ];
    let scale_map: &[(&str, &str)] = &[
        ("K", "thousand"), ("M", "million"), ("B", "billion"), ("T", "trillion"),
    ];

    fancy_replace_all(text, &RE_CURRENCY, |caps: &fancy_regex::Captures| {
        let symbol = caps.get(1).unwrap().as_str();
        let raw = caps.get(2).unwrap().as_str().replace(',', "");
        let scale_suffix = caps.get(3).map(|m| m.as_str());
        let unit = currency_map.iter().find(|&&(s, _)| s == symbol).map(|&(_, u)| u).unwrap_or("");

        if let Some(suffix) = scale_suffix {
            let scale_word = scale_map.iter().find(|&&(s, _)| s == suffix).map(|&(_, w)| w).unwrap_or(suffix);
            let num = if raw.contains('.') { float_to_words(&raw) } else { number_to_words(raw.parse().unwrap_or(0)) };
            return format!("{num} {scale_word} {unit}s");
        }

        if raw.contains('.') {
            let parts: Vec<&str> = raw.split('.').collect();
            let int_part = parts[0].parse::<i64>().unwrap_or(0);
            let dec_str = parts.get(1).unwrap_or(&"0");
            let dec_val: i64 = format!("{:0<2}", &dec_str[..dec_str.len().min(2)]).parse().unwrap_or(0);
            let int_words = number_to_words(int_part);
            let mut result = if !unit.is_empty() { format!("{int_words} {unit}s") } else { int_words };
            if dec_val > 0 {
                let cents = number_to_words(dec_val);
                let cents_unit = if dec_val != 1 { "cents" } else { "cent" };
                result = format!("{result} and {cents} {cents_unit}");
            }
            result
        } else {
            let val = raw.parse::<i64>().unwrap_or(0);
            let words = number_to_words(val);
            if !unit.is_empty() {
                let plural = if val != 1 { "s" } else { "" };
                format!("{words} {unit}{plural}")
            } else {
                words
            }
        }
    })
}

fn expand_time(text: &str) -> String {
    RE_TIME.replace_all(text, |caps: &regex::Captures| {
        let h: i64 = caps[1].parse().unwrap_or(0);
        let mins: i64 = caps[2].parse().unwrap_or(0);
        let suffix = caps.get(4).map(|m| format!(" {}", m.as_str().to_lowercase())).unwrap_or_default();
        let h_words = number_to_words(h);
        if mins == 0 {
            if caps.get(4).is_some() { format!("{h_words}{suffix}") } else { format!("{h_words} hundred{suffix}") }
        } else if mins < 10 {
            format!("{h_words} oh {}{suffix}", number_to_words(mins))
        } else {
            format!("{h_words} {}{suffix}", number_to_words(mins))
        }
    }).to_string()
}

fn expand_ranges(text: &str) -> String {
    fancy_replace_all(text, &RE_RANGE, |caps: &fancy_regex::Captures| {
        let lo = number_to_words(caps.get(1).unwrap().as_str().parse().unwrap_or(0));
        let hi = number_to_words(caps.get(2).unwrap().as_str().parse().unwrap_or(0));
        format!("{lo} to {hi}")
    })
}

fn expand_model_names(text: &str) -> String {
    RE_MODEL_VER.replace_all(text, |caps: &regex::Captures| {
        format!("{} {}", &caps[1], &caps[2])
    }).to_string()
}

fn expand_units(text: &str) -> String {
    let unit_map: &[(&str, &str)] = &[
        ("km", "kilometers"), ("kg", "kilograms"), ("mg", "milligrams"),
        ("ml", "milliliters"), ("gb", "gigabytes"), ("mb", "megabytes"),
        ("kb", "kilobytes"), ("tb", "terabytes"),
        ("hz", "hertz"), ("khz", "kilohertz"), ("mhz", "megahertz"), ("ghz", "gigahertz"),
        ("mph", "miles per hour"), ("kph", "kilometers per hour"),
        ("ms", "milliseconds"), ("ns", "nanoseconds"), ("µs", "microseconds"),
        ("°c", "degrees Celsius"), ("c°", "degrees Celsius"),
        ("°f", "degrees Fahrenheit"), ("f°", "degrees Fahrenheit"),
    ];

    RE_UNIT.replace_all(text, |caps: &regex::Captures| {
        let raw = &caps[1];
        let unit = caps[2].to_lowercase();
        let expanded = unit_map.iter().find(|&&(u, _)| u == unit).map(|&(_, e)| e).unwrap_or(&caps[2]);
        let num = if raw.contains('.') { float_to_words(raw) } else { number_to_words(raw.parse().unwrap_or(0)) };
        format!("{num} {expanded}")
    }).to_string()
}

fn expand_scale_suffixes(text: &str) -> String {
    let map: &[(&str, &str)] = &[("K", "thousand"), ("M", "million"), ("B", "billion"), ("T", "trillion")];
    fancy_replace_all(text, &RE_SCALE, |caps: &fancy_regex::Captures| {
        let raw = caps.get(1).unwrap().as_str();
        let suffix = caps.get(2).unwrap().as_str();
        let scale_word = map.iter().find(|&&(s, _)| s == suffix).map(|&(_, w)| w).unwrap_or(suffix);
        let num = if raw.contains('.') { float_to_words(raw) } else { number_to_words(raw.parse().unwrap_or(0)) };
        format!("{num} {scale_word}")
    })
}

fn expand_scientific(text: &str) -> String {
    fancy_replace_all(text, &RE_SCI, |caps: &fancy_regex::Captures| {
        let coeff = caps.get(1).unwrap().as_str();
        let exp: i64 = caps.get(2).unwrap().as_str().parse().unwrap_or(0);
        let coeff_words = if coeff.contains('.') { float_to_words(coeff) } else { number_to_words(coeff.parse().unwrap_or(0)) };
        let sign = if exp < 0 { "negative " } else { "" };
        let exp_words = number_to_words(exp.abs());
        format!("{coeff_words} times ten to the {sign}{exp_words}")
    })
}

fn expand_fractions(text: &str) -> String {
    RE_FRACTION.replace_all(text, |caps: &regex::Captures| {
        let num: i64 = caps[1].parse().unwrap_or(0);
        let den: i64 = caps[2].parse().unwrap_or(0);
        if den == 0 { return caps[0].to_string(); }
        let num_words = number_to_words(num);
        let denom = match den {
            2 => if num == 1 { "half" } else { "halves" },
            4 => if num == 1 { "quarter" } else { "quarters" },
            _ => {
                let ord = ordinal_suffix(den);
                let pluralized = if num != 1 { format!("{ord}s") } else { ord };
                return format!("{num_words} {pluralized}");
            }
        };
        format!("{num_words} {denom}")
    }).to_string()
}

fn expand_decades(text: &str) -> String {
    let map: &[(u64, &str)] = &[
        (0, "hundreds"), (1, "tens"), (2, "twenties"), (3, "thirties"), (4, "forties"),
        (5, "fifties"), (6, "sixties"), (7, "seventies"), (8, "eighties"), (9, "nineties"),
    ];
    RE_DECADE.replace_all(text, |caps: &regex::Captures| {
        let base: u64 = caps[1].parse().unwrap_or(0);
        let decade_digit = base % 10;
        let decade_word = map.iter().find(|&&(d, _)| d == decade_digit).map(|&(_, w)| w).unwrap_or("");
        if base < 10 {
            decade_word.to_string()
        } else {
            let century = base / 10;
            format!("{} {decade_word}", number_to_words(century as i64))
        }
    }).to_string()
}

fn digits_to_words(s: &str) -> String {
    let d = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
    s.chars().filter_map(|c| c.to_digit(10).map(|i| d[i as usize])).collect::<Vec<_>>().join(" ")
}

fn expand_phone_numbers(text: &str) -> String {
    let text = fancy_replace_all(text, &RE_PHONE_11, |caps: &fancy_regex::Captures| {
        format!("{} {} {} {}",
            digits_to_words(caps.get(1).unwrap().as_str()),
            digits_to_words(caps.get(2).unwrap().as_str()),
            digits_to_words(caps.get(3).unwrap().as_str()),
            digits_to_words(caps.get(4).unwrap().as_str()))
    });
    let text = fancy_replace_all(&text, &RE_PHONE_10, |caps: &fancy_regex::Captures| {
        format!("{} {} {}",
            digits_to_words(caps.get(1).unwrap().as_str()),
            digits_to_words(caps.get(2).unwrap().as_str()),
            digits_to_words(caps.get(3).unwrap().as_str()))
    });
    fancy_replace_all(&text, &RE_PHONE_7, |caps: &fancy_regex::Captures| {
        format!("{} {}",
            digits_to_words(caps.get(1).unwrap().as_str()),
            digits_to_words(caps.get(2).unwrap().as_str()))
    })
}

fn expand_ip(text: &str) -> String {
    RE_IP.replace_all(text, |caps: &regex::Captures| {
        let parts: Vec<String> = (1..=4).map(|i| digits_to_words(&caps[i])).collect();
        parts.join(" dot ")
    }).to_string()
}

fn normalize_leading_decimals(text: &str) -> String {
    // Replace -.<digit> → -0.<digit> and .<digit> → 0.<digit> when not preceded by a digit.
    // Rust's regex crate doesn't support look-behind, so we do character-level processing.
    let chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len() + 10);
    let len = chars.len();

    let mut i = 0;
    while i < len {
        if chars[i] == '.' && i + 1 < len && chars[i + 1].is_ascii_digit() {
            // Check what's before: if it's a digit, leave it alone; otherwise prepend '0'
            let prev_is_digit = i > 0 && chars[i - 1].is_ascii_digit();
            if !prev_is_digit {
                // Check for leading minus: if prev is '-' and the char before that is not a digit
                if i >= 1 && chars[i - 1] == '-' && (i < 2 || !chars[i - 2].is_ascii_digit()) {
                    // Already pushed '-', insert '0' before '.'
                    result.push('0');
                } else {
                    result.push('0');
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    result
}

fn expand_contractions(text: &str) -> String {
    let patterns: &[(&str, &str)] = &[
        (r"(?i)\bcan't\b", "cannot"),
        (r"(?i)\bwon't\b", "will not"),
        (r"(?i)\bshan't\b", "shall not"),
        (r"(?i)\bain't\b", "is not"),
        (r"(?i)\blet's\b", "let us"),
        (r"(?i)\b(\w+)n't\b", "$1 not"),
        (r"(?i)\b(\w+)'re\b", "$1 are"),
        (r"(?i)\b(\w+)'ve\b", "$1 have"),
        (r"(?i)\b(\w+)'ll\b", "$1 will"),
        (r"(?i)\b(\w+)'d\b", "$1 would"),
        (r"(?i)\b(\w+)'m\b", "$1 am"),
        (r"(?i)\bit's\b", "it is"),
    ];
    let mut result = text.to_string();
    for &(pattern, replacement) in patterns {
        let re = Regex::new(pattern).unwrap();
        result = re.replace_all(&result, replacement).to_string();
    }
    result
}

fn replace_numbers(text: &str) -> String {
    fancy_replace_all(text, &RE_NUMBER, |caps: &fancy_regex::Captures| {
        let raw = caps.get(0).unwrap().as_str().replace(',', "");
        if raw.contains('.') {
            float_to_words(&raw)
        } else {
            match raw.parse::<i64>() {
                Ok(n) => number_to_words(n),
                Err(_) => caps.get(0).unwrap().as_str().to_string(),
            }
        }
    })
}

fn remove_urls(text: &str) -> String { RE_URL.replace_all(text, "").to_string() }
fn remove_emails(text: &str) -> String { RE_EMAIL.replace_all(text, "").to_string() }
fn remove_html(text: &str) -> String { RE_HTML.replace_all(text, " ").to_string() }
fn remove_punctuation(text: &str) -> String { RE_PUNCT.replace_all(text, " ").to_string() }
fn remove_extra_whitespace(text: &str) -> String { RE_SPACES.replace_all(text, " ").trim().to_string() }

// ═══════════════════════════════════════════════════════════════════════════════
// Public API — TextPreprocessor
// ═══════════════════════════════════════════════════════════════════════════════

/// Configurable text preprocessing pipeline for TTS.
/// Converts raw text into clean, speakable English words.
pub struct TextPreprocessor {
    pub remove_punct: bool,
    pub lowercase: bool,
}

impl Default for TextPreprocessor {
    fn default() -> Self {
        Self {
            remove_punct: false,
            lowercase: true,
        }
    }
}

impl TextPreprocessor {
    /// Create a new preprocessor matching KittenTTS defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Process input text through the full normalization pipeline.
    pub fn process(&self, text: &str) -> String {
        let mut text = text.to_string();

        // Clean HTML/URLs/emails
        text = remove_html(&text);
        text = remove_urls(&text);
        text = remove_emails(&text);

        // Expand contractions
        text = expand_contractions(&text);

        // IP addresses before leading decimal normalization
        text = expand_ip(&text);
        text = normalize_leading_decimals(&text);

        // Expand special numeric forms (order matters)
        text = expand_currency(&text);
        text = expand_percentages(&text);
        text = expand_scientific(&text);
        text = expand_time(&text);
        text = expand_ordinals(&text);
        text = expand_units(&text);
        text = expand_scale_suffixes(&text);
        text = expand_fractions(&text);
        text = expand_decades(&text);
        text = expand_phone_numbers(&text);
        text = expand_ranges(&text);
        text = expand_model_names(&text);

        // Replace remaining bare numbers
        text = replace_numbers(&text);

        // Final cleanup
        if self.remove_punct {
            text = remove_punctuation(&text);
        }
        if self.lowercase {
            text = text.to_lowercase();
        }
        text = remove_extra_whitespace(&text);

        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_number_to_words() {
        assert_eq!(number_to_words(0), "zero");
        assert_eq!(number_to_words(1), "one");
        assert_eq!(number_to_words(42), "forty-two");
        assert_eq!(number_to_words(1200), "twelve hundred");
        assert_eq!(number_to_words(1000), "one thousand");
        assert_eq!(number_to_words(-5), "negative five");
    }

    #[test]
    fn test_preprocessor_basic() {
        let pp = TextPreprocessor::new();
        let out = pp.process("There are 1200 students.");
        assert!(out.contains("twelve hundred"));
    }

    #[test]
    fn test_currency() {
        let pp = TextPreprocessor::new();
        let out = pp.process("A coffee costs $4.99 here.");
        assert!(out.contains("dollar"));
    }
}
