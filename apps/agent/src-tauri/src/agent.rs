use crate::agent_pure::parse_analysis;
use crate::vision_model::{
    CONFIG_VISION_MODEL_ID, LLAMA_CHAT_MODEL_ID, VISION_GGUF_FILENAME, VISION_MMPROJ_FILENAME,
    VISION_STATUS_LABEL,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::{DateTime, Datelike, Local, NaiveDateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Emitter, State};

pub type AgentState = Mutex<Option<FlowmatesAgent>>;

const SQLITE_BUSY_TIMEOUT_SECS: u64 = 8;
const REPORTS_SCHEMA_VERSION: i32 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReportOwnerScope {
    #[allow(dead_code)]
    All,
    User(String),
    LocalUnowned,
}

impl ReportOwnerScope {
    pub(crate) fn sql_mode_and_user(&self) -> (i32, Option<&str>) {
        match self {
            Self::All => (0, None),
            Self::User(user_id) => (1, Some(user_id)),
            Self::LocalUnowned => (2, None),
        }
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::User(_) => "user",
            Self::LocalUnowned => "local_unowned",
        }
    }
}

pub(crate) fn current_report_owner_scope(conn: &Connection) -> ReportOwnerScope {
    crate::sync::get_user_session_from_conn(conn)
        .map(|session| ReportOwnerScope::User(session.user_id))
        .unwrap_or(ReportOwnerScope::LocalUnowned)
}

