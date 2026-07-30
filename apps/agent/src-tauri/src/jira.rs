use reqwest::blocking::Client;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::error::Error;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JiraIssue {
    pub key: String,
    pub summary: String,
    pub status: String,
}

fn jira_http_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

fn current_user_id(conn: &Connection) -> Result<String, String> {
    crate::sync::get_user_session_from_conn(conn)
        .map(|session| session.user_id)
        .ok_or_else(|| "Sign in to Flowmates before using Jira".to_string())
}

fn scoped_key(conn: &Connection, key: &str) -> Result<String, String> {
    Ok(format!("{key}:{}", current_user_id(conn)?))
}

fn load_secret_with_legacy(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    let account = scoped_key(conn, key)?;
    if let Some(value) = crate::secure_store::get_secret(&account)? {
        return Ok(Some(value));
    }
    if let Some(value) = crate::secure_store::get_secret(key)? {
        crate::secure_store::set_secret(&account, &value)?;
        crate::secure_store::delete_secret(key)?;
        return Ok(Some(value));
    }
    let legacy = conn
        .query_row("SELECT value FROM config WHERE key = ?1", [key], |row| {
            row.get::<_, String>(0)
        })
        .ok();
    if let Some(ref value) = legacy {
        crate::secure_store::set_secret(&account, value)?;
        conn.execute("DELETE FROM config WHERE key = ?1", [key])
            .map_err(|e| e.to_string())?;
    }
    Ok(legacy)
}

pub(crate) fn save_tokens(access: &str, refresh: Option<&str>) -> Result<(), String> {
    let db_path = crate::paths::db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    crate::secure_store::set_secret(&scoped_key(&conn, "jira_access_token")?, access)?;
    if let Some(refresh) = refresh {
        crate::secure_store::set_secret(&scoped_key(&conn, "jira_refresh_token")?, refresh)?;
    }
    conn.execute(
        "DELETE FROM config WHERE key IN ('jira_access_token', 'jira_refresh_token', 'jira_cloud_id')",
        [],
    )
    .map_err(|e| e.to_string())?;

    let cloud_id = fetch_cloud_id(access).map_err(|e| e.to_string())?;
    let cloud_id_key = scoped_key(&conn, "jira_cloud_id")?;
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
        [&cloud_id_key, &cloud_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Refreshes the access token using the stored refresh token
/// Returns the new access token if successful
fn refresh_access_token() -> Result<String, String> {
    let db_path = crate::paths::db_path()?;
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;

    let refresh_token = load_secret_with_legacy(&conn, "jira_refresh_token")?
        .ok_or("No refresh token found. Please reconnect to Jira.")?;

    let cloud_session = crate::sync::get_user_session_from_conn(&conn)
        .ok_or("Sign in to Flowmates before refreshing Jira")?;
    let resp = jira_http_client()?
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
            "provider": "jira",
            "refresh_token": refresh_token,
        }))
        .send()
        .map_err(|e| format!("Failed to refresh token: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        log::warn!("[Jira] token refresh failed with HTTP {status}");
        return Err(format!(
            "Token refresh failed ({}). Please reconnect to Jira.",
            status
        ));
    }

    let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;

    let new_access = json["access_token"]
        .as_str()
        .ok_or("No access_token in refresh response")?
        .to_string();

    let new_refresh = json["refresh_token"].as_str().map(String::from);

    // Save the new tokens
    save_tokens(&new_access, new_refresh.as_deref())?;
    println!("[Jira] Token refreshed successfully");

    Ok(new_access)
}

/// Gets a valid access token, refreshing if necessary
/// This is the main entry point for getting a token to use in API calls
fn get_valid_token() -> Result<String, String> {
    let db_path = crate::paths::db_path()?;
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;

    let access_token =
        load_secret_with_legacy(&conn, "jira_access_token")?.ok_or("Not connected to Jira")?;

    // Quick validation: try to access a lightweight endpoint
    let http_client = jira_http_client()?;
    let test_resp = http_client
        .get("https://api.atlassian.com/oauth/token/accessible-resources")
        .bearer_auth(&access_token)
        .send();

    match test_resp {
        Ok(resp) if resp.status().as_u16() == 401 => {
            // Token expired, try to refresh
            println!("[Jira] Access token expired, attempting refresh...");
            refresh_access_token()
        }
        Ok(resp) if resp.status().is_success() => {
            // Token is still valid
            Ok(access_token)
        }
        Ok(resp) => {
            // Other error
            Err(format!("Jira API error: {}", resp.status()))
        }
        Err(e) => {
            // Network error, return current token and let caller handle it
            println!("[Jira] Network check failed: {}, using cached token", e);
            Ok(access_token)
        }
    }
}

fn fetch_cloud_id(token: &str) -> Result<String, Box<dyn Error>> {
    let client = jira_http_client().map_err(|e| -> Box<dyn Error> { e.into() })?;
    let resp = client
        .get("https://api.atlassian.com/oauth/token/accessible-resources")
        .bearer_auth(token)
        .send()?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("Jira resources request failed with HTTP {status}").into());
    }
    let json: serde_json::Value = resp.json()?;
    // Get first resource ID
    json[0]["id"]
        .as_str()
        .map(String::from)
        .ok_or("No accessible resources".into())
}

