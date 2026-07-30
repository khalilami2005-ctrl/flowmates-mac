use crate::sync_env::{supabase_anon_key, supabase_url};
use crate::sync_pure::{clamp_line_for_summary, jwt_exp, truncate_tasks_for_summary};
use crate::vision_model::LLAMA_CHAT_MODEL_ID;
use base64::Engine;
use reqwest::blocking::{Client, Response};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

const SYNC_INTERVAL_MINS: u64 = 10;
/// Max rows per cloud upload batch (oldest unsynced first). Override with `FLOWMATES_SYNC_BATCH_LIMIT`.
const CLOUDSYNC_BATCH_LIMIT_DEFAULT: u64 = 500;
/// Refresh the access token when it is expired or within this many seconds of expiring.
const JWT_REFRESH_MARGIN_SECS: i64 = 300;
/// Background poll interval for proactive JWT renewal.
const TOKEN_REFRESH_POLL_SECS: u64 = 120;
/// Max Unicode characters of TASKS text sent to the local `/v1/chat/completions` endpoint.
/// Default llama.cpp servers often use `n_ctx=2048`; prompt = instructions + tasks must stay under that.
/// Override with env `FLOWMATES_SUMMARY_MAX_CHARS` (same unit: Unicode chars).
const SUMMARY_MAX_TASK_CHARS: usize = 5000;
/// Avoid one verbose vision capture consuming the whole summary budget (`FLOWMATES_SUMMARY_MAX_LINE_CHARS` to override).
const SUMMARY_MAX_LINE_CHARS_DEFAULT: usize = 450;
static SYNC_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) fn get_or_create_device_id(conn: &Connection) -> Result<String, String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'device_id'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok();
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value) VALUES ('device_id', ?1)",
        params![id],
    )
    .map_err(|e| e.to_string())?;
    log::info!("[Sync] Generated new device_id: {id}");
    Ok(id)
}

// User session stored locally after login
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSession {
    pub user_id: String,
    pub team_id: Option<String>,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub email: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CurrentUserView {
    pub user_id: String,
    pub team_id: Option<String>,
    pub email: String,
}

fn cloud_http_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|e| e.to_string())
}

pub fn start_sync_thread(db_path: std::path::PathBuf) {
    let path_clone = db_path.clone();
    thread::spawn(move || {
        // Run once immediately so the first cloud batch is not delayed by SYNC_INTERVAL_MINS.
        let _ = perform_sync(&path_clone);
        loop {
            thread::sleep(Duration::from_secs(SYNC_INTERVAL_MINS * 60));
            let _ = perform_sync(&path_clone);
        }
    });
}

/// Proactively refreshes the Supabase session when the access token is missing, expired,
/// or close to expiry. Safe to call from a background thread.
pub(crate) fn refresh_session_if_expiring(db_path: &std::path::PathBuf) {
    let Ok(conn) = Connection::open(db_path) else {
        return;
    };
    let Some(session) = get_user_session(&conn) else {
        return;
    };
    if session.refresh_token.is_none() {
        return;
    }

    let exp = jwt_exp(&session.access_token);
    let now = chrono::Utc::now().timestamp();
    if exp > 0 && exp - now > JWT_REFRESH_MARGIN_SECS {
        return;
    }

    match refresh_supabase_token(&session) {
        Ok(new_session) => {
            println!(
                "[Sync] Proactive JWT refresh OK (previous access exp: {})",
                exp
            );
            if let Ok(entitlements) = crate::entitlements::refresh_entitlements_from_supabase(
                &new_session.access_token,
                new_session.team_id.as_deref(),
            ) {
                let _ = crate::entitlements::save_entitlements(&conn, &entitlements);
            }
        }
        Err(e) => println!("[Sync] Proactive JWT refresh failed: {}", e),
    }
}

pub fn start_token_refresh_thread(db_path: std::path::PathBuf) {
    thread::spawn(move || {
        refresh_session_if_expiring(&db_path);
        loop {
            thread::sleep(Duration::from_secs(TOKEN_REFRESH_POLL_SECS));
            refresh_session_if_expiring(&db_path);
        }
    });
}

