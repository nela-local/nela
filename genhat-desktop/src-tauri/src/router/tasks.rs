//! Task definitions and helpers.
//!
//! Convenience constructors for common TaskRequest patterns used internally
//! by the RAG pipeline and other subsystems.

use crate::registry::types::{TaskRequest, TaskType};
use std::collections::HashMap;

/// Create a chat task request.
pub fn chat_request(input: &str) -> TaskRequest {
    TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Chat,
        input: input.to_string(),
        model_override: None,
        extra: HashMap::new(),
    }
}

/// Create a summarization task request.
pub fn summarize_request(input: &str) -> TaskRequest {
    TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Summarize,
        input: input.to_string(),
        model_override: None,
        extra: HashMap::new(),
    }
}

/// Create a mindmap generation task request.
pub fn mindmap_request(input: &str) -> TaskRequest {
    TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Mindmap,
        input: input.to_string(),
        model_override: None,
        extra: HashMap::new(),
    }
}

/// Create a TTS task request.
pub fn tts_request(text: &str) -> TaskRequest {
    TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Tts,
        input: text.to_string(),
        model_override: None,
        extra: HashMap::new(),
    }
}

/// Create an embedding task request (for RAG pipeline).
pub fn embed_request(texts: Vec<String>) -> TaskRequest {
    TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Embed,
        input: serde_json::to_string(&texts).unwrap_or_default(),
        model_override: None,
        extra: HashMap::new(),
    }
}

/// Create a classification task request (for RAG query routing).
pub fn classify_request(query: &str) -> TaskRequest {
    TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Classify,
        input: query.to_string(),
        model_override: None,
        extra: HashMap::new(),
    }
}

/// Create an enrichment task request (for RAG Phase 2 background enrichment).
pub fn enrich_request(chunk_text: &str) -> TaskRequest {
    TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Enrich,
        input: chunk_text.to_string(),
        model_override: None,
        extra: HashMap::new(),
    }
}

/// Create a grading task request (for RAG retrieval grading).
pub fn grade_request(query: &str, context: &str) -> TaskRequest {
    TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Grade,
        input: format!("Query: {query}\n\nContext: {context}"),
        model_override: None,
        extra: HashMap::new(),
    }
}

/// Create a HyDE (Hypothetical Document Embedding) request.
pub fn hyde_request(query: &str) -> TaskRequest {
    TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Hyde,
        input: query.to_string(),
        model_override: None,
        extra: HashMap::new(),
    }
}

/// Create a transcription task request (for Whisper STT).
pub fn transcribe_request(audio_path: &str) -> TaskRequest {
    TaskRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        task_type: TaskType::Transcribe,
        input: audio_path.to_string(),
        model_override: None,
        extra: HashMap::new(),
    }
}
