mod agent;
mod agent_pure;
mod anonymous_analytics;
mod auth;
mod coach_chat;
pub mod context;
mod entitlements;
mod insights_local;
mod jira;
mod linear;
mod llama_port;
mod oauth_env;
pub mod paths;
pub mod secure_store;
mod sync;
mod sync_env;
mod sync_pure;
mod user_preferences;
mod vision_model;

use tauri::Manager;

use agent::{
    check_local_server, get_activity_log, get_config, get_status, get_today_history,
    get_week_summary, initialize_agent, llama_managed_process_status, llama_server_log_tail,
    restart_llama_server_cpu_only, save_activity, start_monitoring, stop_monitoring, update_config,
    AgentState,
};

pub fn run() {
    let app = tauri::Builder::default()
        .manage(AgentState::default())
        .invoke_handler(tauri::generate_handler![
            initialize_agent,
            get_config,
            update_config,
            get_status,
            start_monitoring,
            stop_monitoring,
            save_activity,
            get_activity_log,
            check_local_server,
            agent::capture_context_snapshot,
            jira::fetch_jira_tasks,
            jira::fetch_jira_profile,
            sync::force_sync_now,
            sync::clear_user_session,
            sync::get_current_user,
            sync::join_team,
            sync::get_user_teams,
            sync::set_active_team,
            entitlements::get_entitlements,
            entitlements::claim_license_code,
            entitlements::ensure_personal_team,
            entitlements::refresh_entitlements,
            entitlements::fetch_cloud_insights,
            entitlements::request_cloud_insights,
            coach_chat::get_coach_chat_messages,
            coach_chat::clear_coach_chat,
            coach_chat::get_coach_chat_usage,
            coach_chat::send_coach_chat_message,
            insights_local::generate_local_status_report,
            user_preferences::get_user_preferences,
            user_preferences::save_user_preferences_command,
            anonymous_analytics::get_analytics_consent,
            anonymous_analytics::set_analytics_consent,
            anonymous_analytics::sync_anonymous_analytics,
            anonymous_analytics::submit_product_feedback,
            agent::start_server,
            agent::stop_server,
            llama_managed_process_status,
            llama_server_log_tail,
            restart_llama_server_cpu_only,
            // Auth commands
            auth::start_auth,
            auth::cancel_auth,
            auth::get_auth_session,
            auth::logout,
            auth::is_logged_in,
            auth::login_with_password,
            // Linear commands
            linear::fetch_linear_tasks,
            linear::fetch_linear_profile,
            // History commands
            get_today_history,
            get_week_summary,
            paths::get_flowmates_user_paths,
            paths::save_pdf_to_downloads,
            paths::open_path_in_file_manager,
        ])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_theme(Some(tauri::Theme::Light));
            }

            app.handle().plugin(tauri_plugin_process::init())?;
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;

            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .targets([
                        tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                        tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                            file_name: None,
                        }),
                    ])
                    .build(),
            )?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app_handle, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            agent::shutdown_managed_server();
        }
    });
}

/// One-line description of the backend configuration, for startup logging.
///
/// Never echoes the key: only whether one is present. A log line that leaks a
/// credential is worse than no log line at all.
pub fn backend_status() -> String {
    let url = sync_env::supabase_url();
    let has_key = !sync_env::supabase_anon_key().is_empty();
    match (url.is_empty(), has_key) {
        (true, _) => "none configured — local measurement only".to_string(),
        (false, false) => format!("{url} (NO KEY — cloud stays unavailable)"),
        (false, true) => format!("{url} (key present)"),
    }
}