#[tauri::command]
pub fn force_sync_now() -> Result<String, String> {
    crate::sync_env::require_cloud()?;
    let db_path = crate::paths::db_path()?;
    crate::entitlements::require_feature(&db_path, "sync")?;
    match perform_sync(&db_path) {
        Ok(summary) => Ok(format!("Sync Report:\n\n{}", summary)),
        Err(e) => Err(format!("Sync failed: {}", e)),
    }
}

// Get user session from local config
fn auth_provider_can_supply_user_session(provider: &str) -> bool {
    matches!(provider, "google" | "manual" | "cloud")
}

pub(crate) fn get_user_session_from_conn(conn: &Connection) -> Option<UserSession> {
    match crate::secure_store::get_secret("user_session") {
        Ok(Some(json)) => {
            if let Ok(session) = serde_json::from_str(&json) {
                return Some(session);
            }
            log::error!("[Sync] Invalid user session found in macOS Keychain");
            return None;
        }
        Ok(None) => {}
        Err(error) => {
            log::error!("[Sync] {error}");
            return None;
        }
    }

    // One-time migration from legacy plaintext SQLite storage.
    if let Some(legacy) = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'user_session'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|json| serde_json::from_str::<UserSession>(&json).ok())
    {
        if let Ok(json) = serde_json::to_string(&legacy) {
            if crate::secure_store::set_secret("user_session", &json).is_ok() {
                let _ = conn.execute("DELETE FROM config WHERE key = 'user_session'", []);
            }
        }
        return Some(legacy);
    }

    if let Ok(Some(json)) = crate::secure_store::get_secret("auth_session") {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
            let provider = value["provider"].as_str().unwrap_or("");
            if auth_provider_can_supply_user_session(provider) {
                let session = UserSession {
                    user_id: value["user"]["id"].as_str()?.to_string(),
                    team_id: None,
                    access_token: value["access_token"].as_str()?.to_string(),
                    refresh_token: value["refresh_token"].as_str().map(String::from),
                    email: value["user"]["email"].as_str()?.to_string(),
                };
                if let Ok(serialized) = serde_json::to_string(&session) {
                    let _ = crate::secure_store::set_secret("user_session", &serialized);
                }
                return Some(session);
            }
        }
    }

    let legacy_auth: Option<serde_json::Value> = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'auth_session'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|json_str| serde_json::from_str(&json_str).ok());
    let value = legacy_auth?;
    let provider = value["provider"].as_str().unwrap_or("");
    if !auth_provider_can_supply_user_session(provider) {
        return None;
    }
    let session = UserSession {
        user_id: value["user"]["id"].as_str()?.to_string(),
        team_id: None,
        access_token: value["access_token"].as_str()?.to_string(),
        refresh_token: value["refresh_token"].as_str().map(String::from),
        email: value["user"]["email"].as_str()?.to_string(),
    };
    let serialized = serde_json::to_string(&session).ok()?;
    crate::secure_store::set_secret("auth_session", &value.to_string()).ok()?;
    crate::secure_store::set_secret("user_session", &serialized).ok()?;
    let _ = conn.execute("DELETE FROM config WHERE key = 'auth_session'", []);
    Some(session)
}

fn get_user_session(conn: &Connection) -> Option<UserSession> {
    get_user_session_from_conn(conn)
}

// Save user session to local config
pub fn save_user_session(
    user_id: String,
    team_id: Option<String>,
    access_token: String,
    refresh_token: Option<String>,
    email: String,
) -> Result<(), String> {
    let db_path = crate::paths::db_path()?;
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;

    let session = UserSession {
        user_id,
        team_id,
        access_token,
        refresh_token,
        email,
    };
    let json = serde_json::to_string(&session).map_err(|e| e.to_string())?;

    crate::secure_store::set_secret("user_session", &json)?;
    let metadata = serde_json::json!({
        "user_id": &session.user_id,
        "team_id": &session.team_id,
        "email": &session.email,
    });
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value) VALUES ('user_session_meta', ?1)",
        [metadata.to_string()],
    )
    .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM config WHERE key = 'user_session'", [])
        .map_err(|e| e.to_string())?;

    log::info!("[Sync] user session saved in macOS Keychain");
    Ok(())
}