#[tauri::command]
pub fn fetch_jira_tasks() -> Result<Vec<JiraIssue>, String> {
    let db_path = crate::paths::db_path()?;
    crate::entitlements::require_feature(&db_path, "integrations")?;
    // 1. Get valid token (auto-refreshes if expired)
    let access_token = get_valid_token()?;

    let db_path = crate::paths::db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let cloud_id_key = scoped_key(&conn, "jira_cloud_id")?;
    let cloud_id: String = conn
        .query_row(
            "SELECT value FROM config WHERE key = ?1",
            [&cloud_id_key],
            |r| r.get(0),
        )
        .map_err(|_| "Jira Cloud ID not found".to_string())?;

    // 2. Fetch Issues
    let client = jira_http_client()?;
    let url = format!(
        "https://api.atlassian.com/ex/jira/{}/rest/api/3/search/jql",
        cloud_id
    );
    let jql = "statusCategory != Done ORDER BY updated DESC";

    let resp = client
        .post(&url)
        .bearer_auth(&access_token)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "jql": jql,
            "fields": ["summary", "status"],
            "maxResults": 50
        }))
        .send()
        .map_err(|e| e.to_string())?;

    println!("[Jira] Fetch Status: {}", resp.status());

    // Handle 401 with retry after refresh
    if resp.status().as_u16() == 401 {
        println!("[Jira] Got 401, attempting token refresh...");
        let new_token = refresh_access_token()?;

        // Retry with new token
        let retry_resp = client
            .post(&url)
            .bearer_auth(&new_token)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "jql": jql,
                "fields": ["summary", "status"],
                "maxResults": 50
            }))
            .send()
            .map_err(|e| e.to_string())?;

        if !retry_resp.status().is_success() {
            return Err(format!(
                "Jira API failed after refresh: {}",
                retry_resp.status()
            ));
        }

        return parse_jira_issues(retry_resp.text().map_err(|e| e.to_string())?);
    }

    if !resp.status().is_success() {
        return Err(format!("Jira API failed: {}", resp.status()));
    }

    let text_resp = resp.text().map_err(|e| e.to_string())?;
    parse_jira_issues(text_resp)
}

fn parse_jira_issues(text_resp: String) -> Result<Vec<JiraIssue>, String> {
    let json: serde_json::Value = serde_json::from_str(&text_resp).map_err(|e| e.to_string())?;

    if let Some(message) = json["errorMessages"]
        .as_array()
        .and_then(|errors| errors.first())
        .and_then(|error| error.as_str())
    {
        return Err(message.to_string());
    }

    let mut issues = Vec::new();
    if let Some(opts) = json["issues"].as_array() {
        for i in opts {
            let key = i["key"].as_str().unwrap_or_default().to_string();
            let summary = i["fields"]["summary"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let status = i["fields"]["status"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            issues.push(JiraIssue {
                key,
                summary,
                status,
            });
        }
    }
    Ok(issues)
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JiraUser {
    pub display_name: String,
    pub avatar_url: String,
    pub email: String,
}

#[tauri::command]
pub fn fetch_jira_profile() -> Result<JiraUser, String> {
    let db_path = crate::paths::db_path()?;
    crate::entitlements::require_feature(&db_path, "integrations")?;
    // 1. Get valid token (auto-refreshes if expired)
    let access_token = get_valid_token()?;

    let db_path = crate::paths::db_path()?;
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    let cloud_id_key = scoped_key(&conn, "jira_cloud_id")?;
    let cloud_id: String = conn
        .query_row(
            "SELECT value FROM config WHERE key = ?1",
            [&cloud_id_key],
            |r| r.get(0),
        )
        .map_err(|_| "Jira Cloud ID not found".to_string())?;

    // 2. Call /myself
    let client = jira_http_client()?;
    let url = format!(
        "https://api.atlassian.com/ex/jira/{}/rest/api/3/myself",
        cloud_id
    );

    let resp = client
        .get(&url)
        .bearer_auth(&access_token)
        .send()
        .map_err(|e| e.to_string())?;

    // Handle 401 with retry after refresh
    if resp.status().as_u16() == 401 {
        println!("[Jira] Profile fetch got 401, refreshing token...");
        let new_token = refresh_access_token()?;

        let retry_resp = client
            .get(&url)
            .bearer_auth(&new_token)
            .send()
            .map_err(|e| e.to_string())?;

        if !retry_resp.status().is_success() {
            return Err(format!(
                "Failed to fetch profile after refresh: {}",
                retry_resp.status()
            ));
        }

        return parse_jira_profile(retry_resp.json().map_err(|e| e.to_string())?, &conn);
    }

    if !resp.status().is_success() {
        return Err(format!("Failed to fetch profile: {}", resp.status()));
    }

    parse_jira_profile(resp.json().map_err(|e| e.to_string())?, &conn)
}

fn parse_jira_profile(json: serde_json::Value, _conn: &Connection) -> Result<JiraUser, String> {
    let user = JiraUser {
        display_name: json["displayName"]
            .as_str()
            .unwrap_or("Unknown")
            .to_string(),
        avatar_url: json["avatarUrls"]["48x48"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        email: json["emailAddress"].as_str().unwrap_or("").to_string(),
    };

    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jira_issue_roundtrip() {
        let issue = JiraIssue {
            key: "FS-1".to_string(),
            summary: "Harden OAuth".to_string(),
            status: "In Progress".to_string(),
        };
        let json = serde_json::to_string(&issue).unwrap();
        let decoded: JiraIssue = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.key, "FS-1");
    }
}
