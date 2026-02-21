#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! GenHat Desktop — main entry point.
//!
//! Slim bootstrap: loads config, initializes the process control module,
//! registers Tauri commands, and handles app lifecycle.

use app_lib::commands::inference::TaskRouterState;
use app_lib::commands::models::ProcessManagerState;
use app_lib::commands::rag::RagPipelineState;
use app_lib::process::ProcessManager;
use app_lib::rag::pipeline::RagPipeline;
use app_lib::registry::ModelRegistry;
use app_lib::router::TaskRouter;
use std::sync::Arc;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 1. Load model registry from embedded models.toml
            let registry = Arc::new(
                ModelRegistry::load().expect("Failed to load model registry"),
            );

            // 2. Resolve models directory
            let models_dir = app_lib::commands::models::get_models_dir();
            log::info!("Models directory: {}", models_dir.display());

            // 3. Initialize the process manager
            let process_manager = Arc::new(ProcessManager::new(&registry, models_dir));

            // 4. Initialize the task router
            let router = Arc::new(TaskRouter::new(
                registry.clone(),
                process_manager.clone(),
            ));

            // 5. Start the lifecycle manager (background health checks + reaping)
            let pm_clone = process_manager.clone();
            app_lib::process::lifecycle::start_lifecycle_thread(pm_clone, 30);

            // 6. Auto-start models marked with auto_start = true
            let auto_models: Vec<String> = registry
                .auto_start_models()
                .iter()
                .map(|m| m.id.clone())
                .collect();

            if !auto_models.is_empty() {
                let pm_clone = process_manager.clone();
                tauri::async_runtime::spawn(async move {
                    for model_id in auto_models {
                        log::info!("Auto-starting model: {model_id}");
                        match pm_clone.ensure_running(&model_id, false).await {
                            Ok(id) => log::info!("Auto-started {model_id} (instance: {id})"),
                            Err(e) => log::warn!("Failed to auto-start {model_id}: {e}"),
                        }
                    }
                });
            }

            // 7. Initialize RAG pipeline
            let rag_data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from(".genhat_data"));
            let rag_dir = rag_data_dir.join("rag");
            let rag_pipeline = Arc::new(
                RagPipeline::open(&rag_dir, router.clone())
                    .expect("Failed to initialize RAG pipeline"),
            );

            // Start background enrichment worker (with app handle for event emission)
            RagPipeline::start_enrichment_worker(rag_pipeline.clone(), app.handle().clone());

            // 8. Register state for Tauri commands
            app.manage(ProcessManagerState(process_manager));
            app.manage(TaskRouterState(router));
            app.manage(RagPipelineState(rag_pipeline));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Legacy-compatible commands
            app_lib::commands::models::list_models,
            app_lib::commands::models::list_vision_models,
            app_lib::commands::models::list_audio_models,
            app_lib::commands::models::switch_model,
            app_lib::commands::models::stop_llama,
            app_lib::commands::audio::generate_speech,
            // New unified commands
            app_lib::commands::models::list_registered_models,
            app_lib::commands::models::get_model_status,
            app_lib::commands::models::start_model,
            app_lib::commands::models::stop_model,
            app_lib::commands::models::get_llama_port,
            app_lib::commands::models::get_memory_usage,
            app_lib::commands::models::read_image_base64,
            app_lib::commands::inference::route_request,
            app_lib::commands::inference::vision_chat,
            app_lib::commands::inference::vision_chat_stream,
            app_lib::commands::audio::transcribe_audio,
            // RAG commands
            app_lib::commands::rag::ingest_document,
            app_lib::commands::rag::ingest_folder,
            app_lib::commands::rag::query_rag,
            app_lib::commands::rag::list_rag_documents,
            app_lib::commands::rag::delete_rag_document,
            app_lib::commands::rag::enrich_rag_documents,
            // RAPTOR commands
            app_lib::commands::rag::build_raptor_tree,
            app_lib::commands::rag::has_raptor_tree,
            app_lib::commands::rag::delete_raptor_tree,
            app_lib::commands::rag::query_rag_with_raptor,
        ])
        .build(tauri::generate_context!())
        .expect("error building tauri app")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                log::info!("App exiting — stopping all models...");
                let pm = app_handle.state::<ProcessManagerState>();
                let pm = pm.0.clone();
                // Block on stopping all processes before exit
                tauri::async_runtime::block_on(async {
                    pm.stop_all().await;
                });
            }
        });
}