pub(crate) fn refresh_supabase_token(session: &UserSession) -> Result<UserSession, String> {
    let refresh_token = session
        .refresh_token
        .as_ref()
        .ok_or("No refresh token available in session")?;

    println!("[Sync] Attempting Supabase token refresh");
    let client = cloud_http_client()?;
    let url = format!("{}/auth/v1/token?grant_type=refresh_token", supabase_url());

    let resp = client
        .post(&url)
        .header("apikey", supabase_anon_key())
        .json(&serde_json::json!({ "refresh_token": refresh_token }))
        .send()
        .map_err(|e| e.to_string())?;

    let status = resp.status();
    if !status.is_success() {
        let err_body = resp.text().unwrap_or_default();
        println!("[Sync] Refresh failed: {}", err_body);
        return Err(format!("Refresh failed (HTTP {}): {}", status, err_body));
    }

    let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    let new_access = json["access_token"]
        .as_str()
        .ok_or("Missing access_token in refresh response")?;
    let new_refresh = json["refresh_token"].as_str();

    let mut new_session = session.clone();
    new_session.access_token = new_access.to_string();
    if let Some(r) = new_refresh {
        new_session.refresh_token = Some(r.to_string());
    }

    // Save updated session
    save_user_session(
        new_session.user_id.clone(),
        new_session.team_id.clone(),
        new_session.access_token.clone(),
        new_session.refresh_token.clone(),
        new_session.email.clone(),
    )?;

    Ok(new_session)
}

// Clear user session (logout)
#[tauri::command]
pub fn clear_user_session() -> Result<(), String> {
    let db_path = crate::paths::db_path()?;
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;

    crate::secure_store::delete_secret("user_session")?;
    conn.execute(
        "DELETE FROM config WHERE key IN ('user_session', 'user_session_meta')",
        [],
    )
    .map_err(|e| e.to_string())?;
    crate::entitlements::clear_entitlements(&conn)?;

    println!("[Sync] User session cleared");
    Ok(())
}

// Check if user is logged in
#[tauri::command]
pub fn get_current_user() -> Result<Option<CurrentUserView>, String> {
    let db_path = crate::paths::db_path()?;
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    Ok(get_user_session(&conn).map(|session| CurrentUserView {
        user_id: session.user_id,
        team_id: session.team_id,
        email: session.email,
    }))
}

