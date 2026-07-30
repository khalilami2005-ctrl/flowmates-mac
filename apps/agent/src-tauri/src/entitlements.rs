use crate::sync::get_user_session_from_conn;
use crate::sync_env::{supabase_anon_key, supabase_url};
use reqwest::blocking::Client;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

fn cloud_http_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

pub(crate) fn minimize_local_report_for_cloud(mut report: serde_json::Value) -> serde_json::Value {
    if let Some(object) = report.as_object_mut() {
        object.remove("sample_activities");
        object.remove("longest_sessions");
        object.remove("work_themes");
    }
    report
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Entitlements {
    pub plan: Option<String>,
    pub status: String,
    pub team_ids: Vec<String>,
    pub active_team_id: Option<String>,
    pub can_sync: bool,
    pub can_cloud_ai: bool,
    pub can_integrations: bool,
}

impl Entitlements {
    pub fn free() -> Self {
        Self {
            plan: None,
            status: "free".to_string(),
            team_ids: vec![],
            active_team_id: None,
            can_sync: false,
            can_cloud_ai: false,
            can_integrations: false,
        }
    }

    #[allow(dead_code)]
    pub fn is_paid(&self) -> bool {
        self.can_sync || self.can_cloud_ai || self.can_integrations
    }
}

fn parse_entitlements_json(value: &serde_json::Value) -> Entitlements {
    let features = &value["features"];
    let team_ids: Vec<String> = value["team_ids"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Entitlements {
        plan: value["plan"].as_str().map(String::from),
        status: value["status"].as_str().unwrap_or("free").to_string(),
        team_ids: team_ids.clone(),
        active_team_id: team_ids.first().cloned(),
        can_sync: features["sync"].as_bool().unwrap_or(false),
        can_cloud_ai: features["cloud_ai"].as_bool().unwrap_or(false),
        can_integrations: features["integrations"].as_bool().unwrap_or(false),
    }
}

fn apply_active_team(
    mut entitlements: Entitlements,
    session: &crate::sync::UserSession,
) -> Entitlements {
    if let Some(team_id) = session
        .team_id
        .as_ref()
        .filter(|team_id| entitlements.team_ids.iter().any(|id| id == *team_id))
    {
        entitlements.active_team_id = Some(team_id.clone());
    }
    entitlements
}

pub fn load_entitlements(conn: &Connection) -> Entitlements {
    conn.query_row(
        "SELECT value FROM config WHERE key = 'entitlements'",
        [],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .and_then(|json| serde_json::from_str::<Entitlements>(&json).ok())
    .unwrap_or_else(Entitlements::free)
}

pub fn save_entitlements(conn: &Connection, entitlements: &Entitlements) -> Result<(), String> {
    let json = serde_json::to_string(entitlements).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value) VALUES ('entitlements', ?1)",
        [&json],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn clear_entitlements(conn: &Connection) -> Result<(), String> {
    conn.execute("DELETE FROM config WHERE key = 'entitlements'", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn refresh_entitlements_from_supabase(
    access_token: &str,
    team_id: Option<&str>,
) -> Result<Entitlements, String> {
    let client = cloud_http_client()?;
    let url = format!("{}/rest/v1/rpc/get_user_entitlements", supabase_url());

    let payload = if let Some(tid) = team_id {
        serde_json::json!({ "p_team_id": tid })
    } else {
        serde_json::json!({})
    };

    let resp = client
        .post(&url)
        .header("apikey", supabase_anon_key())
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(format!("Failed to fetch entitlements: {}", body));
    }

    let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    Ok(parse_entitlements_json(&json))
}

pub fn require_feature(db_path: &std::path::Path, feature: &str) -> Result<(), String> {
    crate::sync::refresh_session_if_expiring(&db_path.to_path_buf());
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let session = get_user_session_from_conn(&conn).ok_or("Sign in to use this cloud feature")?;
    let team_id = session.team_id.as_deref();
    let entitlements = apply_active_team(
        refresh_entitlements_from_supabase(&session.access_token, team_id)?,
        &session,
    );
    save_entitlements(&conn, &entitlements)?;
    let allowed = match feature {
        "sync" => entitlements.can_sync,
        "cloud_ai" => entitlements.can_cloud_ai,
        "integrations" => entitlements.can_integrations,
        _ => false,
    };

    if allowed {
        Ok(())
    } else {
        Err(
            "This feature requires an Individual or Team license. Activate cloud features in Profile."
                .to_string(),
        )
    }
}

#[tauri::command]
pub fn get_entitlements() -> Result<Entitlements, String> {
    let db_path = crate::paths::db_path()?;
    crate::sync::refresh_session_if_expiring(&db_path);
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    if let Some(session) = get_user_session_from_conn(&conn) {
        let team_id = session.team_id.as_deref();
        if let Ok(entitlements) = refresh_entitlements_from_supabase(&session.access_token, team_id)
        {
            let entitlements = apply_active_team(entitlements, &session);
            save_entitlements(&conn, &entitlements)?;
            return Ok(entitlements);
        }
    }
    Ok(load_entitlements(&conn))
}

#[tauri::command]
pub fn refresh_entitlements() -> Result<Entitlements, String> {
    crate::sync_env::require_cloud()?;
    let db_path = crate::paths::db_path()?;
    crate::sync::refresh_session_if_expiring(&db_path);
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;

    let session =
        get_user_session_from_conn(&conn).ok_or("Not logged in — cannot refresh entitlements")?;
    let team_id = session.team_id.as_deref();

    let entitlements = apply_active_team(
        refresh_entitlements_from_supabase(&session.access_token, team_id)?,
        &session,
    );
    save_entitlements(&conn, &entitlements)?;
    Ok(entitlements)
}

fn call_authenticated_rpc(
    rpc_name: &str,
    payload: &serde_json::Value,
) -> Result<(crate::sync::UserSession, serde_json::Value), String> {
    let db_path = crate::paths::db_path()?;
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    let mut session = get_user_session_from_conn(&conn).ok_or("Not logged in")?;
    let url = format!("{}/rest/v1/rpc/{}", supabase_url(), rpc_name);
    let client = cloud_http_client()?;
    let send = |access_token: &str| {
        client
            .post(&url)
            .header("apikey", supabase_anon_key())
            .header("Authorization", format!("Bearer {access_token}"))
            .json(payload)
            .send()
            .map_err(|e| e.to_string())
    };
    let mut response = send(&session.access_token)?;
    if matches!(response.status().as_u16(), 401 | 403) {
        session = crate::sync::refresh_supabase_token(&session)?;
        response = send(&session.access_token)?;
    }
    let status = response.status();
    let body: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body["message"]
            .as_str()
            .or_else(|| body["error"].as_str())
            .unwrap_or("Cloud request failed")
            .to_string());
    }
    Ok((session, body))
}

#[tauri::command]
pub fn ensure_personal_team() -> Result<Option<String>, String> {
    let (session, body) = call_authenticated_rpc("ensure_personal_team", &serde_json::json!({}))?;
    let team_id = body["team_id"]
        .as_str()
        .or_else(|| body.as_str())
        .map(String::from);
    if let Some(ref id) = team_id {
        crate::sync::save_user_session(
            session.user_id,
            Some(id.clone()),
            session.access_token,
            session.refresh_token,
            session.email,
        )?;
    }
    Ok(team_id)
}

#[tauri::command]
pub fn claim_license_code(code: String) -> Result<Entitlements, String> {
    let code = code.trim().to_ascii_uppercase();
    if code.len() < 8
        || code.len() > 64
        || !code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("Invalid license code".to_string());
    }
    let (session, _) =
        call_authenticated_rpc("claim_license", &serde_json::json!({ "p_code": code }))?;
    let team_id = session.team_id.as_deref();
    let mut entitlements = refresh_entitlements_from_supabase(&session.access_token, team_id)?;
    if matches!(
        entitlements.plan.as_deref(),
        Some("individual" | "individual_pro")
    ) && entitlements.team_ids.is_empty()
    {
        let _ = ensure_personal_team()?;
        let team_id = {
            let db_path = crate::paths::db_path()?;
            let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
            crate::sync::get_user_session_from_conn(&conn).and_then(|s| s.team_id)
        };
        entitlements =
            refresh_entitlements_from_supabase(&session.access_token, team_id.as_deref())?;
    }
    Ok(entitlements)
}

#[tauri::command]
pub fn fetch_cloud_insights(limit: Option<u32>) -> Result<Vec<serde_json::Value>, String> {
    let db_path = crate::paths::db_path()?;
    require_feature(&db_path, "cloud_ai")?;

    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    let session = get_user_session_from_conn(&conn).ok_or("Not logged in")?;

    let team_filter = session
        .team_id
        .as_ref()
        .map(|team_id| format!("&team_id=eq.{}", urlencoding::encode(team_id)))
        .unwrap_or_default();

    let max_rows = limit.unwrap_or(10).min(50);
    let url = format!(
        "{}/rest/v1/cloud_insights?select=*&user_id=eq.{}&order=created_at.desc&limit={}{}",
        supabase_url(),
        urlencoding::encode(&session.user_id),
        max_rows,
        team_filter
    );

    let client = cloud_http_client()?;
    let resp = client
        .get(&url)
        .header("apikey", supabase_anon_key())
        .header("Authorization", format!("Bearer {}", session.access_token))
        .send()
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(format!("Failed to fetch cloud insights: {}", body));
    }

    resp.json::<Vec<serde_json::Value>>()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn request_cloud_insights(
    period_days: Option<i32>,
    team_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let db_path = crate::paths::db_path()?;
    require_feature(&db_path, "cloud_ai")?;

    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    let session = get_user_session_from_conn(&conn).ok_or("Not logged in")?;
    let entitlements = load_entitlements(&conn);

    let days = period_days.unwrap_or(7).clamp(1, 30);
    let resolved_team_id = team_id.or(session.team_id.clone());
    if let Some(ref requested_team) = resolved_team_id {
        if !entitlements.team_ids.iter().any(|id| id == requested_team) {
            return Err("You are not entitled to access that team".to_string());
        }
    }

    let mut body = serde_json::json!({
        "period_days": days,
        "team_id": resolved_team_id,
        "plan": entitlements.plan,
    });

    if matches!(
        entitlements.plan.as_deref(),
        Some("individual" | "individual_pro")
    ) {
        let local_report = minimize_local_report_for_cloud(
            crate::insights_local::build_user_insights_report(&db_path, days, &session.user_id)?,
        );
        body["local_report"] = local_report;
    }

    let client = cloud_http_client()?;
    let url = format!("{}/functions/v1/generate-insights", supabase_url());
    let resp = client
        .post(&url)
        .header("apikey", supabase_anon_key())
        .header("Authorization", format!("Bearer {}", session.access_token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(format!("Failed to generate cloud insights: {}", body));
    }

    resp.json::<serde_json::Value>().map_err(|e| e.to_string())
}
