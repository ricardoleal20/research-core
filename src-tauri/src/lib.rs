mod agent;
mod chat_commands;
mod checkpoints_commands;
mod commands;
mod dashboard_commands;
mod db;
mod evidence_commands;
mod export_commands;
mod hypotheses_commands;
mod jobs_commands;
mod library_commands;
mod mcp;
mod manuscript_commands;
mod missions_commands;
mod nightshift_commands;
mod onboarding_commands;
mod proposals_commands;
mod readiness_commands;
mod trust_commands;
mod verifier_commands;
mod nightshift;
mod runtime;
mod search_commands;
mod server;
mod trust;

pub mod adapters;
pub mod domain;
pub mod eventstore;

use db::Db;
use mcp::McpRegistry;
use tauri::Manager;

/// Resolved on-disk locations, managed as Tauri state.
pub struct AppPaths {
    pub data_dir: std::path::PathBuf,
    pub log_dir: std::path::PathBuf,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Resolve data dir: ~/Library/Application Support/Research Core/db.sqlite
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&data_dir).ok();
            // Logs live in a hidden subdir alongside the DB.
            let log_dir = data_dir.join("logs");
            std::fs::create_dir_all(&log_dir).ok();
            app.manage(AppPaths { data_dir: data_dir.clone(), log_dir });
            let db_path = data_dir.join("research-core.sqlite");
            let db = Db::open(&db_path).expect("failed to open database");
            app.manage(db.clone());
            app.manage(McpRegistry::new());
            // In-process server shell (AD-7): the same app in the browser —
            // same core instance, one writer (AD-14), read-only API.
            server::spawn(db.clone(), data_dir.clone());
            // Night Shift scheduler (FR-4.1): one tick per minute — due
            // missions run their nightly literature scan, dead runs are
            // reaped honestly, and terminators evaluate (AD-12).
            nightshift::spawn(db);

            // native macOS menu
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::{Menu, MenuItem, Submenu};
                let app_menu = Submenu::with_items(
                    app,
                    "Research Core",
                    true,
                    &[&MenuItem::with_id(app, "about", "About Research Core", true, None::<&str>)?],
                )?;
                let file_menu = Submenu::with_items(
                    app,
                    "File",
                    true,
                    &[
                        &MenuItem::with_id(app, "new_project", "New Project", true, Some("CmdOrCtrl+N"))?,
                        &MenuItem::with_id(app, "new_ref", "Add Reference", true, Some("CmdOrCtrl+R"))?,
                    ],
                )?;
                let edit_menu = Submenu::with_items(
                    app,
                    "Edit",
                    true,
                    &[
                        &MenuItem::with_id(app, "undo", "Undo", true, Some("CmdOrCtrl+Z"))?,
                        &MenuItem::with_id(app, "redo", "Redo", true, Some("CmdOrCtrl+Shift+Z"))?,
                    ],
                )?;
                let window_menu = Submenu::with_items(
                    app,
                    "Window",
                    true,
                    &[
                        &MenuItem::with_id(app, "minimize", "Minimize", true, Some("CmdOrCtrl+M"))?,
                    ],
                )?;
                let debug_menu = Submenu::with_items(
                    app,
                    "Debug",
                    true,
                    &[
                        &MenuItem::with_id(app, "toggle_devtools", "Toggle Developer Tools", true, Some("CmdOrCtrl+Alt+I"))?,
                        &MenuItem::with_id(app, "reload", "Reload", true, Some("CmdOrCtrl+R"))?,
                    ],
                )?;
                let menu = Menu::with_items(app, &[&app_menu, &file_menu, &edit_menu, &window_menu, &debug_menu])?;
                app.set_menu(menu)?;
            }
            // Wire debug menu actions.
            app.on_menu_event(move |app, event| match event.id().as_ref() {
                "toggle_devtools" => {
                    if let Some(win) = app.get_webview_window("main") {
                        if win.is_devtools_open() { win.close_devtools(); } else { win.open_devtools(); }
                    }
                }
                "reload" => {
                    if let Some(win) = app.get_webview_window("main") { let _ = win.eval("location.reload()"); }
                }
                _ => {}
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::get_active_project,
            commands::set_active_project,
            commands::create_project,
            commands::update_project,
            commands::delete_project,
            commands::get_dashboard,
            commands::list_refs,
            commands::get_ref,
            commands::create_ref,
            commands::update_ref,
            commands::delete_ref,
            commands::search_refs,
            commands::search_refs_external,
            commands::list_collections,
            commands::list_reviews,
            commands::run_review,
            commands::list_actions,
            commands::create_action,
            commands::toggle_action,
            commands::update_action,
            commands::delete_action,
            chat_commands::list_chats,
            chat_commands::create_chat,
            chat_commands::get_chat,
            chat_commands::send_message,
            chat_commands::delete_chat,
            chat_commands::set_chat_scope,
            chat_commands::set_chat_skill,
            chat_commands::set_chat_model,
            chat_commands::list_skills,
            chat_commands::add_skill,
            chat_commands::pick_attachment_files,
            chat_commands::add_chat_attachments,
            chat_commands::remove_chat_attachment,
            commands::list_agents,
            commands::toggle_agent,
            commands::list_mcp_servers,
            commands::add_mcp_server,
            commands::update_mcp_server,
            commands::delete_mcp_server,
            commands::test_mcp_server,
            commands::list_mcp_tools,
            commands::get_settings,
            commands::update_setting,
            commands::set_provider_key,
            commands::reset_database,
            commands::app_log,
            commands::get_app_paths,
            commands::reveal_path,
            commands::pick_folder,
            commands::verify_key,
            commands::set_lock_key,
            commands::lock_state,
            commands::test_cli,
            commands::get_ai_config,
            commands::configure_ai_provider,
            commands::use_cli_bridge,
            commands::use_local_provider,
            commands::test_local_provider,
            commands::test_provider_connection,
            commands::list_provider_models,
            missions_commands::create_mission,
            missions_commands::list_missions,
            missions_commands::get_mission_runs,
            missions_commands::get_run_receipt,
            missions_commands::run_agent_step,
            jobs_commands::declare_compute_target,
            jobs_commands::list_compute_targets,
            jobs_commands::list_registered_adapters,
            jobs_commands::probe_compute_target,
            jobs_commands::get_host_allowlist,
            jobs_commands::set_host_allowlist,
            jobs_commands::submit_job,
            jobs_commands::poll_jobs,
            jobs_commands::fetch_job,
            jobs_commands::fetch_job_results,
            nightshift_commands::get_morning_digest,
            nightshift_commands::run_night_shift_now,
            nightshift_commands::set_mission_schedule,
            hypotheses_commands::create_hypothesis,
            hypotheses_commands::list_hypotheses,
            hypotheses_commands::transition_hypothesis,
            hypotheses_commands::add_relation,
            evidence_commands::register_claim,
            evidence_commands::pin_claim_to_citation,
            evidence_commands::pin_claim_to_numerical,
            evidence_commands::list_evidence,
            verifier_commands::run_pin_verification,
            export_commands::export_workspace,
            export_commands::inspect_export,
            proposals_commands::list_proposals,
            proposals_commands::approve_proposal,
            proposals_commands::reject_proposal,
            trust_commands::get_trust_status,
            trust_commands::configure_autonomy,
            trust_commands::configure_ceiling,
            trust_commands::kill_runtime,
            trust_commands::resume_runtime,
            checkpoints_commands::create_checkpoint,
            checkpoints_commands::list_checkpoints,
            checkpoints_commands::preview_rollback,
            checkpoints_commands::rollback_to_checkpoint,
            search_commands::run_search,
            search_commands::get_search_disclosure,
            readiness_commands::get_readiness_report,
            // dashboard (Story 5.10, FR-18): the home panel's one
            // read-only aggregated fold — a composition over the read
            // models above, never a write.
            dashboard_commands::dashboard_summary,
            onboarding_commands::run_first_value,
            onboarding_commands::run_first_value_from_ref,
            // library (evented references CRUD, FR-15, Epic 5): the adds
            // (arXiv paste / manual / Zotero import) and the auditable
            // remove/restore pair — mutations are evented in domain/library
            // (AD-16); the legacy create_ref/delete_ref paths stay dead.
            library_commands::add_ref_from_arxiv,
            library_commands::add_ref_manual,
            library_commands::import_refs_from_zotero,
            library_commands::remove_ref,
            library_commands::restore_ref,
            // manuscript (Story 6.6, FR-20.1/20.2): the .tex repo IS the
            // manuscript — registration + scan + compile + the desktop-only
            // editing surface; the served browser view reads it read-only.
            manuscript_commands::register_manuscript,
            manuscript_commands::get_manuscript,
            manuscript_commands::list_manuscripts,
            manuscript_commands::read_manuscript_file,
            manuscript_commands::write_manuscript_file,
            manuscript_commands::compile_manuscript,
            // quarantined LaTeX diffs (Story 6.7, FR-20.3): the agent seam
            // proposes, the human merges/rejects — the basis is validated
            // at merge time (AD-13), the merge checkpoints before applying.
            manuscript_commands::list_manuscript_diffs,
            manuscript_commands::propose_manuscript_diff,
            manuscript_commands::approve_manuscript_diff,
            manuscript_commands::reject_manuscript_diff,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