fn perform_sync(db_path: &std::path::PathBuf) -> Result<String, String> {
    let sync_mutex = SYNC_LOCK.get_or_init(|| Mutex::new(()));
    let _sync_guard = match sync_mutex.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::WouldBlock) => {
            return Ok("A cloud sync is already in progress.".to_string());
        }
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
    };
    refresh_session_if_expiring(db_path);

    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    // Check if user is logged in
    let session = match get_user_session(&conn) {
        Some(s) => s,
        None => {
            println!("[CloudSync] No user session found. Sync disabled.");
            return Ok("Not logged in - sync disabled".to_string());
        }
    };

    if let Err(reason) = crate::entitlements::require_feature(db_path, "sync") {
        println!("[CloudSync] {}", reason);
        return Ok(reason);
    }

    if let Ok(entitlements) = crate::entitlements::refresh_entitlements_from_supabase(
        &session.access_token,
        session.team_id.as_deref(),
    ) {
        let _ = crate::entitlements::save_entitlements(&conn, &entitlements);
        if !entitlements.can_sync {
            println!("[CloudSync] License inactive — sync disabled.");
            return Ok("License inactive — sync disabled".to_string());
        }
    }

    println!("[CloudSync] REST base: {}", supabase_url());

    let batch_limit = std::env::var("FLOWMATES_SYNC_BATCH_LIMIT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(CLOUDSYNC_BATCH_LIMIT_DEFAULT)
        .min(5000) as usize;

    let team_id = session.team_id.as_deref().unwrap_or("none");

    let total_unsynced: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM reports WHERE synced = 0 AND owner_user_id = ?1 AND owner_team_id IS ?2",
            params![session.user_id, session.team_id],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0);
    println!(
        "[CloudSync] Pending unsynced reports for team {}: {} (uploading oldest up to {} rows)",
        team_id, total_unsynced, batch_limit
    );

    let mut stmt = conn
        .prepare(
            "SELECT id, description, activity_type, duration_seconds, jira_ticket_id, created_at
         FROM reports
         WHERE synced = 0 AND owner_user_id = ?1 AND owner_team_id IS ?2
         ORDER BY id ASC
         LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(
            params![session.user_id, session.team_id, batch_limit as i64],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i32>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;

    let mut ids = Vec::new();
    let mut full_text = String::new();
    let mut total_duration = 0;
    let mut first_created_at: Option<String> = None;
    let mut last_created_at: Option<String> = None;

    // Aggregations
    let mut categories = std::collections::HashMap::new();
    let mut tickets = std::collections::HashMap::new();

    let line_cap = std::env::var("FLOWMATES_SUMMARY_MAX_LINE_CHARS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n >= 80)
        .unwrap_or(SUMMARY_MAX_LINE_CHARS_DEFAULT);
    let summary_max_chars = std::env::var("FLOWMATES_SUMMARY_MAX_CHARS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n >= line_cap)
        .unwrap_or(SUMMARY_MAX_TASK_CHARS);

    for (id, desc, cat, dur, ticket, created_at) in rows.flatten() {
        let desc = clamp_line_for_summary(&desc, line_cap);
        let line = format!("- [{}] {}\n", cat, desc);
        if !ids.is_empty() && full_text.chars().count() + line.chars().count() > summary_max_chars {
            break;
        }
        ids.push(id);
        full_text.push_str(&line);
        total_duration += dur;

        if first_created_at.is_none() {
            first_created_at = created_at.clone();
        }
        last_created_at = created_at;

        // Stats
        *categories.entry(cat).or_insert(0) += dur;
        if let Some(t) = ticket {
            if !t.is_empty() {
                *tickets.entry(t).or_insert(0) += dur;
            }
        }
    }

    if (total_unsynced as usize) > ids.len() {
        println!(
            "[CloudSync] {} more unsynced report(s) queued after this batch; will upload on the next run.",
            total_unsynced as usize - ids.len()
        );
    }

    if ids.is_empty() {
        println!("[CloudSync] No new reports to sync.");
        return Ok("No new activity to report.".to_string());
    }

    // 2. Generate summary with local vision model
    println!("[CloudSync] Summarizing {} reports...", ids.len());
    let summary = summarize_with_vision_model(&full_text).map_err(|e| {
        println!(
            "[CloudSync] Summary generation failed; keeping rows pending: {}",
            e
        );
        format!("Local summary unavailable; nothing was uploaded: {e}")
    })?;
    println!(
        "[CloudSync] Summary generated ({} chars): {:.120}",
        summary.len(),
        summary
    );
    let device_id = get_or_create_device_id(&conn).unwrap_or_else(|_| "unknown".to_string());
    let batch_id = format!(
        "{}:{}:{}:{}:{}:{}",
        session.user_id,
        team_id,
        device_id,
        ids.first().copied().unwrap_or_default(),
        ids.last().copied().unwrap_or_default(),
        ids.len(),
    );
    let batch_id_short =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(batch_id.as_bytes());
    let batch_id = format!("v1:{}", &batch_id_short[..batch_id_short.len().min(64)]);

    // 3. Upload to Supabase with user authentication (retry on JWT expired)
    let upload_result = upload_session(
        &session,
        &batch_id,
        total_duration,
        &summary,
        &categories,
        &tickets,
        last_created_at.as_deref(),
    );
    let upload_result = match &upload_result {
        Err(e) if e.contains("401") || e.contains("PGRST3") => {
            println!(
                "[CloudSync] Auth error detected ({}), attempting JWT refresh...",
                e
            );
            let conn_refresh = Connection::open(db_path).map_err(|e| e.to_string())?;
            let session_for_refresh =
                get_user_session(&conn_refresh).unwrap_or_else(|| session.clone());
            match refresh_supabase_token(&session_for_refresh) {
                Ok(refreshed) => upload_session(
                    &refreshed,
                    &batch_id,
                    total_duration,
                    &summary,
                    &categories,
                    &tickets,
                    last_created_at.as_deref(),
                ),
                Err(ref_err) => {
                    println!("[CloudSync] Token refresh failed: {}", ref_err);
                    upload_result
                }
            }
        }
        _ => upload_result,
    };

    match upload_result {
        Ok(_) => {
            println!(
                "[CloudSync] Upload success for {} — in Supabase open public.work_sessions and public.activity_reports (local dev-agent.db table \"reports\" is not uploaded as raw rows).",
                session.email
            );

            let primary_category = categories
                .iter()
                .max_by_key(|(_, &secs)| secs)
                .map(|(c, _)| c.as_str())
                .unwrap_or("mixed")
                .to_string();

            let primary_jira = tickets
                .iter()
                .max_by_key(|(_, &secs)| secs)
                .map(|(t, _)| t.clone());

            // Same AI summary as work_sessions — not the raw SQLite log (that stays local only).
            let captured_at = last_created_at
                .as_deref()
                .and_then(|ts| {
                    chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S")
                        .ok()
                        .map(|ndt| format!("{}Z", ndt.format("%Y-%m-%dT%H:%M:%S")))
                })
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

            let activity_body = serde_json::json!({
                "user_id": session.user_id,
                "team_id": session.team_id,
                "description": summary.clone(),
                "category": primary_category,
                "jira_ticket_id": primary_jira,
                "duration_seconds": total_duration,
                "captured_at": captured_at,
                "client_batch_id": batch_id,
            });

            post_activity_report_with_refresh(db_path, &session, &activity_body).map_err(|e| {
                format!("activity_reports upload failed; local rows remain pending: {e}")
            })?;
            println!("[CloudSync] activity_reports: AI window summary saved");

            let id_list = ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let updated = conn
                .execute(
                    &format!("UPDATE reports SET synced = 1 WHERE id IN ({})", id_list),
                    [],
                )
                .map_err(|e| {
                    format!("Cloud upload succeeded but local sync state update failed: {e}")
                })?;
            if updated != ids.len() {
                return Err(format!(
                    "Cloud upload succeeded but only {updated}/{} local rows were marked synced",
                    ids.len()
                ));
            }

            let sync_meta = serde_json::json!({
                "at": chrono::Utc::now().to_rfc3339(),
                "rows_marked_synced": ids.len(),
                "supabase_project_host": supabase_url()
                    .trim_end_matches('/')
                    .trim_start_matches("https://"),
                "tables": "work_sessions, activity_reports",
            });
            let _ = conn.execute(
                "INSERT OR REPLACE INTO config (key, value) VALUES ('last_cloud_sync', ?1)",
                [sync_meta.to_string()],
            );
            let base = supabase_url();
            let host = base.trim_end_matches('/').trim_start_matches("https://");
            println!(
                "[CloudSync] (testing) {} UTC | {} local capture(s) → {} | work_sessions + activity_reports",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"),
                ids.len(),
                host
            );
        }
        Err(e) => {
            if e.contains("License expired") || e.contains("403") {
                println!("[CloudSync] LICENSE EXPIRED - Sync blocked");
                return Err("License expired. Contact your PM to renew.".to_string());
            }

            println!("[CloudSync] Upload failed: {}", e);
            return Err(format!(
                "Cloud upload failed; local rows remain pending: {e}"
            ));
        }
    }

    println!("[CloudSync] Processed {} reports.", ids.len());
    Ok(summary)
}

fn summarize_with_vision_model(text: &str) -> Result<String, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;

    let max_chars = std::env::var("FLOWMATES_SUMMARY_MAX_CHARS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(SUMMARY_MAX_TASK_CHARS);

    let n_chars = text.chars().count();
    if n_chars > max_chars {
        println!(
            "[CloudSync] Truncating summary TASKS from {} to {} Unicode chars (local n_ctx limit)",
            n_chars, max_chars
        );
    }
    let tasks = truncate_tasks_for_summary(text, max_chars);

    // Keep instructions short to preserve token budget for TASKS.
    let prompt = format!(
        "Summarize the developer activity below in ONE short paragraph. Use only facts from the list; do not invent work.\n\nTASKS:\n{}",
        tasks
    );

    let body = serde_json::json!({
        "model": LLAMA_CHAT_MODEL_ID,
        "messages": [{ "role": "user", "content": prompt }],
        "temperature": 0.3,
        "max_tokens": 384
    });

    let resp = client
        .post(
            crate::llama_port::managed_chat_completions_url().ok_or_else(|| {
                "Local AI server offline — cannot summarize (start Local AI monitoring first)"
                    .to_string()
            })?,
        )
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let status = resp.status();
        let err_body = resp.text().unwrap_or_default();
        return Err(format!("Summary request failed ({}): {}", status, err_body));
    }

    let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");
    if content.is_empty() {
        return Err("Model returned empty summary.".to_string());
    }

    Ok(content.to_string())
}

fn upload_session(
    session: &UserSession,
    batch_id: &str,
    duration: i32,
    summary: &str,
    categories: &std::collections::HashMap<String, i32>,
    tickets: &std::collections::HashMap<String, i32>,
    last_created_at: Option<&str>,
) -> Result<(), String> {
    let client = cloud_http_client()?;
    let url = format!(
        "{}/rest/v1/work_sessions?on_conflict=client_batch_id",
        supabase_url()
    );

    let (session_date, created_at) = match last_created_at
        .and_then(|ts| chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S").ok())
    {
        Some(ndt) => {
            let rfc = format!("{}Z", ndt.format("%Y-%m-%dT%H:%M:%S"));
            let date = ndt.format("%Y-%m-%d").to_string();
            (date, rfc)
        }
        None => (
            chrono::Local::now().format("%Y-%m-%d").to_string(),
            chrono::Utc::now().to_rfc3339(),
        ),
    };

    let body = serde_json::json!({
        "user_id": session.user_id,
        "team_id": session.team_id,
        "duration_seconds": duration,
        "summary": summary,
        "category_breakdown": categories,
        "jira_breakdown": tickets,
        "session_date": session_date,
        "created_at": created_at,
        "client_batch_id": batch_id,
    });

    let resp = client
        .post(&url)
        .header("apikey", supabase_anon_key())
        .header("Authorization", format!("Bearer {}", session.access_token))
        .header("Content-Type", "application/json")
        .header("Prefer", "resolution=ignore-duplicates,return=minimal")
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;

    let status = resp.status();

    if status.as_u16() == 403 {
        return Err("License expired or invalid".to_string());
    }

    if !status.is_success() {
        let body_text = resp.text().unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, body_text));
    }

    Ok(())
}

fn post_activity_report_row(
    session: &UserSession,
    body: &serde_json::Value,
) -> Result<Response, String> {
    let client = cloud_http_client()?;
    let url = format!(
        "{}/rest/v1/activity_reports?on_conflict=client_batch_id",
        supabase_url()
    );
    client
        .post(&url)
        .header("apikey", supabase_anon_key())
        .header("Authorization", format!("Bearer {}", session.access_token))
        .header("Content-Type", "application/json")
        .header("Prefer", "resolution=ignore-duplicates,return=minimal")
        .json(body)
        .send()
        .map_err(|e| e.to_string())
}

/// Retries once with a refreshed JWT if the first POST returns 401/403.
fn post_activity_report_with_refresh(
    db_path: &std::path::PathBuf,
    session: &UserSession,
    body: &serde_json::Value,
) -> Result<(), String> {
    let mut resp = post_activity_report_row(session, body)?;
    if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
        let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
        if let Some(s) = get_user_session(&conn) {
            if let Ok(new_s) = refresh_supabase_token(&s) {
                resp = post_activity_report_row(&new_s, body)?;
            }
        }
    }
    let status = resp.status();
    if status.as_u16() == 403 {
        return Err("License expired or invalid".to_string());
    }
    if !status.is_success() {
        let t = resp.text().unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, t));
    }
    Ok(())
}

