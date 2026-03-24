use crate::process::ProcessManager;
use crate::commands::models::ProcessManagerState;
use futures_util::StreamExt;
use reqwest::Client;
use std::fs::File;
use std::io::Write;
use tauri::{Emitter, State};


use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

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
    download_state: State<'_, DownloadState>,
) -> Result<(), String> {
    let def = state
        .0
        .get_model_def(&model_id)
        .await
        .ok_or_else(|| format!("Model not found: {}", model_id))?;
        
    let models_dir = crate::paths::resolve_models_dir();
    let model_path = models_dir.join(&def.model_file);
    
    // Attempt to stop the model before deleting
    let _ = state.0.stop_model(&model_id).await;

    // Delete the primary model file or directory
    if model_path.exists() {
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
    
    Ok(())
}


#[derive(Clone, serde::Serialize)]
pub struct DownloadProgress {
    pub model_id: String,
    pub progress: f64,
    pub status: String,
}

#[tauri::command]
pub async fn download_model(
    app_handle: tauri::AppHandle,
    model_id: String,
    state: State<'_, ProcessManagerState>,
    download_state: State<'_, DownloadState>,
) -> Result<(), String> {
    let def = state
        .0
        .get_model_def(&model_id)
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
        map.insert(model_id.clone(), cancel_flag.clone());
    }


    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut map = download_state.cancellations.lock().await;
        map.insert(model_id.clone(), cancel_flag.clone());
    }

    let is_zip = def.is_zip;
    let download_target = if is_zip {
        // If it's a zip we might be unpacking either a file or a folder
        models_dir.join(format!("{}.zip", def.model_file.replace("/", "_")))
    } else {
        model_path.clone()
    };

    let _ = app_handle.emit(
        "model-download-progress",
        DownloadProgress {
            model_id: model_id.clone(),
            progress: 0.0,
            status: "Starting download...".to_string(),
        },
    );

    let client = Client::new();
        
    let base_url = format!("https://drive.google.com/uc?export=download&id={}", gdrive_id);
    let mut response = client.get(&base_url).send().await.map_err(|e| e.to_string())?;

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
            // some forms already have queries in action
            let sep = if final_action.contains('?') { "&" } else { "?" };
            let confirm_url = format!("{}{}{}", final_action, sep, query_params.join("&"));
            response = client.get(&confirm_url).send().await.map_err(|e| e.to_string())?;
        } else if let Some(idx) = text.find("confirm=") {
            let start = idx + 8;
            let end = text[start..]
                .find('&')
                .unwrap_or_else(|| text[start..].find('"').unwrap_or(text.len() - start))
                + start;
            let token = &text[start..end];
            let confirm_url = format!("{}&confirm={}", base_url, token);
            response = client.get(&confirm_url).send().await.map_err(|e| e.to_string())?;
        } else {
            let confirm_url = format!("{}&confirm=t", base_url);
            response = client.get(&confirm_url).send().await.map_err(|e| e.to_string())?;
        }
    }

    if !response.status().is_success() {
        return Err(format!("Download failed with status: {}", response.status()));
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut file = File::create(&download_target).map_err(|e| e.to_string())?;
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    let mut last_emit = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        if cancel_flag.load(Ordering::SeqCst) {
            drop(file);
            let _ = std::fs::remove_file(&download_target);
            let mut map = download_state.cancellations.lock().await;
            map.remove(&model_id);
            return Err("Download cancelled".to_string());
        }
        
        if cancel_flag.load(Ordering::SeqCst) {
            drop(file);
            let _ = std::fs::remove_file(&download_target);
            let mut map = download_state.cancellations.lock().await;
            map.remove(&model_id);
            return Err("Download cancelled".to_string());
        }
        
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;

        if last_emit.elapsed().as_millis() > 500 {
            let mut pct = 0.0;
            let mut status = "Downloading...".to_string();
            if total_size > 0 {
                pct = (downloaded as f64 / total_size as f64) * 100.0;
            } else {
                status = format!("Downloading... ({} MB)", downloaded / 1_000_000);
            }
            let _ = app_handle.emit(
                "model-download-progress",
                DownloadProgress {
                    model_id: model_id.clone(),
                    progress: pct,
                    status,
                },
            );
            last_emit = std::time::Instant::now();
        }
    }
    
    // Explicit sync to disk
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);

    if is_zip {
        let _ = app_handle.emit(
            "model-download-progress",
            DownloadProgress {
                model_id: model_id.clone(),
                progress: 100.0,
                status: "Extracting archive...".to_string(),
            },
        );

        let zip_file = std::fs::File::open(&download_target).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| e.to_string())?;
        
        // Ensure extraction target directory exists
        // E.g., if model_file = "LiquidAI-VLM/mmproj...gguf", extract to "LiquidAI-VLM" or models directory
        let extract_path = model_path.parent().unwrap_or(&models_dir);
        
        archive.extract(extract_path).map_err(|e| e.to_string())?;
        
        // Delete zip
        let _ = std::fs::remove_file(&download_target);
    }

    let _ = app_handle.emit(
        "model-download-progress",
        DownloadProgress {
            model_id: model_id.clone(),
            progress: 100.0,
            status: "Complete".to_string(),
        },
    );

    Ok(())
}
