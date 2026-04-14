use crate::commands::models::ProcessManagerState;
use crate::registry::custom;
use crate::registry::types::TaskType;
use futures_util::StreamExt;
use reqwest::Client;
use std::fs::File;
use std::io::Write;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tauri::{Emitter, State};


use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

fn sanitize_relative_path(input: &str) -> Result<PathBuf, String> {
    let raw = input.replace('\\', "/");
    let mut out = PathBuf::new();

    for comp in std::path::Path::new(&raw).components() {
        match comp {
            Component::Normal(segment) => out.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Invalid relative path: {}", input));
            }
        }
    }

    if out.as_os_str().is_empty() {
        return Err("Path cannot be empty".to_string());
    }

    Ok(out)
}

fn sanitize_repo_id(repo_id: &str) -> Result<PathBuf, String> {
    let normalized = repo_id.trim().replace('\\', "/").trim_matches('/').to_string();
    if normalized.is_empty() {
        return Err("Repository ID cannot be empty".to_string());
    }

    sanitize_relative_path(&normalized)
}

#[derive(Default)]
pub struct DownloadState {
    pub cancellations: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

#[tauri::command]
pub async fn cancel_download(
    model_id: String,
    state: State<'_, DownloadState>,
) -> Result<(), String> {
    let map = state.cancellations.lock().await;
    if let Some(flag) = map.get(&model_id) {
        log::info!("Cancelling download for model {}", model_id);
        flag.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub async fn uninstall_model(
    model_id: String,
    state: State<'_, ProcessManagerState>,
    _download_state: State<'_, DownloadState>,
) -> Result<(), String> {
    let def = state
        .0
        .get_model_def(&model_id)
        .await
        .ok_or_else(|| format!("Model not found: {}", model_id))?;
        
    let models_dir = crate::paths::resolve_models_dir();
    let model_path = models_dir.join(&def.model_file);
    let name_scope_dir = {
        let mut comps = std::path::Path::new(&def.model_file).components();
        match (comps.next(), comps.next()) {
            (Some(Component::Normal(category)), Some(Component::Normal(name))) => {
                Some(models_dir.join(category).join(name))
            }
            _ => None,
        }
    };
    
    let is_custom = custom::is_custom_model(&models_dir, &model_id).unwrap_or(false);
    let is_disk_scanned = def
        .params
        .get("discovery_source")
        .map(|v| v == "disk_scan")
        .unwrap_or(false);

    // Attempt to stop the model before deleting
    let _ = state.0.stop_model(&model_id).await;

    // Delete the model at <category>/<name>/... by removing the <name> folder.
    // This preserves the category folder itself.
    let mut removed_name_scope = false;
    if let Some(name_dir) = &name_scope_dir {
        if name_dir.exists() {
            let _ = std::fs::remove_dir_all(name_dir);
            removed_name_scope = true;
        }
    }

    // Fallback for models that do not follow <category>/<name>/... layout.
    if !removed_name_scope && model_path.exists() {
        if model_path.is_dir() {
            let _ = std::fs::remove_dir_all(&model_path);
        } else {
            let _ = std::fs::remove_file(&model_path);
        }
    }
    
    // Delete any additional files specified in params (e.g. mmproj_file)
    for (key, val) in &def.params {
        if key.ends_with("_file") {
            let p = models_dir.join(val);
            if p.exists() {
                if p.is_dir() {
                    let _ = std::fs::remove_dir_all(&p);
                } else {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
    }
    
    if is_custom || is_disk_scanned {
        let _ = state.0.unregister_model(&model_id).await;
        if is_custom {
            let _ = custom::remove_custom_model(&models_dir, &model_id);
        }
    }

    Ok(())
}


#[derive(Clone, serde::Serialize)]
pub struct DownloadProgress {
    pub model_id: String,
    pub progress: f64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_bps: Option<f64>,
}

const MAX_DOWNLOAD_RETRIES: usize = 4;
const RETRY_BASE_DELAY_SECS: u64 = 2;
const CHUNK_TIMEOUT_SECS: u64 = 30;

enum DownloadStreamError {
    Cancelled,
    Retriable(String),
    Fatal(String),
}

fn emit_download_progress(
    app_handle: &tauri::AppHandle,
    model_id: &str,
    progress: f64,
    status: impl Into<String>,
) {
    emit_download_progress_with_speed(app_handle, model_id, progress, status, None);
}

fn emit_download_progress_with_speed(
    app_handle: &tauri::AppHandle,
    model_id: &str,
    progress: f64,
    status: impl Into<String>,
    speed_bps: Option<f64>,
) {
    let _ = app_handle.emit(
        "model-download-progress",
        DownloadProgress {
            model_id: model_id.to_string(),
            progress,
            status: status.into(),
            speed_bps,
        },
    );
}

fn retry_delay_for_attempt(attempt: usize) -> u64 {
    let power = attempt.saturating_sub(1).min(4) as u32;
    RETRY_BASE_DELAY_SECS.saturating_mul(1_u64 << power)
}

fn should_retry_status(status: reqwest::StatusCode) -> bool {
    status.is_server_error()
        || status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
}

async fn sleep_with_cancel(
    cancel_flag: &Arc<AtomicBool>,
    delay_secs: u64,
) -> Result<(), DownloadStreamError> {
    let mut remaining_ms = delay_secs.saturating_mul(1000);
    while remaining_ms > 0 {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err(DownloadStreamError::Cancelled);
        }

        let step = remaining_ms.min(250);
        tokio::time::sleep(Duration::from_millis(step)).await;
        remaining_ms -= step;
    }
    Ok(())
}

async fn stream_response_to_file(
    response: reqwest::Response,
    target_path: &Path,
    model_id: &str,
    app_handle: &tauri::AppHandle,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), DownloadStreamError> {
    let total_size = response.content_length().unwrap_or(0);
    let mut file = File::create(target_path)
        .map_err(|e| DownloadStreamError::Fatal(format!("Failed to create output file: {e}")))?;
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    let mut last_emit = std::time::Instant::now();
    let mut speed_window_started = std::time::Instant::now();
    let mut speed_window_bytes: u64 = 0;

    loop {
        if cancel_flag.load(Ordering::SeqCst) {
            drop(file);
            let _ = std::fs::remove_file(target_path);
            return Err(DownloadStreamError::Cancelled);
        }

        let next = tokio::time::timeout(Duration::from_secs(CHUNK_TIMEOUT_SECS), stream.next())
            .await
            .map_err(|_| {
                DownloadStreamError::Retriable(format!(
                    "No download data received for {CHUNK_TIMEOUT_SECS}s"
                ))
            })?;

        let Some(chunk) = next else {
            break;
        };

        let chunk = chunk.map_err(|e| {
            DownloadStreamError::Retriable(format!("Network stream interrupted: {e}"))
        })?;

        file.write_all(&chunk)
            .map_err(|e| DownloadStreamError::Fatal(format!("Failed writing downloaded bytes: {e}")))?;
        let chunk_size = chunk.len() as u64;
        downloaded += chunk_size;
        speed_window_bytes += chunk_size;

        if last_emit.elapsed().as_millis() > 500 {
            let (pct, status) = if total_size > 0 {
                (
                    (downloaded as f64 / total_size as f64) * 100.0,
                    "Downloading...".to_string(),
                )
            } else {
                (
                    0.0,
                    format!("Downloading... ({} MB)", downloaded / 1_000_000),
                )
            };

            let elapsed_s = speed_window_started.elapsed().as_secs_f64().max(0.001);
            let speed_bps = speed_window_bytes as f64 / elapsed_s;
            emit_download_progress_with_speed(
                app_handle,
                model_id,
                pct,
                status,
                Some(speed_bps.max(0.0)),
            );
            last_emit = std::time::Instant::now();
            speed_window_started = std::time::Instant::now();
            speed_window_bytes = 0;
        }
    }

    file.sync_all()
        .map_err(|e| DownloadStreamError::Fatal(format!("Failed to sync downloaded file: {e}")))?;
    drop(file);

    if downloaded == 0 {
        let _ = std::fs::remove_file(target_path);
        return Err(DownloadStreamError::Retriable(
            "Download produced an empty file".to_string(),
        ));
    }

    if total_size > 0 && downloaded != total_size {
        let _ = std::fs::remove_file(target_path);
        return Err(DownloadStreamError::Retriable(format!(
            "Incomplete download: {downloaded}/{total_size} bytes"
        )));
    }

    Ok(())
}

async fn fetch_gdrive_download_response(
    client: &Client,
    base_url: &str,
) -> Result<reqwest::Response, String> {
    let mut response = client.get(base_url).send().await.map_err(|e| e.to_string())?;

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    if content_type.starts_with("text/html") {
        let text = response.text().await.map_err(|e| e.to_string())?;

        let mut form_action = None;
        if let Some(action_idx) = text.find(r#"action=""#) {
            let start = action_idx + 8;
            if let Some(end) = text[start..].find('"') {
                form_action = Some(text[start..start + end].to_string());
            }
        }

        if let Some(action) = form_action {
            let mut query_params = vec![];
            let mut search_idx = 0;
            while let Some(input_idx) = text[search_idx..].find("<input type=\"hidden\"") {
                let offset = search_idx + input_idx;
                let bracket_end = text[offset..].find('>').unwrap_or(0);
                let input_tag = &text[offset..offset + bracket_end];

                if let Some(name_idx) = input_tag.find("name=\"") {
                    let name_start = name_idx + 6;
                    let name_len = input_tag[name_start..].find('"').unwrap_or(0);
                    let name = &input_tag[name_start..name_start + name_len];

                    if let Some(val_idx) = input_tag.find("value=\"") {
                        let val_start = val_idx + 7;
                        let val_len = input_tag[val_start..].find('"').unwrap_or(0);
                        let val = &input_tag[val_start..val_start + val_len];
                        query_params.push(format!("{}={}", name, val));
                    }
                }
                search_idx = offset + bracket_end + 1;
            }

            let mut final_action = action.clone();
            if final_action.starts_with('/') {
                final_action = format!("https://drive.google.com{}", final_action);
            }
            let sep = if final_action.contains('?') { "&" } else { "?" };
            let confirm_url = format!("{}{}{}", final_action, sep, query_params.join("&"));
            response = client
                .get(&confirm_url)
                .send()
                .await
                .map_err(|e| e.to_string())?;
        } else if let Some(idx) = text.find("confirm=") {
            let start = idx + 8;
            let end = text[start..]
                .find('&')
                .unwrap_or_else(|| text[start..].find('"').unwrap_or(text.len() - start))
                + start;
            let token = &text[start..end];
            let confirm_url = format!("{}&confirm={}", base_url, token);
            response = client
                .get(&confirm_url)
                .send()
                .await
                .map_err(|e| e.to_string())?;
        } else {
            let confirm_url = format!("{}&confirm=t", base_url);
            response = client
                .get(&confirm_url)
                .send()
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(response)
}

fn normalize_advanced_category(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "embedding" | "embeddings" => Some("embedding"),
        "grader" | "grade" => Some("grader"),
        "classifier" | "classify" | "router" => Some("classifier"),
        _ => None,
    }
}

fn model_in_advanced_category(def: &crate::registry::types::ModelDef, category: &str) -> bool {
    match category {
        "embedding" => def.tasks.contains(&TaskType::Embed),
        "grader" => def.tasks.contains(&TaskType::Grade),
        "classifier" => def.tasks.contains(&TaskType::Classify),
        _ => false,
    }
}

async fn ensure_model_registered_from_catalog(
    state: &State<'_, ProcessManagerState>,
    model_id: &str,
) -> Result<(), String> {
    if state.0.get_model_def(model_id).await.is_some() {
        return Ok(());
    }

    let defs = crate::config::load_model_definitions()?;
    let def = defs
        .into_iter()
        .find(|d| d.id == model_id)
        .ok_or_else(|| format!("Model not found: {}", model_id))?;
    state.0.register_model(def).await
}

async fn download_model_internal(
    app_handle: tauri::AppHandle,
    model_id: &str,
    state: &State<'_, ProcessManagerState>,
    download_state: &State<'_, DownloadState>,
) -> Result<(), String> {
    let def = state
        .0
        .get_model_def(model_id)
        .await
        .ok_or_else(|| format!("Model not found: {}", model_id))?;
        
    let gdrive_id = def
        .gdrive_id
        .as_ref()
        .ok_or_else(|| format!("Model '{}' does not have a GDrive ID", model_id))?;

    let models_dir = crate::paths::resolve_models_dir();
    let model_path = models_dir.join(&def.model_file);

    if let Some(parent) = model_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut map = download_state.cancellations.lock().await;
        map.insert(model_id.to_string(), cancel_flag.clone());
    }

    let is_zip = def.is_zip;
    let download_target = if is_zip {
        // If it's a zip we might be unpacking either a file or a folder
        models_dir.join(format!("{}.zip", def.model_file.replace("/", "_")))
    } else {
        model_path.clone()
    };

    emit_download_progress(&app_handle, model_id, 0.0, "Starting download...");

    let client = Client::new();
    let base_url = format!("https://drive.google.com/uc?export=download&id={}", gdrive_id);
    let mut transfer_ok = false;

    for attempt in 1..=MAX_DOWNLOAD_RETRIES {
        if cancel_flag.load(Ordering::SeqCst) {
            let _ = std::fs::remove_file(&download_target);
            let mut map = download_state.cancellations.lock().await;
            map.remove(model_id);
            return Err("Download cancelled".to_string());
        }

        let response = match fetch_gdrive_download_response(&client, &base_url).await {
            Ok(response) => response,
            Err(err) => {
                if attempt < MAX_DOWNLOAD_RETRIES {
                    let retry_in = retry_delay_for_attempt(attempt);
                    emit_download_progress(
                        &app_handle,
                        model_id,
                        0.0,
                        format!(
                            "Paused (network issue). Retrying in {retry_in}s (attempt {}/{})...",
                            attempt + 1,
                            MAX_DOWNLOAD_RETRIES
                        ),
                    );
                    if let Err(DownloadStreamError::Cancelled) =
                        sleep_with_cancel(&cancel_flag, retry_in).await
                    {
                        let _ = std::fs::remove_file(&download_target);
                        let mut map = download_state.cancellations.lock().await;
                        map.remove(model_id);
                        return Err("Download cancelled".to_string());
                    }
                    continue;
                }
                return Err(format!(
                    "Download failed after {} attempts: {}",
                    MAX_DOWNLOAD_RETRIES, err
                ));
            }
        };

        let status = response.status();
        if !status.is_success() {
            if should_retry_status(status) && attempt < MAX_DOWNLOAD_RETRIES {
                let retry_in = retry_delay_for_attempt(attempt);
                emit_download_progress(
                    &app_handle,
                    model_id,
                    0.0,
                    format!(
                        "Paused (server error {status}). Retrying in {retry_in}s (attempt {}/{})...",
                        attempt + 1,
                        MAX_DOWNLOAD_RETRIES
                    ),
                );
                if let Err(DownloadStreamError::Cancelled) =
                    sleep_with_cancel(&cancel_flag, retry_in).await
                {
                    let _ = std::fs::remove_file(&download_target);
                    let mut map = download_state.cancellations.lock().await;
                    map.remove(model_id);
                    return Err("Download cancelled".to_string());
                }
                continue;
            }

            return Err(format!("Download failed with status: {status}"));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");

        if content_type.starts_with("text/html") {
            let err = "Download endpoint returned HTML instead of model payload".to_string();
            if attempt < MAX_DOWNLOAD_RETRIES {
                let retry_in = retry_delay_for_attempt(attempt);
                emit_download_progress(
                    &app_handle,
                    model_id,
                    0.0,
                    format!(
                        "Paused (invalid response). Retrying in {retry_in}s (attempt {}/{})...",
                        attempt + 1,
                        MAX_DOWNLOAD_RETRIES
                    ),
                );
                if let Err(DownloadStreamError::Cancelled) =
                    sleep_with_cancel(&cancel_flag, retry_in).await
                {
                    let _ = std::fs::remove_file(&download_target);
                    let mut map = download_state.cancellations.lock().await;
                    map.remove(model_id);
                    return Err("Download cancelled".to_string());
                }
                continue;
            }
            return Err(err);
        }

        match stream_response_to_file(response, &download_target, model_id, &app_handle, &cancel_flag)
            .await
        {
            Ok(()) => {
                transfer_ok = true;
                break;
            }
            Err(DownloadStreamError::Cancelled) => {
                let _ = std::fs::remove_file(&download_target);
                let mut map = download_state.cancellations.lock().await;
                map.remove(model_id);
                return Err("Download cancelled".to_string());
            }
            Err(DownloadStreamError::Fatal(err)) => return Err(err),
            Err(DownloadStreamError::Retriable(err)) => {
                if attempt < MAX_DOWNLOAD_RETRIES {
                    let retry_in = retry_delay_for_attempt(attempt);
                    emit_download_progress(
                        &app_handle,
                        model_id,
                        0.0,
                        format!(
                            "Paused (network issue: {err}). Retrying in {retry_in}s (attempt {}/{})...",
                            attempt + 1,
                            MAX_DOWNLOAD_RETRIES
                        ),
                    );
                    if let Err(DownloadStreamError::Cancelled) =
                        sleep_with_cancel(&cancel_flag, retry_in).await
                    {
                        let _ = std::fs::remove_file(&download_target);
                        let mut map = download_state.cancellations.lock().await;
                        map.remove(model_id);
                        return Err("Download cancelled".to_string());
                    }
                    continue;
                }
                return Err(format!(
                    "Download failed after {} attempts: {}",
                    MAX_DOWNLOAD_RETRIES, err
                ));
            }
        }
    }

    if !transfer_ok {
        return Err("Download failed before transfer could complete".to_string());
    }

    if is_zip {
        emit_download_progress(&app_handle, model_id, 100.0, "Extracting archive...");

        let zip_open = std::fs::File::open(&download_target).map_err(|e| e.to_string())?;
        match zip::ZipArchive::new(zip_open) {
            Ok(mut archive) => {
                // Extract safely while normalizing entry paths to avoid nested duplicate directories
                // (e.g. distilBert-query-router/onnx_model/distilBert-query-router/onnx_model/...)
                let extract_path = model_path.parent().unwrap_or(&models_dir).to_path_buf();
                std::fs::create_dir_all(&extract_path).map_err(|e| e.to_string())?;

                let extract_path_rel = extract_path
                    .strip_prefix(&models_dir)
                    .map(|p| p.to_path_buf())
                    .unwrap_or_default();
                let extract_leaf = extract_path.file_name().map(|n| n.to_os_string());

                for i in 0..archive.len() {
                    let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
                    let entry_name = entry
                        .enclosed_name()
                        .ok_or_else(|| format!("Invalid zip entry path at index {}", i))?
                        .to_path_buf();

                    let mut normalized_rel = entry_name.clone();

                    if !extract_path_rel.as_os_str().is_empty() {
                        if let Ok(stripped) = entry_name.strip_prefix(&extract_path_rel) {
                            normalized_rel = stripped.to_path_buf();
                        }
                    }

                    if normalized_rel == entry_name {
                        if let Some(ref leaf) = extract_leaf {
                            if let Ok(stripped) = entry_name.strip_prefix(std::path::Path::new(leaf)) {
                                normalized_rel = stripped.to_path_buf();
                            }
                        }
                    }

                    if normalized_rel.as_os_str().is_empty() {
                        continue;
                    }

                    let out_path = extract_path.join(&normalized_rel);

                    if entry.is_dir() {
                        std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
                        continue;
                    }

                    if let Some(parent) = out_path.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }

                    let mut out_file = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
                    std::io::copy(&mut entry, &mut out_file).map_err(|e| e.to_string())?;
                }

                // Delete zip
                let _ = std::fs::remove_file(&download_target);
            }
            Err(zip_err) => {
                // Some GDrive IDs point directly to a single file even when config says is_zip=true.
                // If model_file is a file path, keep the downloaded payload as the final model file.
                if model_path.extension().is_some() {
                    if let Some(parent) = model_path.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }

                    if model_path.exists() {
                        let _ = std::fs::remove_file(&model_path);
                    }

                    std::fs::rename(&download_target, &model_path).map_err(|e| e.to_string())?;
                } else {
                    let _ = std::fs::remove_file(&download_target);
                    return Err(format!(
                        "Downloaded archive is invalid for model '{}': {}",
                        model_id, zip_err
                    ));
                }
            }
        }
    }

    emit_download_progress(&app_handle, model_id, 100.0, "Complete");

    {
        let mut map = download_state.cancellations.lock().await;
        map.remove(model_id);
    }

    Ok(())
}

#[tauri::command]
pub async fn download_model(
    app_handle: tauri::AppHandle,
    model_id: String,
    state: State<'_, ProcessManagerState>,
    download_state: State<'_, DownloadState>,
) -> Result<(), String> {
    ensure_model_registered_from_catalog(&state, &model_id).await?;
    download_model_internal(
        app_handle,
        &model_id,
        &state,
        &download_state,
    ).await
}

#[tauri::command]
pub async fn download_model_category(
    app_handle: tauri::AppHandle,
    category: String,
    state: State<'_, ProcessManagerState>,
    download_state: State<'_, DownloadState>,
) -> Result<u32, String> {
    let category = normalize_advanced_category(&category)
        .ok_or_else(|| format!("Unknown category '{}'", category))?;

    let defs = crate::config::load_model_definitions()?;
    let mut defs_by_id: HashMap<String, crate::registry::types::ModelDef> = HashMap::new();
    let mut target_ids = Vec::new();

    for def in defs {
        if model_in_advanced_category(&def, category) && def.gdrive_id.is_some() {
            target_ids.push(def.id.clone());
            defs_by_id.insert(def.id.clone(), def);
        }
    }

    if target_ids.is_empty() {
        return Err(format!("No downloadable models configured for category '{}'", category));
    }

    let models_dir = crate::paths::resolve_models_dir();
    let mut downloaded = 0_u32;

    for model_id in target_ids {
        let mut def = state.0.get_model_def(&model_id).await;
        if def.is_none() {
            if let Some(config_def) = defs_by_id.get(&model_id).cloned() {
                state.0.register_model(config_def).await?;
                def = state.0.get_model_def(&model_id).await;
            }
        }

        let def = def.ok_or_else(|| format!("Model not found: {}", model_id))?;
        if def.files_exist(&models_dir) {
            continue;
        }

        download_model_internal(
            app_handle.clone(),
            &model_id,
            &state,
            &download_state,
        ).await?;
        downloaded += 1;
    }

    Ok(downloaded)
}

#[tauri::command]
pub async fn download_custom_file(
    app_handle: tauri::AppHandle,
    url: String,
    folder: String,
    filename: String,
    repo_id: Option<String>,
    relative_path: Option<String>,
    download_state: State<'_, DownloadState>,
) -> Result<(), String> {
    let models_dir = crate::paths::resolve_models_dir();
    let folder_rel = sanitize_relative_path(&folder)?;
    let file_rel = if let Some(ref rel) = relative_path {
        sanitize_relative_path(rel)?
    } else {
        sanitize_relative_path(&filename)?
    };

    let container_rel = if let Some(repo) = repo_id.as_ref().filter(|s| !s.trim().is_empty()) {
        folder_rel.join(sanitize_repo_id(repo)?)
    } else {
        folder_rel
    };

    let model_path = models_dir.join(&container_rel).join(&file_rel);

    if let Some(parent) = model_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let model_id = if let Some(repo) = repo_id.as_ref().filter(|s| !s.trim().is_empty()) {
        format!(
            "{}/{}/{}",
            folder,
            repo.trim_matches('/'),
            file_rel.to_string_lossy().replace('\\', "/")
        )
    } else {
        format!("{}/{}", folder, filename)
    };

    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut map = download_state.cancellations.lock().await;
        map.insert(model_id.clone(), cancel_flag.clone());
    }

    emit_download_progress(&app_handle, &model_id, 0.0, "Starting download...");

    let client = Client::new();
    let is_gdrive = url.contains("drive.google.com");
    let mut transfer_ok = false;

    for attempt in 1..=MAX_DOWNLOAD_RETRIES {
        if cancel_flag.load(Ordering::SeqCst) {
            let _ = std::fs::remove_file(&model_path);
            let mut map = download_state.cancellations.lock().await;
            map.remove(&model_id);
            return Err("Download cancelled".to_string());
        }

        let response = if is_gdrive {
            fetch_gdrive_download_response(&client, &url).await
        } else {
            client.get(&url).send().await.map_err(|e| e.to_string())
        };

        let response = match response {
            Ok(response) => response,
            Err(err) => {
                if attempt < MAX_DOWNLOAD_RETRIES {
                    let retry_in = retry_delay_for_attempt(attempt);
                    emit_download_progress(
                        &app_handle,
                        &model_id,
                        0.0,
                        format!(
                            "Paused (network issue). Retrying in {retry_in}s (attempt {}/{})...",
                            attempt + 1,
                            MAX_DOWNLOAD_RETRIES
                        ),
                    );
                    if let Err(DownloadStreamError::Cancelled) =
                        sleep_with_cancel(&cancel_flag, retry_in).await
                    {
                        let _ = std::fs::remove_file(&model_path);
                        let mut map = download_state.cancellations.lock().await;
                        map.remove(&model_id);
                        return Err("Download cancelled".to_string());
                    }
                    continue;
                }
                return Err(format!(
                    "Download failed after {} attempts: {}",
                    MAX_DOWNLOAD_RETRIES, err
                ));
            }
        };

        let status = response.status();
        if !status.is_success() {
            if should_retry_status(status) && attempt < MAX_DOWNLOAD_RETRIES {
                let retry_in = retry_delay_for_attempt(attempt);
                emit_download_progress(
                    &app_handle,
                    &model_id,
                    0.0,
                    format!(
                        "Paused (server error {status}). Retrying in {retry_in}s (attempt {}/{})...",
                        attempt + 1,
                        MAX_DOWNLOAD_RETRIES
                    ),
                );
                if let Err(DownloadStreamError::Cancelled) =
                    sleep_with_cancel(&cancel_flag, retry_in).await
                {
                    let _ = std::fs::remove_file(&model_path);
                    let mut map = download_state.cancellations.lock().await;
                    map.remove(&model_id);
                    return Err("Download cancelled".to_string());
                }
                continue;
            }

            return Err(format!("Download failed with status: {status}"));
        }

        if is_gdrive {
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok())
                .unwrap_or("");
            if content_type.starts_with("text/html") {
                let err = "Download endpoint returned HTML instead of file payload".to_string();
                if attempt < MAX_DOWNLOAD_RETRIES {
                    let retry_in = retry_delay_for_attempt(attempt);
                    emit_download_progress(
                        &app_handle,
                        &model_id,
                        0.0,
                        format!(
                            "Paused (invalid response). Retrying in {retry_in}s (attempt {}/{})...",
                            attempt + 1,
                            MAX_DOWNLOAD_RETRIES
                        ),
                    );
                    if let Err(DownloadStreamError::Cancelled) =
                        sleep_with_cancel(&cancel_flag, retry_in).await
                    {
                        let _ = std::fs::remove_file(&model_path);
                        let mut map = download_state.cancellations.lock().await;
                        map.remove(&model_id);
                        return Err("Download cancelled".to_string());
                    }
                    continue;
                }
                return Err(err);
            }
        }

        match stream_response_to_file(response, &model_path, &model_id, &app_handle, &cancel_flag).await {
            Ok(()) => {
                transfer_ok = true;
                break;
            }
            Err(DownloadStreamError::Cancelled) => {
                let _ = std::fs::remove_file(&model_path);
                let mut map = download_state.cancellations.lock().await;
                map.remove(&model_id);
                return Err("Download cancelled".to_string());
            }
            Err(DownloadStreamError::Fatal(err)) => return Err(err),
            Err(DownloadStreamError::Retriable(err)) => {
                if attempt < MAX_DOWNLOAD_RETRIES {
                    let retry_in = retry_delay_for_attempt(attempt);
                    emit_download_progress(
                        &app_handle,
                        &model_id,
                        0.0,
                        format!(
                            "Paused (network issue: {err}). Retrying in {retry_in}s (attempt {}/{})...",
                            attempt + 1,
                            MAX_DOWNLOAD_RETRIES
                        ),
                    );
                    if let Err(DownloadStreamError::Cancelled) =
                        sleep_with_cancel(&cancel_flag, retry_in).await
                    {
                        let _ = std::fs::remove_file(&model_path);
                        let mut map = download_state.cancellations.lock().await;
                        map.remove(&model_id);
                        return Err("Download cancelled".to_string());
                    }
                    continue;
                }
                return Err(format!(
                    "Download failed after {} attempts: {}",
                    MAX_DOWNLOAD_RETRIES, err
                ));
            }
        }
    }

    if !transfer_ok {
        return Err("Download failed before transfer could complete".to_string());
    }

    emit_download_progress(&app_handle, &model_id, 100.0, "Complete");

    {
        let mut map = download_state.cancellations.lock().await;
        map.remove(&model_id);
    }

    Ok(())
}

#[tauri::command]
pub fn check_custom_file_exists(
    folder: String,
    filename: String,
    repo_id: Option<String>,
    relative_path: Option<String>,
) -> bool {
    let models_dir = crate::paths::resolve_models_dir();
    let folder_rel = match sanitize_relative_path(&folder) {
        Ok(path) => path,
        Err(_) => return false,
    };

    let file_rel = match relative_path
        .as_ref()
        .map(|value| sanitize_relative_path(value))
        .unwrap_or_else(|| sanitize_relative_path(&filename))
    {
        Ok(path) => path,
        Err(_) => return false,
    };

    let container_rel = match repo_id.as_ref().filter(|s| !s.trim().is_empty()) {
        Some(repo) => match sanitize_repo_id(repo) {
            Ok(repo_rel) => folder_rel.join(repo_rel),
            Err(_) => return false,
        },
        None => folder_rel,
    };

    let model_path = models_dir.join(container_rel).join(file_rel);
    model_path.exists()
}