// Upload individual activity report (for granular tracking)
#[allow(dead_code)]
pub fn upload_activity_report(
    description: String,
    category: String,
    jira_ticket_id: Option<String>,
    duration_seconds: i32,
) -> Result<(), String> {
    let db_path = crate::paths::db_path()?;
    crate::entitlements::require_feature(&db_path, "sync")?;
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;

    let session = get_user_session(&conn).ok_or("Not logged in")?;

    let captured_at = conn
        .query_row(
            "SELECT created_at FROM reports WHERE synced = 0 ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|ts| {
            chrono::NaiveDateTime::parse_from_str(&ts, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|ndt| format!("{}Z", ndt.format("%Y-%m-%dT%H:%M:%S")))
        })
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    let body = serde_json::json!({
        "user_id": session.user_id,
        "team_id": session.team_id,
        "description": description,
        "category": category,
        "jira_ticket_id": jira_ticket_id,
        "duration_seconds": duration_seconds,
        "captured_at": captured_at,
    });

    let resp = post_activity_report_row(&session, &body)?;

    if resp.status().as_u16() == 403 {
        return Err("License expired or invalid".to_string());
    }

    resp.error_for_status().map_err(|e| e.to_string())?;

    Ok(())
}

// Get all teams the current user belongs to
#[tauri::command]
pub fn get_user_teams() -> Result<serde_json::Value, String> {
    let db_path = crate::paths::db_path()?;
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;

    let session = get_user_session(&conn).ok_or("Not logged in")?;

    let client = cloud_http_client()?;
    let mut current_token = session.access_token.clone();

    // Fetch team memberships from Supabase
    let url = format!(
        "{}/rest/v1/team_members?user_id=eq.{}&select=team_id,role,joined_at",
        supabase_url(),
        session.user_id
    );

    let mut resp = client
        .get(&url)
        .header("apikey", supabase_anon_key())
        .header("Authorization", format!("Bearer {}", current_token))
        .send()
        .map_err(|e| e.to_string())?;

    // Retry on 401/403
    if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
        if let Ok(new_s) = refresh_supabase_token(&session) {
            current_token = new_s.access_token.clone();
            resp = client
                .get(&url)
                .header("apikey", supabase_anon_key())
                .header("Authorization", format!("Bearer {}", current_token))
                .send()
                .map_err(|e| e.to_string())?;
        }
    }

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(format!("Failed to fetch teams (HTTP {}): {}", status, body));
    }

    let teams: Vec<serde_json::Value> = resp.json().map_err(|e| e.to_string())?;

    // Auto-select the first team when none is persisted as active yet.
    // Prevents activity_reports/work_sessions from uploading with team_id=NULL when the
    // user already has membership(s) but never touched the dropdown (the browser shows
    // the first option without firing `change`, so `set_active_team` was never called).
    let active_team_id = match session.team_id.clone() {
        Some(id) => Some(id),
        None => {
            let first = teams
                .first()
                .and_then(|t| t["team_id"].as_str().map(|s| s.to_string()));
            if let Some(id) = first.clone() {
                println!(
                    "[Team] No active team in session; auto-selecting first membership: {}",
                    id
                );
                save_user_session(
                    session.user_id.clone(),
                    Some(id.clone()),
                    current_token.clone(),
                    session.refresh_token.clone(),
                    session.email.clone(),
                )?;
            }
            first
        }
    };

    println!(
        "[Team] Found {} team memberships, active: {:?}",
        teams.len(),
        active_team_id
    );

    Ok(serde_json::json!({
        "teams": teams,
        "active_team_id": active_team_id
    }))
}

