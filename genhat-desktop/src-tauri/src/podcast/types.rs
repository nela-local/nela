//! Types for the Podcast pipeline.

use serde::{Deserialize, Serialize};

/// Frontend request to generate a podcast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodcastRequest {
    /// The topic/question to base the podcast on (goes through RAG).
    pub query: String,
    /// KittenTTS voice name for Speaker A (e.g. "Leo", "Bella").
    pub voice_a: String,
    /// KittenTTS voice name for Speaker B.
    pub voice_b: String,
    /// Display name for Speaker A (e.g. "Alex").
    pub speaker_a_name: String,
    /// Display name for Speaker B (e.g. "Sam").
    pub speaker_b_name: String,
    /// Target number of dialogue turns (alternating lines).
    pub max_turns: usize,
    /// Optional RAG top-k retrieval count.
    ///
    /// If omitted, the backend chooses a dynamic value based on the requested
    /// podcast size and query complexity.
    #[serde(default)]
    pub top_k: Option<usize>,
}

/// A single line of podcast dialogue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodcastLine {
    /// Speaker display name.
    pub speaker: String,
    /// KittenTTS voice name used for this line.
    pub voice: String,
    /// The dialogue text.
    pub text: String,
    /// Line index (0-based ordering).
    pub index: usize,
}

/// The full podcast script (dialogue + metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodcastScript {
    /// Generated title for the podcast episode.
    pub title: String,
    /// Ordered dialogue lines.
    pub lines: Vec<PodcastLine>,
    /// RAG source chunk texts used as context.
    pub source_chunks: Vec<String>,
}

/// A synthesized audio segment for one dialogue line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodcastSegment {
    /// The dialogue line this segment corresponds to.
    pub line: PodcastLine,
    /// Base64 data URL of the WAV audio for this line.
    pub audio_data_url: String,
}

/// Final result returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodcastResult {
    /// The generated script.
    pub script: PodcastScript,
    /// Individual audio segments (one per line).
    pub segments: Vec<PodcastSegment>,
    /// Base64 data URL of the full merged podcast audio.
    pub combined_audio_data_url: String,
}

/// Progress event emitted during podcast generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodcastProgress {
    /// Current stage: "rag", "scripting", "tts", "merging", "done".
    pub stage: String,
    /// Human-readable detail message.
    pub detail: String,
    /// Progress fraction (0.0 to 1.0).
    pub progress: f32,
}
