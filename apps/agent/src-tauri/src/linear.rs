use reqwest::blocking::Client;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LinearIssue {
    pub id: String,
    pub identifier: String, // e.g., "ENG-123"
    pub title: String,
    pub state: String,
}

fn get_db_conn() -> Result<Connection, String> {
    Connection::open(crate::paths::db_path()?).map_err(|e| e.to_string())
}

fn linear_http_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

fn linear_session_account(conn: &Connection) -> Result<String, String> {
    let user_id = crate::sync::get_user_session_from_conn(conn)
        .map(|session| session.user_id)
        .ok_or("Sign in to Flowmates before using Linear")?;
    Ok(format!("linear_auth_session:{user_id}"))
}

fn get_linear_token() -> Result<String, String> {
    let conn = get_db_conn()?;
    let account = linear_session_account(&conn)?;
    let mut json = crate::secure_store::get_secret(&account)?;
    if json.is_none() {
        if let Some(legacy) = crate::secure_store::get_secret("linear_auth_session")? {
            crate::secure_store::set_secret(&account, &legacy)?;
            crate::secure_store::delete_secret("linear_auth_session")?;
            json = Some(legacy);
        }
    }
    if json.is_none() {
        json = conn
            .query_row(
                "SELECT value FROM config WHERE key = 'linear_auth_session'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok();
        if let Some(ref legacy) = json {
            crate::secure_store::set_secret(&account, legacy)?;
            let _ = conn.execute("DELETE FROM config WHERE key = 'linear_auth_session'", []);
        }
    }
    let json = json.ok_or("Not logged in with Linear")?;

    let session: serde_json::Value =
        serde_json::from_str(&json).map_err(|_| "Invalid session".to_string())?;

    if session["provider"].as_str() != Some("linear") {
        return Err("Not logged in with Linear".to_string());
    }

    session["access_token"]
        .as_str()
        .map(String::from)
        .ok_or("No access token".to_string())
}

fn refresh_linear_token() -> Result<String, String> {
    let conn = get_db_conn()?;
    let account = linear_session_account(&conn)?;
    let session_json = crate::secure_store::get_secret(&account)?
        .ok_or("Reconnect Linear to refresh its session")?;
    let mut integration_session: serde_json::Value =
        serde_json::from_str(&session_json).map_err(|_| "Invalid Linear session".to_string())?;
    let refresh_token = integration_session["refresh_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or("Reconnect Linear to refresh its session")?;
    let cloud_session = crate::sync::get_user_session_from_conn(&conn)
        .ok_or("Sign in to Flowmates before refreshing Linear")?;
    let response = linear_http_client()?
        .post(format!(
            "{}/functions/v1/oauth-exchange",
            crate::sync_env::supabase_url()
        ))
        .header("apikey", crate::sync_env::supabase_anon_key())
        .header(
            "Authorization",
            format!("Bearer {}", cloud_session.access_token),
        )
        .json(&serde_json::json!({
            "provider": "linear",
            "refresh_token": refresh_token,
        }))
        .send()
        .map_err(|e| format!("Failed to refresh Linear: {e}"))?;
    let status = response.status();
    let payload: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(payload["error"]
            .as_str()
            .unwrap_or("Linear token refresh failed")
            .to_string());
    }
    let access_token = payload["access_token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or("Linear refresh returned no access token")?
        .to_string();
    integration_session["access_token"] = serde_json::Value::String(access_token.clone());
    if let Some(new_refresh) = payload["refresh_token"].as_str() {
        integration_session["refresh_token"] = serde_json::Value::String(new_refresh.to_string());
    }
    crate::secure_store::set_secret(&account, &integration_session.to_string())?;
    Ok(access_token)
}

#[tauri::command]
pub fn fetch_linear_tasks() -> Result<Vec<LinearIssue>, String> {
    let db_path = crate::paths::db_path()?;
    crate::entitlements::require_feature(&db_path, "integrations")?;
    let access_token = get_linear_token()?;

    let client = linear_http_client()?;

    // GraphQL query to get assigned issues
    let query = r#"{
        "query": "query { viewer { assignedIssues(first: 50, filter: { state: { type: { nin: [\"completed\", \"canceled\"] } } }) { nodes { id identifier title state { name } } } } }"
    }"#;

    let send = |token: &str| {
        client
            .post("https://api.linear.app/graphql")
            .bearer_auth(token)
            .header("Content-Type", "application/json")
            .body(query)
            .send()
            .map_err(|e| format!("Linear API error: {e}"))
    };
    let mut resp = send(&access_token)?;
    if resp.status().as_u16() == 401 {
        resp = send(&refresh_linear_token()?)?;
    }

    if !resp.status().is_success() {
        return Err(format!("Linear API failed: {}", resp.status()));
    }

    let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    if let Some(errors) = json["errors"].as_array() {
        let message = errors
            .first()
            .and_then(|error| error["message"].as_str())
            .unwrap_or("Linear GraphQL request failed");
        return Err(message.to_string());
    }

    let mut issues = Vec::new();

    if let Some(nodes) = json["data"]["viewer"]["assignedIssues"]["nodes"].as_array() {
        for node in nodes {
            issues.push(LinearIssue {
                id: node["id"].as_str().unwrap_or_default().to_string(),
                identifier: node["identifier"].as_str().unwrap_or_default().to_string(),
                title: node["title"].as_str().unwrap_or_default().to_string(),
                state: node["state"]["name"]
                    .as_str()
                    .unwrap_or("Unknown")
                    .to_string(),
            });
        }
    }

    println!("[Linear] Fetched {} issues", issues.len());
    Ok(issues)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LinearUser {
    pub id: String,
    pub name: String,
    pub email: String,
    pub avatar_url: Option<String>,
}

#[tauri::command]
pub fn fetch_linear_profile() -> Result<LinearUser, String> {
    let db_path = crate::paths::db_path()?;
    crate::entitlements::require_feature(&db_path, "integrations")?;
    let access_token = get_linear_token()?;

    let client = linear_http_client()?;

    let query = r#"{"query": "{ viewer { id name email avatarUrl } }"}"#;

    let send = |token: &str| {
        client
            .post("https://api.linear.app/graphql")
            .bearer_auth(token)
            .header("Content-Type", "application/json")
            .body(query)
            .send()
            .map_err(|e| format!("Linear API error: {e}"))
    };
    let mut resp = send(&access_token)?;
    if resp.status().as_u16() == 401 {
        resp = send(&refresh_linear_token()?)?;
    }

    if !resp.status().is_success() {
        return Err(format!("Linear API failed: {}", resp.status()));
    }

    let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    if let Some(errors) = json["errors"].as_array() {
        let message = errors
            .first()
            .and_then(|error| error["message"].as_str())
            .unwrap_or("Linear GraphQL request failed");
        return Err(message.to_string());
    }
    let viewer = &json["data"]["viewer"];

    if viewer["id"].as_str().is_none() {
        return Err("Linear returned no user profile".to_string());
    }

    Ok(LinearUser {
        id: viewer["id"].as_str().unwrap_or_default().to_string(),
        name: viewer["name"].as_str().unwrap_or_default().to_string(),
        email: viewer["email"].as_str().unwrap_or_default().to_string(),
        avatar_url: viewer["avatarUrl"].as_str().map(String::from),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_issue_roundtrip() {
        let i = LinearIssue {
            id: "u".into(),
            identifier: "ENG-1".into(),
            title: "t".into(),
            state: "Done".into(),
        };
        let j = serde_json::to_string(&i).unwrap();
        let back: LinearIssue = serde_json::from_str(&j).unwrap();
        assert_eq!(back.identifier, "ENG-1");
    }
}