// Set the active team for the current user (persists to SQLite)
#[tauri::command]
pub fn set_active_team(team_id: String) -> Result<(), String> {
    let db_path = crate::paths::db_path()?;
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;

    let session = get_user_session(&conn).ok_or("Not logged in")?;
    let entitlements = crate::entitlements::refresh_entitlements_from_supabase(
        &session.access_token,
        Some(&team_id),
    )?;
    if !entitlements.team_ids.iter().any(|id| id == &team_id) {
        return Err("You are not a member of that team".to_string());
    }
    crate::entitlements::save_entitlements(&conn, &entitlements)?;

    println!("[Team] Setting active team to: {}", team_id);

    save_user_session(
        session.user_id,
        Some(team_id),
        session.access_token,
        session.refresh_token,
        session.email,
    )
}

// Join a team using an invitation token
#[tauri::command]
pub fn join_team(token: String) -> Result<serde_json::Value, String> {
    let token = token.trim();
    if token.len() < 16
        || token.len() > 256
        || !token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err("Invalid invitation token".to_string());
    }

    let db_path = crate::paths::db_path()?;
    refresh_session_if_expiring(&db_path);
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    let mut session = get_user_session(&conn).ok_or("Not logged in. Please sign in first.")?;
    let url = format!("{}/rest/v1/rpc/accept_team_invitation", supabase_url());
    let client = cloud_http_client()?;
    let payload = serde_json::json!({ "invitation_token": token });

    let send = |access_token: &str| {
        client
            .post(&url)
            .header("apikey", supabase_anon_key())
            .header("Authorization", format!("Bearer {access_token}"))
            .json(&payload)
            .send()
            .map_err(|e| e.to_string())
    };
    let mut response = send(&session.access_token)?;
    if matches!(response.status().as_u16(), 401 | 403) {
        session = refresh_supabase_token(&session)?;
        response = send(&session.access_token)?;
    }
    let status = response.status();
    let body: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body["message"]
            .as_str()
            .or_else(|| body["error"].as_str())
            .unwrap_or("Could not accept invitation")
            .to_string());
    }
    let team_id = body
        .as_str()
        .or_else(|| body["team_id"].as_str())
        .ok_or("Invitation response did not contain a team ID")?
        .to_string();

    save_user_session(
        session.user_id,
        Some(team_id.clone()),
        session.access_token,
        session.refresh_token,
        session.email,
    )?;
    Ok(serde_json::json!({ "success": true, "team_id": team_id }))
}

