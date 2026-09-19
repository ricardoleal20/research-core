mod agent;
mod commands;
mod db;
mod mcp;
mod missions_commands;

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
            app.manage(db);
            app.manage(McpRegistry::new());

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
            commands::list_chats,
            commands::create_chat,
            commands::get_chat,
            commands::send_message,
            commands::delete_chat,
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
            commands::reset_database,
            commands::app_log,
            commands::get_app_paths,
            commands::reveal_path,
            commands::pick_folder,
            commands::verify_key,
            commands::set_lock_key,
            commands::lock_state,
            commands::test_cli,
            missions_commands::create_mission,
            missions_commands::list_missions,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