pub(crate) fn open_agent_database(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| format!("SQLite open {:?}: {e}", path))?;
    conn.busy_timeout(Duration::from_secs(SQLITE_BUSY_TIMEOUT_SECS))
        .map_err(|e| format!("SQLite busy_timeout {:?}: {e}", path))?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| format!("SQLite synchronous pragma {:?}: {e}", path))?;
    Ok(conn)
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| e.to_string())?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?;
    for name in names {
        if name.map_err(|e| e.to_string())? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActivityReport {
    pub id: Option<i64>,
    pub timestamp: String,
    pub description: String,
    pub activity_type: String,
    pub synced: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentConfig {
    #[serde(rename = "captureInterval")]
    pub capture_interval: Option<u64>,
    #[serde(rename = "visionModel")]
    pub vision_model: Option<String>,
    /// `None` or `-1` => automatic GPU layer ladder on local llama-server.
    /// `Some(n)` for `n >= 0` => fixed `--n-gpu-layers` (manual / power user).
    #[serde(rename = "gpuLayers")]
    pub gpu_layers: Option<i32>,
    #[serde(rename = "dailyGoalHours")]
    pub daily_goal_hours: Option<f64>,
}

pub struct FlowmatesAgent {
    pub config: AgentConfig,
    pub is_running: bool,
    pub reports_sent: u32,
    pub db_path: PathBuf,
}

impl FlowmatesAgent {
    pub fn new() -> Result<Self, String> {
        let db_path = crate::paths::db_path()?;

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create database directory {:?}: {e}", parent))?;
        }

        let mut agent = Self {
            config: AgentConfig {
                capture_interval: Some(60000),
                vision_model: Some(CONFIG_VISION_MODEL_ID.to_string()),
                // -1 = automatic tier probing (maximum compatibility + strongest profile that survives).
                gpu_layers: Some(-1),
                daily_goal_hours: Some(6.0),
            },
            is_running: false,
            reports_sent: 0,
            db_path,
        };

        agent.init_db()?;
        agent.load_config();

        // Start Background Sync (10m interval)
        crate::sync::start_sync_thread(agent.db_path.clone());
        // Proactive Supabase JWT refresh (~every 2m when near expiry)
        crate::sync::start_token_refresh_thread(agent.db_path.clone());
        // Opt-in anonymous analytics sync (~every 6h when consented)
        crate::anonymous_analytics::start_analytics_sync_thread(agent.db_path.clone());

        Ok(agent)
    }

    fn init_db(&self) -> Result<(), String> {
        let mut conn = open_agent_database(&self.db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| format!("Could not enable SQLite WAL: {e}"))?;

        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS config (
                key TEXT PRIMARY KEY,
                value TEXT
             );
              CREATE TABLE IF NOT EXISTS reports (
                 id INTEGER PRIMARY KEY,
                 description TEXT NOT NULL DEFAULT '',
                 activity_type TEXT NOT NULL DEFAULT 'General',
                 synced INTEGER NOT NULL DEFAULT 0,
                 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                 jira_ticket_id TEXT,
                 duration_seconds INTEGER NOT NULL DEFAULT 30,
                 owner_user_id TEXT,
                 owner_team_id TEXT
              );",
        )
        .map_err(|e| format!("SQLite schema bootstrap failed: {e}"))?;

        for (column, migration) in [
            (
                "jira_ticket_id",
                "ALTER TABLE reports ADD COLUMN jira_ticket_id TEXT",
            ),
            (
                "duration_seconds",
                "ALTER TABLE reports ADD COLUMN duration_seconds INTEGER NOT NULL DEFAULT 30",
            ),
            (
                "owner_user_id",
                "ALTER TABLE reports ADD COLUMN owner_user_id TEXT",
            ),
            (
                "owner_team_id",
                "ALTER TABLE reports ADD COLUMN owner_team_id TEXT",
            ),
        ] {
            if !table_has_column(&tx, "reports", column)? {
                tx.execute(migration, [])
                    .map_err(|e| format!("SQLite migration for reports.{column} failed: {e}"))?;
            }
        }

        tx.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_reports_created_at ON reports(created_at);
             CREATE INDEX IF NOT EXISTS idx_reports_synced_id ON reports(synced, id);
             CREATE INDEX IF NOT EXISTS idx_reports_owner_synced_id
                ON reports(owner_user_id, synced, id);
             CREATE INDEX IF NOT EXISTS idx_reports_owner_team_synced
                ON reports(owner_user_id, owner_team_id, synced, id);",
        )
        .map_err(|e| format!("SQLite index migration failed: {e}"))?;
        tx.pragma_update(None, "user_version", REPORTS_SCHEMA_VERSION)
            .map_err(|e| format!("SQLite schema version migration failed: {e}"))?;
        tx.commit().map_err(|e| e.to_string())?;

        std::fs::set_permissions(&self.db_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Could not secure database {:?}: {e}", self.db_path))?;
        Ok(())
    }

    fn load_config(&mut self) {
        let Ok(conn) = open_agent_database(&self.db_path) else {
            log::warn!(
                "[Agent] load_config: cannot open {:?}; using defaults",
                self.db_path
            );
            return;
        };

        if let Ok(val) = conn.query_row::<String, _, _>(
            "SELECT value FROM config WHERE key = 'vision_model'",
            [],
            |r| r.get(0),
        ) {
            self.config.vision_model = Some(val);
        }

        if let Ok(val) = conn.query_row::<String, _, _>(
            "SELECT value FROM config WHERE key = 'capture_interval'",
            [],
            |r| r.get(0),
        ) {
            if let Ok(parsed) = val.parse::<u64>() {
                self.config.capture_interval = Some(parsed.clamp(5_000, 3_600_000));
            }
        }

        if let Ok(val) = conn.query_row::<String, _, _>(
            "SELECT value FROM config WHERE key = 'gpu_layers'",
            [],
            |r| r.get(0),
        ) {
            if let Ok(parsed) = val.parse::<i32>() {
                self.config.gpu_layers = Some(parsed);
            }
        }

        if let Ok(val) = conn.query_row::<String, _, _>(
            "SELECT value FROM config WHERE key = 'daily_goal_hours'",
            [],
            |r| r.get(0),
        ) {
            if let Ok(parsed) = val.parse::<f64>() {
                self.config.daily_goal_hours = Some(parsed.clamp(0.0, 24.0));
            }
        }
    }

    fn save_config(&self) -> Result<(), String> {
        let conn = open_agent_database(&self.db_path)?;

        if let Some(vision_model) = &self.config.vision_model {
            conn.execute(
                "INSERT OR REPLACE INTO config (key, value) VALUES ('vision_model', ?1)",
                [vision_model],
            )
            .map_err(|e| e.to_string())?;
        }

        if let Some(interval) = self.config.capture_interval {
            conn.execute(
                "INSERT OR REPLACE INTO config (key, value) VALUES (?, ?)",
                params!["capture_interval", interval.to_string()],
            )
            .map_err(|e| e.to_string())?;
        }

        if let Some(layers) = self.config.gpu_layers {
            conn.execute(
                "INSERT OR REPLACE INTO config (key, value) VALUES (?, ?)",
                params!["gpu_layers", layers.to_string()],
            )
            .map_err(|e| e.to_string())?;
        }

        if let Some(hours) = self.config.daily_goal_hours {
            conn.execute(
                "INSERT OR REPLACE INTO config (key, value) VALUES (?, ?)",
                params!["daily_goal_hours", hours.to_string()],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn save_report(
        &self,
        desc: &str,
        activity_type: &str,
        ticket: Option<String>,
        duration: u64,
    ) -> Result<i64, String> {
        let conn = open_agent_database(&self.db_path)?;
        let session = crate::sync::get_user_session_from_conn(&conn);
        let owner_user_id = session.as_ref().map(|s| s.user_id.clone());
        let owner_team_id = session.as_ref().and_then(|s| s.team_id.clone());
        conn.execute(
            "INSERT INTO reports (
                description, activity_type, jira_ticket_id, duration_seconds,
                owner_user_id, owner_team_id
             ) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                desc,
                activity_type,
                ticket,
                duration,
                owner_user_id,
                owner_team_id
            ],
        )
        .map_err(|e| format!("Failed to insert local activity: {e}"))?;
        Ok(conn.last_insert_rowid())
    }

    fn get_recent(&self, limit: u32) -> Result<Vec<ActivityReport>, String> {
        let conn = open_agent_database(&self.db_path)?;
        let scope = current_report_owner_scope(&conn);
        query_recent_reports(&conn, limit, &scope)
    }
}

fn query_recent_reports(
    conn: &Connection,
    limit: u32,
    scope: &ReportOwnerScope,
) -> Result<Vec<ActivityReport>, String> {
    let mut reports = Vec::new();
    let (scope_mode, owner_user_id) = scope.sql_mode_and_user();
    let mut stmt = conn
        .prepare(
            "SELECT id, description, activity_type, synced, created_at
                 FROM reports
                 WHERE (?1 = 0)
                    OR (?1 = 1 AND owner_user_id = ?2)
                    OR (?1 = 2 AND owner_user_id IS NULL)
                 ORDER BY id DESC LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![scope_mode, owner_user_id, limit], |row| {
            Ok(ActivityReport {
                id: row.get(0).ok(),
                description: row.get(1)?,
                activity_type: row.get(2)?,
                synced: row.get::<_, i32>(3).unwrap_or(0) == 1,
                timestamp: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    for row in rows {
        reports.push(row.map_err(|e| e.to_string())?);
    }
    Ok(reports)
}

static CAPTURE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

struct CaptureLease;

impl CaptureLease {
    fn acquire() -> Result<Self, String> {
        CAPTURE_IN_PROGRESS
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| "A screen capture is already in progress".to_string())
    }
}

impl Drop for CaptureLease {
    fn drop(&mut self) {
        CAPTURE_IN_PROGRESS.store(false, Ordering::Release);
    }
}

fn capture_screen() -> Result<String, String> {
    let cg_display = core_graphics::display::CGDisplay::main();
    let cg_image = cg_display.image().ok_or_else(|| {
        let bin_path = std::env::current_exe().unwrap_or_default();
        format!(
            "CoreGraphics screen capture returned null (no image) for display ID {}. \
             macOS blocked the capture because this binary lacks Screen Recording permission.\n\n\
             Add it manually in:\n\
              System Settings → Privacy & Security → Screen Recording → + \n\
             Select: {:?}\n\n\
             Then QUIT and relaunch Flowmates completely (Cmd+Q + re-open).",
            cg_display.id, bin_path
        )
    })?;

    let width = u32::try_from(cg_image.width()).map_err(|_| "Display width is too large")?;
    let height = u32::try_from(cg_image.height()).map_err(|_| "Display height is too large")?;
    if width == 0 || height == 0 {
        return Err("CoreGraphics returned an empty display image".to_string());
    }

    let bpr = cg_image.bytes_per_row();
    let bpp = cg_image.bits_per_pixel();
    let bits_per_component = cg_image.bits_per_component();
    let pixel_bytes = bpp / 8;
    if bpp != 32 || bits_per_component != 8 || pixel_bytes != 4 {
        return Err(format!(
            "Unsupported CoreGraphics pixel format: {bpp} bits per pixel, {bits_per_component} bits per component"
        ));
    }

    let cf_data = cg_image.data();
    let raw = cf_data.bytes();

    let expected_row_bytes = width as usize * pixel_bytes;
    if bpr < expected_row_bytes {
        return Err(format!(
            "Invalid CoreGraphics row stride: {bpr} bytes for an expected minimum of {expected_row_bytes}"
        ));
    }
    let required_bytes = bpr
        .checked_mul(height as usize)
        .ok_or_else(|| "CoreGraphics image byte size overflow".to_string())?;
    if raw.len() < required_bytes {
        return Err(format!(
            "CoreGraphics image buffer is truncated: {} bytes, expected at least {required_bytes}",
            raw.len()
        ));
    }

    let packed_len = expected_row_bytes
        .checked_mul(height as usize)
        .ok_or_else(|| "Packed image byte size overflow".to_string())?;
    let mut rgba = Vec::with_capacity(packed_len);
    for y in 0..height as usize {
        let start = y * bpr;
        rgba.extend_from_slice(&raw[start..start + expected_row_bytes]);
    }

    // macOS CGDisplay returns BGRA (kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little)
    // Swap R (index 0) and B (index 2) for each 4-byte pixel -> RGBA
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    let img = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "Failed to create RgbaImage from CGImage data".to_string())?;
    let img = image::DynamicImage::ImageRgba8(img);

    let img = img.resize(960, 540, image::imageops::FilterType::Lanczos3);

    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;

    Ok(BASE64.encode(&png))
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ContextSnapshot {
    pub vector: Vec<f32>,
    pub dimension: usize,
    pub description: String,
    pub category: String, // NEW
    pub analysis_failed: bool,
    pub metadata: SnapshotMetadata,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SnapshotMetadata {
    pub task: Option<String>,
    pub file: Option<String>,
    pub app: Option<String>,
    pub branch: Option<String>,
    pub language: Option<String>,
}

#[tauri::command]
pub async fn capture_context_snapshot(
    state: State<'_, AgentState>,
    user_task: Option<String>,
    jira_ticket: Option<String>,
) -> Result<ContextSnapshot, String> {
    use crate::context::get_system_context;

    let _capture_lease = CaptureLease::acquire()?;
    let sys = get_system_context();

    // Extract config (default to 16 if not set to ensure balanced load)
    let gpu_layers = {
        let guard = state.lock().unwrap();
        guard
            .as_ref()
            .and_then(|a| a.config.gpu_layers)
            .or(Some(16))
    };

    // Run ALL heavy work on a background thread to avoid blocking the main/UI thread
    tauri::async_runtime::spawn_blocking(move || {
        // 1. Capture Screen
        let base64 = capture_screen()?;

        // 2. Local vision analysis (visual description + category)
        let task_context = jira_ticket
            .clone()
            .or(user_task.clone())
            .unwrap_or_else(|| "General".to_string());

        let raw_analysis = match analyze_image_with_vision(&base64, &task_context, gpu_layers) {
            Ok(res) => (res, false),
            Err(e) => {
                let err_msg = format!("[Agent] AI Analysis Failed: {}", e);
                println!("{}", err_msg);

                if let Ok(log_path) = crate::paths::agent_error_log_path() {
                    if let Ok(mut file) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log_path)
                    {
                        let _ = writeln!(file, "{}", err_msg);
                    }
                }

                (
                    "Screen analysis failed. Category: General".to_string(),
                    true,
                )
            }
        };

        // Parse category from response
        let (description, category) = parse_analysis(&raw_analysis.0);
        let analysis_failed =
            raw_analysis.1 || description.eq_ignore_ascii_case("No analysis available");

        // 3. Git Context (Project) — not yet resolved dynamically
        let git: Option<crate::context::GitContext> = None;

        Ok(ContextSnapshot {
            vector: vec![],
            dimension: 0,
            description,
            category,
            analysis_failed,
            metadata: SnapshotMetadata {
                task: jira_ticket.or(user_task),
                file: sys.file_name,
                app: sys.app_name,
                branch: git.and_then(|g| g.branch),
                language: None,
            },
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub fn save_activity(
    state: State<'_, AgentState>,
    description: String,
    activity_type: String,
    jira_ticket: Option<String>,
    duration_seconds: Option<i32>,
) -> Result<ActivityReport, String> {
    let mut agent = state.lock().unwrap();
    let Some(a) = agent.as_mut() else {
        return Err(
            "Agent not initialized — wait for startup to finish before capturing.".to_string(),
        );
    };
    let dur: u64 = duration_seconds.unwrap_or(60).clamp(30, 8 * 3600) as u64;
    let report_id = a.save_report(&description, &activity_type, jira_ticket, dur)?;
    a.reports_sent += 1;

    Ok(ActivityReport {
        id: Some(report_id),
        timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        description,
        activity_type,
        synced: false,
    })
}

// ============== TAURI COMMANDS ==============

/// Checks that SQLite can write to `dev-agent.db` before initialization continues.
fn probe_sqlite_database_rw() -> Result<(), String> {
    let db_path = crate::paths::db_path()?;
    let conn = open_agent_database(&db_path)?;
    conn.execute_batch(
        "BEGIN IMMEDIATE;
         CREATE TEMP TABLE IF NOT EXISTS _flowmates_io_probe (x INTEGER);
         INSERT INTO _flowmates_io_probe VALUES (1);
         COMMIT;",
    )
    .map_err(|e| {
        format!(
            "SQLite cannot write to {:?}. Verify the app data folder permissions and available disk space ({e})",
            db_path
        )
    })?;
    Ok(())
}

#[tauri::command]
pub fn initialize_agent(state: State<'_, AgentState>) -> Result<bool, String> {
    let mut g = state.lock().unwrap();
    if g.is_some() {
        return Ok(true);
    }

    crate::paths::verify_app_dir_filesystem_writable()?;
    probe_sqlite_database_rw()?;

    let max_h: u64 = std::env::var("FLOWMATES_SCREENSHOT_TMP_MAX_HOURS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(72);
    match crate::paths::prune_screenshots_tmp_older_than(Duration::from_secs(max_h * 3600)) {
        Ok(n) if n > 0 => {
            log::info!(
                "[Flowmates] removed {n} screenshot(s) older than {max_h}h from screenshots_tmp"
            );
        }
        Err(e) => log::warn!("[Flowmates] screenshots_tmp retention prune: {e}"),
        _ => {}
    }

    *g = Some(FlowmatesAgent::new()?);
    Ok(true)
}

#[tauri::command]
pub fn get_config(state: State<'_, AgentState>) -> Result<AgentConfig, String> {
    Ok(state
        .lock()
        .unwrap()
        .as_ref()
        .map(|a| a.config.clone())
        .unwrap_or_default())
}

#[tauri::command]
pub fn update_config(state: State<'_, AgentState>, patch: AgentConfig) -> Result<bool, String> {
    if let Some(agent) = state.lock().unwrap().as_mut() {
        let c = &mut agent.config;
        if patch.capture_interval.is_some() {
            c.capture_interval = patch.capture_interval;
        }
        if patch.vision_model.is_some() {
            c.vision_model = patch.vision_model;
        }
        // callers (renderer) omit `gpuLayers`; full replace here used to wipe auto/manual choice
        if patch.gpu_layers.is_some() {
            c.gpu_layers = patch.gpu_layers;
        }
        if patch.daily_goal_hours.is_some() {
            c.daily_goal_hours = patch.daily_goal_hours.map(|h| h.clamp(0.0, 24.0));
        }
        agent.save_config()?;
    }
    Ok(true)
}

#[tauri::command]
pub fn get_status(state: State<'_, AgentState>) -> Result<serde_json::Value, String> {
    let agent = state.lock().unwrap();
    Ok(if let Some(a) = agent.as_ref() {
        serde_json::json!({
            "isRunning": a.is_running,
            "reportsSent": a.reports_sent
        })
    } else {
        serde_json::json!({"isRunning": false, "reportsSent": 0})
    })
}

#[tauri::command]
pub fn start_monitoring(state: State<'_, AgentState>) -> Result<bool, String> {
    if let Some(a) = state.lock().unwrap().as_mut() {
        a.is_running = true;
    }
    Ok(true)
}

#[tauri::command]
pub fn stop_monitoring(state: State<'_, AgentState>) -> Result<bool, String> {
    if let Some(a) = state.lock().unwrap().as_mut() {
        a.is_running = false;
    }
    Ok(true)
}

#[tauri::command]
pub fn get_activity_log(
    state: State<'_, AgentState>,
    limit: Option<u32>,
) -> Result<Vec<ActivityReport>, String> {
    match state.lock().unwrap().as_ref() {
        Some(agent) => agent.get_recent(limit.unwrap_or(20).clamp(1, 500)),
        None => Ok(Vec::new()),
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DayHistoryEntry {
    pub time: String,
    pub description: String,
    pub category: String,
    pub ticket: Option<String>,
    pub duration_seconds: i32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CategoryBreakdown {
    pub category: String,
    pub total_seconds: i32,
    pub count: i32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TicketBreakdown {
    pub ticket: String,
    pub total_seconds: i32,
    pub count: i32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TodayHistory {
    pub entries: Vec<DayHistoryEntry>,
    pub total_seconds: i32,
    pub category_breakdown: Vec<CategoryBreakdown>,
    pub ticket_breakdown: Vec<TicketBreakdown>,
    pub date: String,
}

#[tauri::command]
pub fn get_today_history(state: State<'_, AgentState>) -> Result<TodayHistory, String> {
    let agent = state.lock().unwrap();
    let agent = agent.as_ref().ok_or("Agent not initialized")?;

    let conn = open_agent_database(&agent.db_path)?;
    let owner_scope = current_report_owner_scope(&conn);
    let (scope_mode, owner_user_id) = owner_scope.sql_mode_and_user();
    // Calendar 'today' in local TZ must use UTC→local conversion: `created_at`
    // defaults to CURRENT_TIMESTAMP (UTC). Comparing plain `date(created_at)`
    // to `date('now','localtime')` used mismatched halves and often returned zero rows.
    let today = Local::now().format("%Y-%m-%d").to_string();

    let mut stmt = conn
        .prepare(
            "SELECT created_at, description, activity_type, jira_ticket_id, duration_seconds
         FROM reports
         WHERE date(created_at, 'localtime') = ?1
           AND ((?2 = 0)
             OR (?2 = 1 AND owner_user_id = ?3)
             OR (?2 = 2 AND owner_user_id IS NULL))
         ORDER BY datetime(created_at) DESC",
        )
        .map_err(|e| e.to_string())?;

    let mapped_entries = stmt
        .query_map(params![today, scope_mode, owner_user_id], |row| {
            let raw_time: String = row.get::<_, String>(0).unwrap_or_default();
            let local_time = NaiveDateTime::parse_from_str(&raw_time, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|ndt| {
                    let utc = DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc);
                    utc.with_timezone(&Local)
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string()
                })
                .unwrap_or_else(|| raw_time.clone());
            Ok(DayHistoryEntry {
                time: local_time,
                description: row.get::<_, String>(1).unwrap_or_default(),
                category: row.get::<_, String>(2).unwrap_or_default(),
                ticket: row.get::<_, Option<String>>(3).unwrap_or(None),
                duration_seconds: row.get::<_, i32>(4).unwrap_or(30),
            })
        })
        .map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    for entry in mapped_entries {
        entries.push(entry.map_err(|e| e.to_string())?);
    }

    // Calculate total
    let total_seconds: i32 = entries.iter().map(|e| e.duration_seconds).sum();

    // Category breakdown
    let mut cat_map: std::collections::HashMap<String, (i32, i32)> =
        std::collections::HashMap::new();
    for e in &entries {
        let entry = cat_map.entry(e.category.clone()).or_insert((0, 0));
        entry.0 += e.duration_seconds;
        entry.1 += 1;
    }
    let category_breakdown: Vec<CategoryBreakdown> = cat_map
        .into_iter()
        .map(|(category, (total_seconds, count))| CategoryBreakdown {
            category,
            total_seconds,
            count,
        })
        .collect();

    // Ticket breakdown
    let mut ticket_map: std::collections::HashMap<String, (i32, i32)> =
        std::collections::HashMap::new();
    for e in &entries {
        if let Some(ref ticket) = e.ticket {
            let entry = ticket_map.entry(ticket.clone()).or_insert((0, 0));
            entry.0 += e.duration_seconds;
            entry.1 += 1;
        }
    }
    let ticket_breakdown: Vec<TicketBreakdown> = ticket_map
        .into_iter()
        .map(|(ticket, (total_seconds, count))| TicketBreakdown {
            ticket,
            total_seconds,
            count,
        })
        .collect();

    Ok(TodayHistory {
        entries,
        total_seconds,
        category_breakdown,
        ticket_breakdown,
        date: today,
    })
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DayActivity {
    pub date: String,
    pub weekday: String,
    pub total_seconds: i32,
    pub has_activity: bool,
    pub is_today: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WeekSummary {
    pub days: Vec<DayActivity>,
    pub yesterday_seconds: i32,
}

#[tauri::command]
pub fn get_week_summary(state: State<'_, AgentState>) -> Result<WeekSummary, String> {
    let agent = state.lock().unwrap();
    let agent = agent.as_ref().ok_or("Agent not initialized")?;

    let conn = open_agent_database(&agent.db_path)?;
    let owner_scope = current_report_owner_scope(&conn);
    let (scope_mode, owner_user_id) = owner_scope.sql_mode_and_user();
    let today = Local::now().date_naive();
    let weekday = today.weekday().num_days_from_monday();
    let week_start = today - chrono::Duration::days(weekday as i64);
    let week_end = week_start + chrono::Duration::days(6);
    let yesterday = today - chrono::Duration::days(1);

    let mut stmt = conn
        .prepare(
            "SELECT date(created_at, 'localtime') as d, SUM(duration_seconds) as total
         FROM reports
         WHERE date(created_at, 'localtime') >= ?1 AND date(created_at, 'localtime') <= ?2
           AND ((?3 = 0)
             OR (?3 = 1 AND owner_user_id = ?4)
             OR (?3 = 2 AND owner_user_id IS NULL))
         GROUP BY d",
        )
        .map_err(|e| e.to_string())?;

    let start_str = week_start.format("%Y-%m-%d").to_string();
    let end_str = week_end.format("%Y-%m-%d").to_string();

    let mut day_totals: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
    let rows = stmt
        .query_map(
            params![start_str, end_str, scope_mode, owner_user_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1).unwrap_or(0))),
        )
        .map_err(|e| e.to_string())?;

    for row in rows.filter_map(|r| r.ok()) {
        day_totals.insert(row.0, row.1);
    }

    let weekday_labels = ["M", "T", "W", "T", "F", "S", "S"];
    let mut days = Vec::with_capacity(7);
    for offset in 0..7 {
        let day = week_start + chrono::Duration::days(offset);
        let date_str = day.format("%Y-%m-%d").to_string();
        let total = day_totals.get(&date_str).copied().unwrap_or(0);
        days.push(DayActivity {
            date: date_str,
            weekday: weekday_labels[offset as usize].to_string(),
            total_seconds: total,
            has_activity: total > 0,
            is_today: day == today,
        });
    }

    let yesterday_str = yesterday.format("%Y-%m-%d").to_string();
    let yesterday_seconds = day_totals.get(&yesterday_str).copied().unwrap_or(0);

    Ok(WeekSummary {
        days,
        yesterday_seconds,
    })
}

const LOCAL_HEALTH_HTTP_TIMEOUT_SECS: u64 = 12;

/// Quick binary health check reused by diagnostics and automated tier probing.
fn local_server_health_ok() -> bool {
    let Some(health_url) = crate::llama_port::managed_health_url() else {
        return false;
    };
    let Ok(client) = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(
            LOCAL_HEALTH_HTTP_TIMEOUT_SECS,
        ))
        .build()
    else {
        return false;
    };
    client
        .get(&health_url)
        .send()
        .ok()
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

#[tauri::command]
pub fn check_local_server() -> Result<serde_json::Value, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(
            LOCAL_HEALTH_HTTP_TIMEOUT_SECS,
        ))
        .build()
        .map_err(|e| e.to_string())?;

    let Some(health_url) = crate::llama_port::managed_health_url() else {
        return Ok(serde_json::json!({
            "online": false,
            "installed": true,
            "error": "No managed llama-server port (start Local AI first)."
        }));
    };

    match client.get(&health_url).send() {
        Ok(r) if r.status().is_success() => Ok(serde_json::json!({
            "online": true,
            "installed": true,
            "models": [VISION_STATUS_LABEL],
            "hasVisionModel": true,
            "localServerPort": crate::llama_port::current_managed_listen_port(),
        })),
        Ok(r) => Ok(serde_json::json!({
            "online": false,
            "installed": true,
            "error": format!("Local server status: {}", r.status())
        })),
        Err(_) => Ok(serde_json::json!({
            "online": false,
            "installed": true
        })),
    }
}

// LLAMA SERVER COMMANDS

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ManagedServerPhase {
    Stopped,
    Starting,
    Running,
    Stopping,
}

impl ManagedServerPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
        }
    }
}

struct ManagedServerLifecycle {
    phase: ManagedServerPhase,
    generation: u64,
    child: Option<std::process::Child>,
    listen_port: Option<u16>,
}

impl ManagedServerLifecycle {
    const fn new() -> Self {
        Self {
            phase: ManagedServerPhase::Stopped,
            generation: 0,
            child: None,
            listen_port: None,
        }
    }
}

static SERVER_LIFECYCLE: Mutex<ManagedServerLifecycle> = Mutex::new(ManagedServerLifecycle::new());

fn lock_server_lifecycle() -> std::sync::MutexGuard<'static, ManagedServerLifecycle> {
    match SERVER_LIFECYCLE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::warn!("[Flowmates] managed llama-server mutex was poisoned; recovering");
            poisoned.into_inner()
        }
    }
}

fn clear_lifecycle_port(port: Option<u16>) {
    if let Some(port) = port {
        crate::llama_port::clear_managed_llama_port_if(port);
    }
}

fn reap_exited_child_locked(lifecycle: &mut ManagedServerLifecycle) -> Option<Option<i32>> {
    let child = lifecycle.child.as_mut()?;
    let exit = match child.try_wait() {
        Ok(Some(status)) => Some(status.code()),
        Ok(None) => return None,
        Err(error) => {
            log::warn!("[Flowmates] could not inspect managed llama-server: {error}");
            Some(None)
        }
    };

    lifecycle.child.take();
    let port = lifecycle.listen_port.take();
    if lifecycle.phase == ManagedServerPhase::Running {
        lifecycle.phase = ManagedServerPhase::Stopped;
    }
    clear_lifecycle_port(port);
    exit
}

fn claim_server_start() -> Result<u64, ManagedServerPhase> {
    let mut lifecycle = lock_server_lifecycle();
    if matches!(lifecycle.phase, ManagedServerPhase::Running) {
        let _ = reap_exited_child_locked(&mut lifecycle);
    }

    match lifecycle.phase {
        ManagedServerPhase::Stopped => {
            lifecycle.generation = lifecycle.generation.wrapping_add(1);
            lifecycle.phase = ManagedServerPhase::Starting;
            Ok(lifecycle.generation)
        }
        phase => Err(phase),
    }
}

fn startup_is_current(generation: u64) -> bool {
    let lifecycle = lock_server_lifecycle();
    lifecycle.generation == generation && lifecycle.phase == ManagedServerPhase::Starting
}

fn register_starting_child(
    generation: u64,
    mut child: std::process::Child,
    listen_port: u16,
) -> Result<(), String> {
    let mut lifecycle = lock_server_lifecycle();
    if lifecycle.generation != generation || lifecycle.phase != ManagedServerPhase::Starting {
        drop(lifecycle);
        let _ = child.kill();
        let _ = child.wait();
        return Err("llama-server startup was cancelled".to_string());
    }
    if lifecycle.child.is_some() {
        drop(lifecycle);
        let _ = child.kill();
        let _ = child.wait();
        return Err("A managed llama-server child is already registered".to_string());
    }

    lifecycle.child = Some(child);
    lifecycle.listen_port = Some(listen_port);
    crate::llama_port::set_managed_llama_port(listen_port);
    Ok(())
}

fn take_starting_child(generation: u64) -> (Option<std::process::Child>, Option<u16>) {
    let mut lifecycle = lock_server_lifecycle();
    if lifecycle.generation != generation || lifecycle.phase != ManagedServerPhase::Starting {
        return (None, None);
    }
    (lifecycle.child.take(), lifecycle.listen_port.take())
}

fn terminate_child(mut child: std::process::Child) -> Result<(), String> {
    match child.try_wait() {
        Ok(Some(_)) => return Ok(()),
        Ok(None) => {}
        Err(error) => log::warn!("[Flowmates] failed to inspect llama-server before stop: {error}"),
    }

    if let Err(error) = child.kill() {
        if error.kind() != std::io::ErrorKind::InvalidInput {
            return Err(format!("Could not stop managed llama-server: {error}"));
        }
    }
    child
        .wait()
        .map(|_| ())
        .map_err(|error| format!("Could not reap managed llama-server: {error}"))
}

fn discard_starting_child(generation: u64) {
    let (child, port) = take_starting_child(generation);
    clear_lifecycle_port(port);
    if let Some(child) = child {
        if let Err(error) = terminate_child(child) {
            log::warn!("[Flowmates] {error}");
        }
    }
}

fn finish_start_failure(generation: u64) {
    discard_starting_child(generation);
    let mut lifecycle = lock_server_lifecycle();
    if lifecycle.generation == generation && lifecycle.phase == ManagedServerPhase::Starting {
        lifecycle.phase = ManagedServerPhase::Stopped;
    }
}

fn mark_server_running(generation: u64) -> bool {
    let mut lifecycle = lock_server_lifecycle();
    if lifecycle.generation == generation
        && lifecycle.phase == ManagedServerPhase::Starting
        && lifecycle.child.is_some()
    {
        lifecycle.phase = ManagedServerPhase::Running;
        true
    } else {
        false
    }
}

/// Puertos nuevos ante `EADDRINUSE`/fallo rápido de escucha tras TOCTOU o TIME_WAIT.
const LLAMA_LISTEN_PORT_SPAWN_ATTEMPTS: u8 = 8;

fn clamp_llama_gpu_layers(n: i32) -> i32 {
    n.clamp(0, 16_384)
}

/// Descending Metal offload steps for vision GGUF (+ mmproj): try the highest that
/// survives startup + `/health`, then fall back toward CPU-only (`0`).
const AUTO_GPU_LAYER_TIERS: &[i32] = &[56, 40, 24, 12, 0];
/// Per-tier budget while the weights load (slow disks / AV can dominate here).
const AUTO_TIER_HEALTH_WAIT_SECS: u64 = 56;

#[derive(Clone, Copy, Debug)]
enum GpuServeMode {
    Automatic,
    Manual(i32),
}

fn gpu_serve_mode(state: &State<AgentState>) -> GpuServeMode {
    let guard = state.lock().unwrap();
    let raw = match guard.as_ref() {
        None => return GpuServeMode::Automatic,
        Some(a) => a.config.gpu_layers,
    };
    match raw {
        None | Some(-1) => GpuServeMode::Automatic,
        Some(n) if n >= 0 => GpuServeMode::Manual(clamp_llama_gpu_layers(n)),
        Some(_) => GpuServeMode::Automatic,
    }
}

/// Returns true once `/health` succeeds. A cancelled or exited startup stops early.
fn wait_for_start_health_secs(generation: u64, max_secs: u64) -> bool {
    for _ in 0..max_secs {
        if !startup_is_current(generation) {
            return false;
        }
        if local_server_health_ok() {
            return mark_server_running(generation);
        }

        let still_running = {
            let mut lifecycle = lock_server_lifecycle();
            if lifecycle.generation != generation || lifecycle.phase != ManagedServerPhase::Starting
            {
                return false;
            }
            match lifecycle.child.as_mut() {
                Some(ch) => match ch.try_wait() {
                    Ok(Some(_)) => {
                        lifecycle.child.take();
                        let port = lifecycle.listen_port.take();
                        clear_lifecycle_port(port);
                        false
                    }
                    Ok(None) => true,
                    Err(_) => {
                        lifecycle.child.take();
                        let port = lifecycle.listen_port.take();
                        clear_lifecycle_port(port);
                        false
                    }
                },
                None => false,
            }
        };

        if !still_running {
            return false;
        }

        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    false
}

/// Quick health check — does NOT start the server. Returns true if already healthy.
pub fn local_llm_is_healthy() -> bool {
    local_server_health_ok()
}

fn read_server_log_tail_chars(max_chars: usize) -> String {
    const MAX_LOG_TAIL_CHARS: usize = 64 * 1024;
    let max_chars = max_chars.min(MAX_LOG_TAIL_CHARS);
    if max_chars == 0 {
        return String::new();
    }

    let Ok(path) = crate::paths::server_log_path() else {
        return String::new();
    };
    let Ok(mut file) = std::fs::File::open(&path) else {
        return String::new();
    };
    let Ok(length) = file.metadata().map(|metadata| metadata.len()) else {
        return String::new();
    };
    let byte_budget = max_chars.saturating_mul(4).saturating_add(4) as u64;
    let start = length.saturating_sub(byte_budget);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut bytes = Vec::with_capacity((length - start) as usize);
    if file.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    let decoded = String::from_utf8_lossy(&bytes);
    last_chars(&decoded, max_chars)
}

fn last_chars(text: &str, max_chars: usize) -> String {
    let mut tail: Vec<char> = text.chars().rev().take(max_chars).collect();
    tail.reverse();
    tail.into_iter().collect()
}

/// Estado del proceso hijo que Flowmates lanzó (no confundir con un llama-server huérfano).
#[tauri::command]
pub fn llama_managed_process_status() -> Result<serde_json::Value, String> {
    let mut lifecycle = lock_server_lifecycle();
    let phase_before_reap = lifecycle.phase;
    if let Some(exit_code) = reap_exited_child_locked(&mut lifecycle) {
        let phase = lifecycle.phase;
        return Ok(serde_json::json!({
            "managed": true,
            "alive": false,
            "phase": phase.as_str(),
            "exitCode": exit_code,
        }));
    }

    let phase = lifecycle.phase;
    let managed = lifecycle.child.is_some()
        || matches!(
            phase,
            ManagedServerPhase::Starting | ManagedServerPhase::Stopping
        );
    Ok(serde_json::json!({
        "managed": managed,
        "alive": lifecycle.child.as_ref().map(|_| true),
        "phase": phase.as_str(),
        "previousPhase": phase_before_reap.as_str(),
        "localServerPort": lifecycle.listen_port,
    }))
}

#[tauri::command]
pub fn llama_server_log_tail(max_chars: Option<usize>) -> Result<String, String> {
    let n = max_chars.unwrap_or(1_200).clamp(200, 64 * 1024);
    Ok(read_server_log_tail_chars(n))
}

fn configure_llama_command(
    bin_path: &Path,
    model_path: &Path,
    mmproj_path: &Path,
    log_path: &Path,
    listen_port: u16,
    gpu_layers: i32,
    redirect_log_to_file: bool,
) -> Result<std::process::Command, String> {
    use std::process::Command;

    let n_gpu_layers = clamp_llama_gpu_layers(gpu_layers);

    let mut cmd = Command::new(bin_path);
    cmd.stdin(std::process::Stdio::null());
    cmd.arg("-m")
        .arg(model_path)
        .arg("--mmproj")
        .arg(mmproj_path)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(listen_port.to_string())
        .arg("--ctx-size")
        .arg("4096")
        .arg("--parallel")
        .arg("2")
        .arg("--threads")
        .arg("2")
        .arg("--n-gpu-layers")
        .arg(n_gpu_layers.to_string());

    // Keep the working directory next to the universal macOS binary so any
    // bundled runtime resources resolve consistently in dev and packaged apps.
    if let Some(parent) = bin_path.parent() {
        cmd.current_dir(parent);
    }

    if redirect_log_to_file {
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(log_path)
        {
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
            if let Ok(file_err) = file.try_clone() {
                cmd.stdout(std::process::Stdio::from(file));
                cmd.stderr(std::process::Stdio::from(file_err));
            }
        }
    }

    Ok(cmd)
}

fn log_suggests_listen_bind_failure(tail: &str) -> bool {
    let t = tail.to_ascii_lowercase();
    t.contains("eaddrinuse")
        || t.contains("address already in use")
        || t.contains("10048")
        || t.contains("failed to bind")
        || t.contains("bind failed")
        || t.contains("could not bind")
        || (t.contains("bind") && t.contains("in use"))
        || (t.contains("error") && t.contains("listen") && t.contains("socket"))
}

fn try_spawn_llama_process(
    bin_path: &Path,
    model_path: &Path,
    mmproj_path: &Path,
    log_path: &Path,
    listen_port: u16,
    gpu_layers: i32,
) -> Result<std::process::Child, std::io::Error> {
    let mut cmd = configure_llama_command(
        bin_path,
        model_path,
        mmproj_path,
        log_path,
        listen_port,
        gpu_layers,
        true,
    )
    .map_err(std::io::Error::other)?;
    cmd.spawn()
}

fn spawn_llama_managed_child(
    app: &tauri::AppHandle,
    generation: u64,
    gpu_layers: i32,
) -> Result<u16, String> {
    let local_llm_dir = crate::paths::resource_local_llm_dir(app)?;
    let bin_path = crate::paths::resource_llama_server_path(app)?;
    let model_path = local_llm_dir.join(VISION_GGUF_FILENAME);
    let mmproj_path = local_llm_dir.join(VISION_MMPROJ_FILENAME);

    if !bin_path.exists() {
        return Err(format!(
            "llama-server not found at {:?}. Reinstall Flowmates.",
            bin_path
        ));
    }
    if !model_path.exists() {
        return Err(format!(
            "Vision weights not found at {:?}. Reinstall Flowmates.",
            model_path
        ));
    }
    if !mmproj_path.exists() {
        return Err(format!(
            "Vision projector not found at {:?}. Reinstall Flowmates.",
            mmproj_path
        ));
    }

    let log_path = crate::paths::server_log_path()?;

    let mut last_err: Option<String> = None;

    for attempt in 0..LLAMA_LISTEN_PORT_SPAWN_ATTEMPTS {
        let listen_port = crate::llama_port::pick_localhost_listen_port()?;

        let spawn_result = try_spawn_llama_process(
            &bin_path,
            &model_path,
            &mmproj_path,
            &log_path,
            listen_port,
            gpu_layers,
        );

        match spawn_result {
            Ok(child) => {
                register_starting_child(generation, child, listen_port)?;
                std::thread::sleep(Duration::from_secs(2));

                let (exit_code, child_to_terminate, port_to_clear) = {
                    let mut lifecycle = lock_server_lifecycle();
                    if lifecycle.generation != generation
                        || lifecycle.phase != ManagedServerPhase::Starting
                    {
                        return Err("llama-server startup was cancelled".to_string());
                    }

                    match lifecycle.child.as_mut() {
                        Some(child) => match child.try_wait() {
                            Ok(None) => return Ok(listen_port),
                            Ok(Some(status)) => (
                                Some(status.code()),
                                lifecycle.child.take(),
                                lifecycle.listen_port.take(),
                            ),
                            Err(error) => {
                                log::warn!(
                                    "[Flowmates] failed to inspect llama-server during startup: {error}"
                                );
                                (
                                    Some(None),
                                    lifecycle.child.take(),
                                    lifecycle.listen_port.take(),
                                )
                            }
                        },
                        None => return Err("Managed llama-server child disappeared".to_string()),
                    }
                };

                clear_lifecycle_port(port_to_clear);
                if let Some(child) = child_to_terminate {
                    let _ = terminate_child(child);
                }

                let log_tail = read_server_log_tail_chars(1_200);
                let can_retry_port = (attempt + 1) < LLAMA_LISTEN_PORT_SPAWN_ATTEMPTS
                    && log_suggests_listen_bind_failure(&log_tail);
                if can_retry_port {
                    log::warn!(
                        "[Flowmates llama-server] quick exit (code {:?}); retrying another listen port (attempt {}/{})",
                        exit_code.flatten(),
                        attempt + 2,
                        LLAMA_LISTEN_PORT_SPAWN_ATTEMPTS
                    );
                    std::thread::sleep(Duration::from_millis(
                        40_u64.saturating_mul(u64::from(attempt) + 1),
                    ));
                    continue;
                }
                return Err(format!(
                    "llama-server exited during startup (code: {:?}). {}",
                    exit_code.flatten(),
                    if log_tail.is_empty() {
                        format!("See {:?}", log_path)
                    } else {
                        format!("Log tail: {}", log_tail)
                    }
                ));
            }
            Err(e) => {
                let msg = format!("Failed to start server: {}", e);
                last_err = Some(msg.clone());
                if (attempt + 1) < LLAMA_LISTEN_PORT_SPAWN_ATTEMPTS
                    && crate::llama_port::tcp_bind_addr_in_use(&e)
                {
                    log::warn!(
                        "[Flowmates llama-server] spawn EADDRINUSE-style error; retrying another port ({}/{}) — {}",
                        attempt + 2,
                        LLAMA_LISTEN_PORT_SPAWN_ATTEMPTS,
                        e
                    );
                    std::thread::sleep(std::time::Duration::from_millis(
                        50_u64.saturating_mul(u64::from(attempt) + 1),
                    ));
                    continue;
                }
                return Err(msg);
            }
        }
    }

    Err(last_err
        .unwrap_or_else(|| "Failed to start server: exhausted listen-port retries.".to_string()))
}

/// Starts llama-server: automatic mode walks down from a high GPU layer count until
/// `/health` answers; manual mode forces a fixed `--n-gpu-layers`.
#[tauri::command]
pub fn start_server(
    app: tauri::AppHandle,
    state: State<'_, AgentState>,
) -> Result<serde_json::Value, String> {
    let mode = gpu_serve_mode(&state);
    let generation = match claim_server_start() {
        Ok(generation) => generation,
        Err(ManagedServerPhase::Running) => {
            return Ok(serde_json::json!({
                "status": "already_running",
                "message": "Server is already running",
                "gpuAuto": false,
                "localServerPort": crate::llama_port::current_managed_listen_port(),
            }))
        }
        Err(ManagedServerPhase::Starting) => {
            return Ok(serde_json::json!({
                "status": "started_async",
                "message": "Server startup is already in progress",
            }))
        }
        Err(ManagedServerPhase::Stopping) => {
            return Err("The managed llama-server is still stopping; retry shortly".to_string())
        }
        Err(ManagedServerPhase::Stopped) => unreachable!(),
    };

    let app_clone = app.clone();
    std::thread::spawn(move || {
        let result = match mode {
            GpuServeMode::Manual(gpu_layers) => {
                let _ = app_clone.emit(
                    "server-start-progress",
                    serde_json::json!({
                        "step": "tier",
                        "layers": gpu_layers,
                        "pct": 50,
                        "status": format!("Starting server with {} GPU layers…", gpu_layers),
                    }),
                );
                match spawn_llama_managed_child(&app_clone, generation, gpu_layers) {
                    Ok(listen_port) => {
                        let alive =
                            wait_for_start_health_secs(generation, AUTO_TIER_HEALTH_WAIT_SECS);
                        if alive {
                            let _ = app_clone.emit(
                                "server-start-progress",
                                serde_json::json!({
                                    "step": "ready",
                                    "layers": gpu_layers,
                                    "pct": 100,
                                    "status": "Server ready ✓",
                                }),
                            );
                            Ok(serde_json::json!({
                                "status": "started",
                                "pid": "managed",
                                "model": VISION_STATUS_LABEL,
                                "gpuLayers": gpu_layers,
                                "gpuAuto": false,
                                "localServerPort": listen_port,
                            }))
                        } else {
                            let tail = read_server_log_tail_chars(800);
                            let detail = if tail.is_empty() {
                                String::new()
                            } else {
                                format!(". Last log: {}", tail)
                            };
                            Err(format!(
                                "Server did not become healthy within {}s{}",
                                AUTO_TIER_HEALTH_WAIT_SECS, detail
                            ))
                        }
                    }
                    Err(e) => Err(format!("Failed to spawn server: {}", e)),
                }
            }
            GpuServeMode::Automatic => auto_tier_start(&app_clone, generation),
        };
        match result {
            Ok(json) => {
                let lifecycle = lock_server_lifecycle();
                if lifecycle.generation == generation
                    && lifecycle.phase == ManagedServerPhase::Running
                {
                    drop(lifecycle);
                    let _ = app_clone.emit("server-start-ready", json);
                }
            }
            Err(e) => {
                let should_emit = startup_is_current(generation);
                finish_start_failure(generation);
                if should_emit {
                    log::error!("[Flowmates] Server startup failed: {}", e);
                    let _ = app_clone.emit(
                        "server-start-error",
                        serde_json::json!({
                            "error": e,
                        }),
                    );
                }
            }
        }
    });

    Ok(serde_json::json!({
        "status": "started_async",
    }))
}

/// Runs the automatic GPU-tier probing loop in a background thread.
/// Emits `server-start-progress` events for frontend UX.
fn auto_tier_start(app: &tauri::AppHandle, generation: u64) -> Result<serde_json::Value, String> {
    let mut last_err = String::from("unknown auto-start error");
    let total_tiers = AUTO_GPU_LAYER_TIERS.len();

    for (tier_idx, &layers) in AUTO_GPU_LAYER_TIERS.iter().enumerate() {
        if !startup_is_current(generation) {
            return Err("llama-server startup was cancelled".to_string());
        }
        let tier_pct = 20 + ((tier_idx as f64 / total_tiers as f64) * 70.0) as u32;
        let _ = app.emit(
            "server-start-progress",
            serde_json::json!({
                "step": "tier",
                "layers": layers,
                "pct": tier_pct,
                "status": format!("Starting server with {} GPU layers…", layers),
            }),
        );

        discard_starting_child(generation);
        std::thread::sleep(Duration::from_millis(350));

        let listen_port = match spawn_llama_managed_child(app, generation, layers) {
            Ok(port) => port,
            Err(error) => {
                if !startup_is_current(generation) {
                    return Err("llama-server startup was cancelled".to_string());
                }
                log::warn!(
                    "[Flowmates llama-server] automatic Metal tier gpu_layers={} failed: {}",
                    layers,
                    error
                );
                last_err = error.clone();
                let _ = app.emit(
                    "server-start-progress",
                    serde_json::json!({
                        "step": "error",
                        "layers": layers,
                        "pct": tier_pct,
                        "status": format!("Failed to start: {}", error),
                    }),
                );
                continue;
            }
        };

        log::info!(
            "[Flowmates llama-server] automatic Metal tier gpu_layers={}, waiting health up to {}s",
            layers,
            AUTO_TIER_HEALTH_WAIT_SECS
        );

        if wait_for_start_health_secs(generation, AUTO_TIER_HEALTH_WAIT_SECS) {
            let _ = app.emit(
                "server-start-progress",
                serde_json::json!({
                    "step": "ready",
                    "layers": layers,
                    "pct": 100,
                    "status": "Server ready ✓",
                }),
            );
            return Ok(serde_json::json!({
                "status": "started",
                "pid": "managed",
                "model": VISION_STATUS_LABEL,
                "gpuLayers": layers,
                "gpuAuto": true,
                "localServerPort": listen_port,
            }));
        }

        if !startup_is_current(generation) {
            return Err("llama-server startup was cancelled".to_string());
        }
        last_err = format!(
            "gpu_layers={} did not reach /health within {}s{}",
            layers,
            AUTO_TIER_HEALTH_WAIT_SECS,
            {
                let tail = read_server_log_tail_chars(800);
                if tail.is_empty() {
                    String::new()
                } else {
                    format!(". Last log excerpt: {}", tail)
                }
            }
        );
        log::warn!("[Flowmates llama-server] {}", last_err);
        discard_starting_child(generation);
        std::thread::sleep(Duration::from_millis(350));

        if tier_idx + 1 < total_tiers {
            let _ = app.emit(
                "server-start-progress",
                serde_json::json!({
                    "step": "retry",
                    "layers": layers,
                    "pct": tier_pct,
                    "status": format!("Failed with {} layers, retrying with fewer…", layers),
                }),
            );
        }
    }

    Err(format!(
        "Automatic GPU tier startup failed on all steps. {}",
        last_err
    ))
}

/// After repeated GPU failures (drivers/hardware), restarts CPU-only — slower, far more compatible.
#[tauri::command]
pub fn restart_llama_server_cpu_only(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    stop_managed_server()?;
    let generation = claim_server_start().map_err(|phase| {
        format!(
            "Could not reserve managed server while it is {}",
            phase.as_str()
        )
    })?;
    let listen_port = match spawn_llama_managed_child(&app, generation, 0) {
        Ok(port) => port,
        Err(error) => {
            finish_start_failure(generation);
            return Err(error);
        }
    };
    if !wait_for_start_health_secs(generation, AUTO_TIER_HEALTH_WAIT_SECS) {
        finish_start_failure(generation);
        return Err("CPU-only llama-server did not become healthy in time".to_string());
    }

    Ok(serde_json::json!({
        "status": "started",
        "pid": "managed",
        "model": VISION_STATUS_LABEL,
        "gpuLayers": 0,
        "cpuFallback": true,
        "gpuAuto": false,
        "localServerPort": listen_port,
    }))
}

#[tauri::command]
pub fn stop_server() -> Result<bool, String> {
    stop_managed_server()?;
    Ok(true)
}

fn stop_managed_server() -> Result<(), String> {
    let (child, port, stop_generation) = {
        let mut lifecycle = lock_server_lifecycle();
        if lifecycle.phase == ManagedServerPhase::Stopping {
            return Ok(());
        }
        if lifecycle.phase == ManagedServerPhase::Stopped && lifecycle.child.is_none() {
            let stale_port = lifecycle.listen_port.take();
            drop(lifecycle);
            clear_lifecycle_port(stale_port);
            return Ok(());
        }

        lifecycle.generation = lifecycle.generation.wrapping_add(1);
        lifecycle.phase = ManagedServerPhase::Stopping;
        let generation = lifecycle.generation;
        (
            lifecycle.child.take(),
            lifecycle.listen_port.take(),
            generation,
        )
    };

    clear_lifecycle_port(port);
    let stop_result = child.map(terminate_child).transpose().map(|_| ());

    let mut lifecycle = lock_server_lifecycle();
    if lifecycle.generation == stop_generation && lifecycle.phase == ManagedServerPhase::Stopping {
        lifecycle.phase = ManagedServerPhase::Stopped;
    }
    drop(lifecycle);
    stop_result
}

pub(crate) fn shutdown_managed_server() {
    if let Err(error) = stop_managed_server() {
        log::warn!("[Flowmates] failed to stop managed llama-server during shutdown: {error}");
    }
}

fn truncate_repetition(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 10 {
        return text.to_string();
    }

    let mut result: Vec<&str> = Vec::with_capacity(words.len());
    let mut repeat_count = 0u32;

    for (i, word) in words.iter().enumerate() {
        if i > 0 && *word == words[i - 1] {
            repeat_count += 1;
            if repeat_count >= 4 {
                continue;
            }
        } else {
            repeat_count = 0;
        }
        result.push(word);
    }

    if result.len() < words.len() {
        println!(
            "[Vision] Truncated {} repeated tokens from output",
            words.len() - result.len()
        );
    }
    result.join(" ")
}

// RESTORED AI ANALYSIS (Backend)
#[tauri::command]
fn analyze_image_with_vision(
    base64_img: &str,
    current_task: &str,
    _gpu_layers: Option<i32>,
) -> Result<String, String> {
    let chat_url = crate::llama_port::managed_chat_completions_url().ok_or_else(|| {
        "Local vision server URL unknown — start the embedded Local AI server first.".to_string()
    })?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;

    let system_msg = "You are a screenshot analysis assistant. You ALWAYS respond with a filled-in template. You NEVER refuse. You NEVER say you cannot see the image. Be accurate and concise: capture the user's primary task, not a full inventory of the UI.";

    let prompt = format!(
        r#"Study this screenshot and complete EVERY field below. Plain text only (no markdown). If the screen is very dense (spreadsheet, large table, dashboard, long doc), stay high-level — do NOT transcribe cell values, columns, or long lists.

TASK CONTEXT (may be empty): {}

Complete this template exactly:

APP: [application name, e.g. Microsoft Excel, Google Chrome, Visual Studio Code]
WINDOW TITLE: [title bar text if readable]
VISIBLE CONTENT: [1–2 short sentences: the main artifact on screen and what it is for — not every panel or control]
FILES OR URLS: [up to about five of the most relevant file names, paths, or URLs; otherwise None]
CURRENT ACTION: [what the user appears to be doing right now, one sentence]
PROGRESS: [errors, warnings, build/test status if any, or None visible]
NEXT STEP: [one short sentence: likely next action]
CATEGORY: [pick exactly ONE from: Coding, Debugging, CodeReview, Testing, Documentation, Design, Planning, Meeting, Communication, Research, Learning, DevOps, Database, Sales, Admin, Browsing, Idle, General]

CATEGORY rules: use Coding ONLY for software development (editing code, debugging in an IDE, repo/PR review in a dev tool, programming-focused terminal). Spreadsheets (Excel/Sheets), email, chat, slides, PDFs, CRM, and generic browsing are NOT Coding unless the visible work is clearly programming.]"#,
        current_task
    );

    // Retry up to 2 times on empty/refusal responses
    let max_attempts = 2;
    for attempt in 1..=max_attempts {
        let body = serde_json::json!({
            "model": LLAMA_CHAT_MODEL_ID,
            "messages": [
                {
                    "role": "system",
                    "content": system_msg
                },
                {
                    "role": "user",
                    "content": [
                        { "type": "text", "text": prompt },
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:image/png;base64,{}", base64_img)
                            }
                        }
                    ]
                }
            ],
            "temperature": 0.1,
            "top_p": 0.9,
            "max_tokens": 800,
            "repeat_penalty": 1.3,
            "frequency_penalty": 0.5,
            "presence_penalty": 0.5,
            "stream": false
        });

        let resp = client
            .post(&chat_url)
            .json(&body)
            .send()
            .map_err(|e| format!("Request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Server Error: {}", resp.status()));
        }

        let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .trim();

        // Detect empty or refusal responses
        let is_empty = content.is_empty();
        let c = content.to_lowercase();
        let is_refusal = c.contains("i'm unable to")
            || c.contains("i cannot")
            || c.contains("i can't")
            || c.contains("i am unable")
            || c.contains("unable to view")
            || c.contains("unable to analyze")
            || c.contains("can't assist")
            || c.contains("cannot assist")
            || c.contains("as an ai language model")
            || c.contains("no puedo ver")
            || c.contains("no puedo analizar");

        if is_empty || is_refusal {
            println!(
                "[Vision] Attempt {}/{}: empty or refusal response, retrying...",
                attempt, max_attempts
            );
            if attempt < max_attempts {
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
            return Err(if is_empty {
                "Model returned empty response after retries".to_string()
            } else {
                "Model refused or could not analyze the screenshot after retries".to_string()
            });
        }

        let content = truncate_repetition(content);
        return Ok(content);
    }

    Err("Model analysis failed after retries".to_string())
}

#[cfg(test)]
mod agent_struct_tests {
    use super::*;

    #[test]
    fn legacy_reports_schema_migrates_without_claiming_historical_rows() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("agent.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE config (key TEXT PRIMARY KEY, value TEXT);
             CREATE TABLE reports (
                id INTEGER PRIMARY KEY,
                description TEXT,
                activity_type TEXT,
                synced INTEGER DEFAULT 0,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
             );
             INSERT INTO reports (description, activity_type)
                VALUES ('historical', 'Coding');",
        )
        .unwrap();
        drop(conn);

        let agent = FlowmatesAgent {
            config: AgentConfig::default(),
            is_running: false,
            reports_sent: 0,
            db_path: db_path.clone(),
        };
        agent.init_db().unwrap();

        let conn = open_agent_database(&db_path).unwrap();
        assert!(table_has_column(&conn, "reports", "owner_user_id").unwrap());
        assert!(table_has_column(&conn, "reports", "duration_seconds").unwrap());
        assert!(table_has_column(&conn, "reports", "owner_team_id").unwrap());
        let owner: Option<String> = conn
            .query_row(
                "SELECT owner_user_id FROM reports WHERE description = 'historical'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owner, None);
        let team: Option<String> = conn
            .query_row(
                "SELECT owner_team_id FROM reports WHERE description = 'historical'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(team, None);
        let version: i32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, REPORTS_SCHEMA_VERSION);
        let owner_index: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_reports_owner_synced_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owner_index, 1);
        let team_index: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_reports_owner_team_synced'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(team_index, 1);
        let mode: u32 = std::fs::metadata(&db_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn recent_activity_scope_never_mixes_accounts_or_unowned_rows() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("agent.db");
        let agent = FlowmatesAgent {
            config: AgentConfig::default(),
            is_running: false,
            reports_sent: 0,
            db_path: db_path.clone(),
        };
        agent.init_db().unwrap();
        let conn = open_agent_database(&db_path).unwrap();
        for (description, owner) in [
            ("account-a", Some("user-a")),
            ("account-b", Some("user-b")),
            ("local-only", None),
        ] {
            conn.execute(
                "INSERT INTO reports (description, activity_type, owner_user_id)
                 VALUES (?1, 'Coding', ?2)",
                params![description, owner],
            )
            .unwrap();
        }

        let account_a =
            query_recent_reports(&conn, 20, &ReportOwnerScope::User("user-a".to_string())).unwrap();
        assert_eq!(account_a.len(), 1);
        assert_eq!(account_a[0].description, "account-a");

        let account_b =
            query_recent_reports(&conn, 20, &ReportOwnerScope::User("user-b".to_string())).unwrap();
        assert_eq!(account_b.len(), 1);
        assert_eq!(account_b[0].description, "account-b");

        let local = query_recent_reports(&conn, 20, &ReportOwnerScope::LocalUnowned).unwrap();
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].description, "local-only");
    }

    #[test]
    fn managed_server_lifecycle_has_cancellable_start_transition() {
        stop_managed_server().unwrap();
        let generation = claim_server_start().unwrap();
        {
            let lifecycle = lock_server_lifecycle();
            assert_eq!(lifecycle.phase, ManagedServerPhase::Starting);
            assert_eq!(lifecycle.generation, generation);
        }
        finish_start_failure(generation);
        assert_eq!(lock_server_lifecycle().phase, ManagedServerPhase::Stopped);
        stop_managed_server().unwrap();
    }

    #[test]
    fn unicode_log_tail_is_character_bounded() {
        let text = "prefix écran 🧠 漢字 fin";
        let tail = last_chars(text, 6);
        assert_eq!(tail, "漢字 fin");
        assert_eq!(tail.chars().count(), 6);
        assert_eq!(last_chars(text, 0), "");
    }

    #[test]
    fn capture_lease_rejects_overlapping_backend_capture() {
        CAPTURE_IN_PROGRESS.store(false, Ordering::Release);
        let first = CaptureLease::acquire().unwrap();
        assert!(CaptureLease::acquire().is_err());
        drop(first);
        assert!(CaptureLease::acquire().is_ok());
    }

    #[test]
    fn agent_config_json_roundtrip_negative_one_auto_marker() {
        let c = AgentConfig {
            capture_interval: Some(42_000),
            vision_model: Some("model-id".into()),
            gpu_layers: Some(-1),
            daily_goal_hours: Some(6.0),
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.gpu_layers, Some(-1));
    }

    #[test]
    fn agent_config_json_roundtrip() {
        let c = AgentConfig {
            capture_interval: Some(42_000),
            vision_model: Some("model-id".into()),
            gpu_layers: Some(4),
            daily_goal_hours: Some(8.0),
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.capture_interval, c.capture_interval);
        assert_eq!(back.gpu_layers, c.gpu_layers);
    }

    #[test]
    fn activity_report_serializes() {
        let r = ActivityReport {
            id: Some(1),
            timestamp: "t".into(),
            description: "d".into(),
            activity_type: "coding".into(),
            synced: false,
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["activity_type"], "coding");
    }
}

#[cfg(test)]
mod repetition_tests {
    use super::truncate_repetition;

    #[test]
    fn truncate_short_text_noop() {
        let s = "a b c d e f g h i";
        assert_eq!(truncate_repetition(s), s);
    }

    #[test]
    fn truncate_collapses_many_repeated_words() {
        let spam = "spam ".repeat(25);
        let out = truncate_repetition(spam.trim());
        assert!(out.len() < spam.len());
    }
}