#[cfg(test)]
mod user_session_tests {
    use super::{auth_provider_can_supply_user_session, get_or_create_device_id, UserSession};
    use rusqlite::{params, Connection};

    #[test]
    fn cloud_auth_provider_can_rebuild_sync_session() {
        assert!(auth_provider_can_supply_user_session("cloud"));
        assert!(auth_provider_can_supply_user_session("google"));
        assert!(!auth_provider_can_supply_user_session("local"));
    }

    #[test]
    fn user_session_json_roundtrip() {
        let s = UserSession {
            user_id: "u1".into(),
            team_id: Some("t1".into()),
            access_token: "at".into(),
            refresh_token: Some("rt".into()),
            email: "a@b.c".into(),
        };
        let j = serde_json::to_string(&s).unwrap();
        let back: UserSession = serde_json::from_str(&j).unwrap();
        assert_eq!(back.email, s.email);
        assert_eq!(back.team_id, s.team_id);
    }

    #[test]
    fn device_id_generates_and_persists() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE config (key TEXT PRIMARY KEY, value TEXT);
             CREATE TABLE reports (
                id INTEGER PRIMARY KEY,
                description TEXT,
                activity_type TEXT,
                synced INTEGER DEFAULT 0,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                owner_user_id TEXT,
                owner_team_id TEXT
             );",
        )
        .unwrap();

        let first = get_or_create_device_id(&conn).unwrap();
        assert!(!first.is_empty());
        assert_eq!(first.len(), 36); // UUID v4 with hyphens

        let second = get_or_create_device_id(&conn).unwrap();
        assert_eq!(second, first); // same device ID on second call

        let stored: String = conn
            .query_row(
                "SELECT value FROM config WHERE key = 'device_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, first);
    }

    #[test]
    fn sync_selects_rows_filtered_by_owner_and_team() {
        let conn = Connection::open_in_memory().unwrap();
        let schema = "CREATE TABLE reports (
            id INTEGER PRIMARY KEY,
            description TEXT,
            activity_type TEXT,
            synced INTEGER DEFAULT 0,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            duration_seconds INTEGER DEFAULT 30,
            jira_ticket_id TEXT,
            owner_user_id TEXT,
            owner_team_id TEXT
        )";
        conn.execute_batch(schema).unwrap();

        conn.execute(
            "INSERT INTO reports (description, activity_type, synced, owner_user_id, owner_team_id)
             VALUES ('team-a-row-a', 'Coding', 0, 'user1', 'team-a')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reports (description, activity_type, synced, owner_user_id, owner_team_id)
             VALUES ('team-b-row', 'Coding', 0, 'user1', 'team-b')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reports (description, activity_type, synced, owner_user_id, owner_team_id)
             VALUES ('team-a-row-b', 'Coding', 0, 'user1', 'team-a')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reports (description, activity_type, synced, owner_user_id, owner_team_id)
             VALUES ('other-user', 'Coding', 0, 'user2', 'team-a')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reports (description, activity_type, synced, owner_user_id, owner_team_id)
             VALUES ('no-team', 'Coding', 0, 'user1', NULL)",
            [],
        )
        .unwrap();

        let team_a_rows: Vec<String> = conn
            .prepare(
                "SELECT description FROM reports
                 WHERE synced = 0 AND owner_user_id = ?1 AND owner_team_id IS ?2
                 ORDER BY id ASC",
            )
            .unwrap()
            .query_map(params!["user1", Some("team-a")], |row| {
                row.get::<_, String>(0)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(team_a_rows, vec!["team-a-row-a", "team-a-row-b"]);

        let no_team_rows: Vec<String> = conn
            .prepare(
                "SELECT description FROM reports
                 WHERE synced = 0 AND owner_user_id = ?1 AND owner_team_id IS ?2
                 ORDER BY id ASC",
            )
            .unwrap()
            .query_map(params!["user1", None::<String>], |row| {
                row.get::<_, String>(0)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(no_team_rows, vec!["no-team"]);
    }
}
