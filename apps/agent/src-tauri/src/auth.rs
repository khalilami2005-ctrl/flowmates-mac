use crate::sync_env::{supabase_anon_key, supabase_url};
use base64::Engine;
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, CsrfToken, PkceCodeChallenge, RedirectUrl, Scope,
    TokenUrl,
};
use reqwest::blocking::Client;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tiny_http::{Method, Response, Server};
use url::Url;

static OAUTH_STATE: OnceLock<Mutex<OAuthState>> = OnceLock::new();
static OAUTH_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static OAUTH_EPOCH: AtomicU64 = AtomicU64::new(0);

fn auth_log(message: impl AsRef<str>) {
    let message = message.as_ref();
    let line = format!(
        "[{}] {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        message
    );

    log::info!("{}", message);

    if let Ok(path) = crate::paths::auth_log_path() {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

fn panic_payload_to_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic>".to_string()
    }
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn supabase_login_error_html(error: &str, description: &str) -> String {
    let error = html_escape(error);
    let description = html_escape(description);
    let auth_log_hint = crate::paths::auth_log_path()
        .map(|p| html_escape(&p.to_string_lossy()))
        .unwrap_or_else(|_| html_escape("auth.log under Flowmates's local app data folder"));
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Flowmates - Login Failed</title>
  <style>
    body {{
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Arial, sans-serif;
      background: #0a0a0a;
      color: #fafafa;
      min-height: 100vh;
      display: flex;
      align-items: center;
      justify-content: center;
      margin: 0;
    }}
    .container {{
      max-width: 520px;
      padding: 32px;
      text-align: center;
    }}
    .error {{
      color: #ef4444;
      font-weight: 700;
      margin-bottom: 12px;
    }}
    .details {{
      background: rgba(255,255,255,0.06);
      border: 1px solid rgba(255,255,255,0.12);
      border-radius: 12px;
      padding: 16px;
      text-align: left;
      color: #d4d4d8;
      word-break: break-word;
      font-size: 13px;
      line-height: 1.5;
    }}
    .hint {{
      color: #a1a1aa;
      font-size: 13px;
      margin-top: 18px;
    }}
  </style>
</head>
<body>
  <div class="container">
    <h1 class="error">Login failed</h1>
    <div class="details">
      <strong>Supabase error:</strong> {error}<br>
      <strong>Description:</strong> {description}
    </div>
    <p class="hint">Copy this error and check <code>{auth_log_hint}</code> for the full OAuth trace.</p>
  </div>
</body>
</html>"#
    )
}

#[derive(Clone, Default)]
struct OAuthState {
    verifier: Option<String>,
    provider: Option<String>,
    csrf_state: Option<String>,
    epoch: u64,
}

fn set_oauth_state(verifier: String, provider: String, csrf_state: String, epoch: u64) {
    let mutex = OAUTH_STATE.get_or_init(|| Mutex::new(OAuthState::default()));
    match mutex.lock() {
        Ok(mut lock) => {
            lock.verifier = Some(verifier);
            lock.provider = Some(provider);
            lock.csrf_state = Some(csrf_state);
            lock.epoch = epoch;
        }
        Err(poisoned) => {
            let mut lock = poisoned.into_inner();
            lock.verifier = Some(verifier);
            lock.provider = Some(provider);
            lock.csrf_state = Some(csrf_state);
            lock.epoch = epoch;
            auth_log("[Auth] OAuth state mutex was poisoned; recovered.");
        }
    }
}

fn clear_oauth_state() {
    if let Some(mutex) = OAUTH_STATE.get() {
        if let Ok(mut lock) = mutex.lock() {
            lock.verifier = None;
            lock.provider = None;
            lock.csrf_state = None;
            lock.epoch = 0;
        }
    }
}

fn get_oauth_state() -> OAuthState {
    let mutex = OAUTH_STATE.get_or_init(|| Mutex::new(OAuthState::default()));
    match mutex.lock() {
        Ok(lock) => lock.clone(),
        Err(poisoned) => {
            let lock = poisoned.into_inner();
            println!("[Auth] OAuth state mutex was poisoned; recovered.");
            lock.clone()
        }
    }
}

fn take_oauth_state() -> OAuthState {
    let mutex = OAUTH_STATE.get_or_init(|| Mutex::new(OAuthState::default()));
    match mutex.lock() {
        Ok(mut lock) => std::mem::take(&mut *lock),
        Err(poisoned) => {
            let mut lock = poisoned.into_inner();
            std::mem::take(&mut *lock)
        }
    }
}

fn csrf_matches(received: Option<&String>, expected: Option<&String>) -> bool {
    matches!((received, expected), (Some(received), Some(expected)) if received == expected)
}

// Provider configs
#[derive(Clone)]
struct ProviderConfig {
    /// Provider id (reserved for logging / future use).
    #[allow(dead_code)]
    name: &'static str,
    auth_url: &'static str,
    token_url: &'static str,
    scopes: &'static [&'static str],
    userinfo_url: &'static str,
}

const GOOGLE: ProviderConfig = ProviderConfig {
    name: "google",
    auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
    token_url: "https://oauth2.googleapis.com/token",
    scopes: &["openid", "email", "profile"],
    userinfo_url: "https://www.googleapis.com/oauth2/v3/userinfo",
};

const JIRA: ProviderConfig = ProviderConfig {
    name: "jira",
    auth_url: "https://auth.atlassian.com/authorize",
    token_url: "https://auth.atlassian.com/oauth/token",
    scopes: &[
        "read:jira-work",
        "read:jira-user",
        "offline_access",
        "read:me",
    ],
    userinfo_url: "https://api.atlassian.com/me",
};

const LINEAR: ProviderConfig = ProviderConfig {
    name: "linear",
    auth_url: "https://linear.app/oauth/authorize",
    token_url: "https://api.linear.app/oauth/token",
    scopes: &["read", "issues:create"],
    userinfo_url: "https://api.linear.app/graphql",
};

const REDIRECT_URL: &str = "http://localhost:12345/callback";

const SUPABASE_SUCCESS_HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Flowmates - Login Successful</title>
  <style>
    * { margin: 0; padding: 0; box-sizing: border-box; }
    body {
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
      background: linear-gradient(135deg, #0a0a0a 0%, #1a1a2e 50%, #16213e 100%);
      min-height: 100vh;
      display: flex;
      align-items: center;
      justify-content: center;
      color: #fafafa;
    }
    .container {
      text-align: center;
      padding: 48px;
      max-width: 420px;
    }
    .logo-img {
      display: block;
      margin: 0 auto 20px;
      max-width: min(240px, 80vw);
      height: auto;
    }
    .check-icon {
      width: 80px;
      height: 80px;
      margin: 24px auto;
      background: linear-gradient(135deg, #22c55e, #16a34a);
      border-radius: 50%;
      display: flex;
      align-items: center;
      justify-content: center;
      box-shadow: 0 0 40px rgba(34, 197, 94, 0.3);
      animation: pulse 2s ease-in-out infinite;
    }
    .check-icon svg {
      width: 40px;
      height: 40px;
      stroke: white;
      stroke-width: 3;
      fill: none;
    }
    @keyframes pulse {
      0%, 100% { transform: scale(1); box-shadow: 0 0 40px rgba(34, 197, 94, 0.3); }
      50% { transform: scale(1.05); box-shadow: 0 0 60px rgba(34, 197, 94, 0.5); }
    }
    h1 {
      font-size: 24px;
      font-weight: 600;
      margin-bottom: 12px;
    }
    p {
      color: #a1a1aa;
      font-size: 14px;
      line-height: 1.6;
    }
    .hint {
      margin-top: 32px;
      padding: 16px 24px;
      background: rgba(255, 255, 255, 0.05);
      border: 1px solid rgba(255, 255, 255, 0.1);
      border-radius: 12px;
      font-size: 13px;
      color: #71717a;
    }
    .close-btn {
      margin-top: 24px;
      padding: 12px 32px;
      background: linear-gradient(135deg, #3b82f6, #8b5cf6);
      border: none;
      border-radius: 8px;
      color: white;
      font-size: 14px;
      font-weight: 500;
      cursor: pointer;
      transition: all 0.2s;
    }
    .close-btn:hover {
      transform: translateY(-2px);
      box-shadow: 0 8px 24px rgba(59, 130, 246, 0.4);
    }
  </style>
</head>
<body>
  <div class="container">
    <img class="logo-img" src="__FLOW_LOGO_DATA_URI__" alt="Flowmates" />
    <div class="check-icon">
      <svg viewBox="0 0 24 24"><polyline points="20 6 9 17 4 12"></polyline></svg>
    </div>
    <h1>Login Successful!</h1>
    <p>Your account has been connected successfully.</p>
    <div class="hint">You can now close this tab and return to the Flowmates app.</div>
    <button class="close-btn" onclick="window.close()">Close Tab</button>
  </div>
</body>
</html>"#;

fn supabase_login_success_html() -> String {
    static HTML: OnceLock<String> = OnceLock::new();
    HTML.get_or_init(|| {
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/flowmates-mark.png"
        ));
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let data_uri = format!("data:image/png;base64,{}", b64);
        SUPABASE_SUCCESS_HTML_TEMPLATE.replace("__FLOW_LOGO_DATA_URI__", &data_uri)
    })
    .clone()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub provider: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthSession {
    pub user: AuthUser,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub provider: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthSessionView {
    pub user: AuthUser,
    pub provider: String,
}

impl From<AuthSession> for AuthSessionView {
    fn from(session: AuthSession) -> Self {
        Self {
            user: session.user,
            provider: session.provider,
        }
    }
}

fn get_env_var(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

fn auth_http_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))
}

fn get_provider_client_id(provider: &str) -> String {
    match provider {
        "google" => get_env_var("VITE_GOOGLE_CLIENT_ID").unwrap_or_default(),
        "jira" => crate::oauth_env::jira_client_id(),
        "linear" => crate::oauth_env::linear_client_id(),
        _ => String::new(),
    }
}

fn get_provider_config(provider: &str) -> Option<ProviderConfig> {
    match provider {
        "google" => Some(GOOGLE),
        "jira" => Some(JIRA),
        "linear" => Some(LINEAR),
        _ => None,
    }
}

fn create_oauth_client(provider: &str) -> Result<BasicClient, String> {
    let config = get_provider_config(provider).ok_or("Unknown provider")?;
    let client_id = get_provider_client_id(provider);

    if client_id.is_empty() {
        return Err(format!("Missing client ID for {}", provider));
    }

    let mut client = BasicClient::new(
        ClientId::new(client_id),
        None,
        AuthUrl::new(config.auth_url.to_string()).map_err(|e| e.to_string())?,
        Some(TokenUrl::new(config.token_url.to_string()).map_err(|e| e.to_string())?),
    );

    client = client
        .set_redirect_uri(RedirectUrl::new(REDIRECT_URL.to_string()).map_err(|e| e.to_string())?);

    Ok(client)
}

#[tauri::command]
pub fn start_auth(provider: String) -> Result<String, String> {
    auth_log(format!(
        "[Auth] start_auth requested for provider: {}",
        provider
    ));

    let db_path = crate::paths::db_path()?;

    // Google uses Supabase OAuth (configured in Supabase Dashboard)
    if provider == "google" {
        return start_supabase_oauth(&provider);
    }

    // Jira and Linear are paid-plan integrations only.
    crate::entitlements::require_feature(&db_path, "integrations")?;

    // Jira and Linear use direct OAuth with .env keys
    let config = get_provider_config(&provider).ok_or("Unknown provider")?;
    let client = create_oauth_client(&provider)?;
    if OAUTH_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        return Err("Another OAuth connection is already in progress".to_string());
    }

    // Increment epoch so old listeners can detect they are stale
    let epoch = OAUTH_EPOCH.fetch_add(1, Ordering::SeqCst).wrapping_add(1);

    // Bind port BEFORE opening the browser — no other process can preempt it
    let server = Server::http("127.0.0.1:12345").map_err(|e| {
        OAUTH_IN_PROGRESS.store(false, Ordering::SeqCst);
        format!("Could not bind callback port 12345: {e}")
    })?;

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let mut auth_request = client
        .authorize_url(CsrfToken::new_random)
        .set_pkce_challenge(pkce_challenge);

    for scope in config.scopes {
        auth_request = auth_request.add_scope(Scope::new(scope.to_string()));
    }

    let (mut auth_url, csrf) = auth_request.url();
    set_oauth_state(
        pkce_verifier.secret().to_string(),
        provider.clone(),
        csrf.secret().to_string(),
        epoch,
    );
    if provider == "jira" {
        auth_url
            .query_pairs_mut()
            .append_pair("audience", "api.atlassian.com");
    }

    auth_log(format!(
        "[Auth] Opening direct OAuth URL for provider: {}",
        provider
    ));

    if let Err(e) = open::that(auth_url.as_str()) {
        OAUTH_IN_PROGRESS.store(false, Ordering::SeqCst);
        return Err(e.to_string());
    }

    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            listen_for_callback(server, epoch);
        }));
        if let Err(payload) = result {
            auth_log(format!(
                "[Auth] Direct OAuth callback listener panicked: {}",
                panic_payload_to_string(payload)
            ));
        }
        // Only release the lock if we are still the current epoch
        if OAUTH_EPOCH.load(Ordering::SeqCst) == epoch {
            OAUTH_IN_PROGRESS.store(false, Ordering::SeqCst);
        }
    });

    Ok(format!("Browser opened for {} login", provider))
}

fn start_supabase_oauth(provider: &str) -> Result<String, String> {
    if OAUTH_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        return Err("Another OAuth connection is already in progress".to_string());
    }

    let epoch = OAUTH_EPOCH.fetch_add(1, Ordering::SeqCst).wrapping_add(1);

    // Bind port BEFORE opening the browser
    let server = Server::http("127.0.0.1:12345").map_err(|e| {
        OAUTH_IN_PROGRESS.store(false, Ordering::SeqCst);
        format!("Could not bind callback port 12345: {e}")
    })?;

    let csrf = CsrfToken::new_random().secret().to_string();
    set_oauth_state(
        "supabase".to_string(),
        provider.to_string(),
        csrf.clone(),
        epoch,
    );
    auth_log(format!(
        "[Auth] Starting Supabase OAuth for provider: {}",
        provider
    ));

    let redirect_to = format!(
        "http://localhost:12345/callback?state={}",
        urlencoding::encode(&csrf)
    );
    let auth_url = format!(
        "{}/auth/v1/authorize?provider={}&redirect_to={}",
        supabase_url(),
        provider,
        urlencoding::encode(&redirect_to)
    );

    auth_log(format!(
        "[Auth] Opening Supabase OAuth URL for provider: {}",
        provider
    ));

    if let Err(e) = open::that(&auth_url) {
        OAUTH_IN_PROGRESS.store(false, Ordering::SeqCst);
        auth_log(format!("[Auth] Failed to open browser: {}", e));
        return Err(format!("Failed to open browser: {}", e));
    }

    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            listen_for_supabase_callback(server, epoch);
        }));
        if let Err(payload) = result {
            auth_log(format!(
                "[Auth] Supabase callback listener panicked: {}",
                panic_payload_to_string(payload)
            ));
        }
        if OAUTH_EPOCH.load(Ordering::SeqCst) == epoch {
            OAUTH_IN_PROGRESS.store(false, Ordering::SeqCst);
        }
    });

    Ok(format!(
        "Browser opened for {} login via Supabase",
        provider
    ))
}

fn listen_for_callback(server: Server, epoch: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);

    auth_log("[Auth] Direct listener started with bound server");

    let poll_interval = std::time::Duration::from_millis(250);

    loop {
        if OAUTH_EPOCH.load(Ordering::SeqCst) != epoch {
            auth_log("[Auth] Listener cancelled (epoch changed)");
            break;
        }

        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            auth_log("[Auth] Listener timed out waiting for OAuth callback");
            break;
        }

        let timeout = std::cmp::min(remaining, poll_interval);
        let request = match server.recv_timeout(timeout) {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(e) => {
                auth_log(format!("[Auth] Listener error: {}", e));
                break;
            }
        };

        let url = format!("http://localhost:12345{}", request.url());
        let parsed = match Url::parse(&url) {
            Ok(p) => p,
            Err(e) => {
                auth_log(format!("[Auth] Ignoring unparsable OAuth callback: {}", e));
                let _ = request.respond(Response::from_string("Bad request"));
                continue;
            }
        };
        let pairs: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        if let Some(code) = pairs.get("code") {
            if OAUTH_EPOCH.load(Ordering::SeqCst) != epoch {
                auth_log("[Auth] Epoch changed before processing code — discarding late callback");
                let _ = request
                    .respond(Response::from_string("Login session expired").with_status_code(410));
                break;
            }

            let current = get_oauth_state();
            if !csrf_matches(pairs.get("state"), current.csrf_state.as_ref()) {
                auth_log("[Auth] Rejected direct OAuth callback with invalid state");
                let _ = request.respond(
                    Response::from_string("Error: Invalid OAuth state").with_status_code(400),
                );
                continue;
            }
            let oauth_state = take_oauth_state();

            let (verifier, provider) = match (oauth_state.verifier, oauth_state.provider) {
                (Some(v), Some(p)) => (v, p),
                _ => {
                    auth_log("[Auth] Callback received but OAuth state is missing");
                    let _ = request.respond(Response::from_string("Error: Invalid OAuth state"));
                    continue;
                }
            };

            match exchange_code(&provider, code, &verifier) {
                Ok(session) => {
                    if OAUTH_EPOCH.load(Ordering::SeqCst) != epoch {
                        auth_log("[Auth] Epoch changed after exchange — discarding result");
                        break;
                    }
                    if let Err(e) = save_auth_session(&session) {
                        auth_log(format!("[Auth] Could not persist integration session: {e}"));
                        let _ = request.respond(
                            Response::from_string("Could not save session").with_status_code(500),
                        );
                        continue;
                    }

                    if provider == "jira" {
                        if let Err(error) = save_jira_specific_tokens(&session) {
                            auth_log(format!("[Auth] Could not save Jira integration: {error}"));
                            let _ = request.respond(
                                Response::from_string("Could not save Jira connection")
                                    .with_status_code(500),
                            );
                            continue;
                        }
                    }

                    let _ = request.respond(
                        Response::from_string(supabase_login_success_html()).with_header(
                            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..])
                                .unwrap(),
                        ),
                    );
                    break;
                }
                Err(e) => {
                    let _ = request.respond(Response::from_string(format!("Error: {}", e)));
                }
            }
        }
    }

    auth_log("[Auth] Direct listener exiting");
}

fn listen_for_supabase_callback(server: Server, epoch: u64) {
    auth_log("[Auth] Supabase callback listener starting with bound server");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let poll_interval = std::time::Duration::from_millis(250);

    let capture_html = r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <title>Flowmates Login</title>
</head>
<body>
  <p>Completing login, please wait...</p>
  <script>
    (function () {
      var hash = window.location.hash.substring(1);
      if (!hash) {
        document.body.innerHTML = '<p style="color:red">Login failed &ndash; no token received. Please close this tab and try again.</p>';
        return;
      }
      var form = document.createElement('form');
      form.method = 'POST';
      form.action = '/token';
      var state = new URLSearchParams(window.location.search).get('state');
      if (!state) {
        document.body.textContent = 'Login failed — invalid OAuth state. Please close this tab and try again.';
        return;
      }
      var stateInput = document.createElement('input');
      stateInput.type = 'hidden';
      stateInput.name = 'state';
      stateInput.value = state;
      form.appendChild(stateInput);
      hash.split('&').forEach(function (pair) {
        var eqIdx = pair.indexOf('=');
        if (eqIdx === -1) return;
        var key = decodeURIComponent(pair.substring(0, eqIdx));
        var val = decodeURIComponent(pair.substring(eqIdx + 1));
        var inp = document.createElement('input');
        inp.type = 'hidden';
        inp.name = key;
        inp.value = val;
        form.appendChild(inp);
      });
      document.body.appendChild(form);
      form.submit();
    })();
  </script>
</body>
</html>"#;

    loop {
        if OAUTH_EPOCH.load(Ordering::SeqCst) != epoch {
            auth_log("[Auth] Supabase listener cancelled (epoch changed)");
            break;
        }

        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            auth_log("[Auth] Supabase listener timed out waiting for callback");
            break;
        }

        let timeout = std::cmp::min(remaining, poll_interval);
        let mut request = match server.recv_timeout(timeout) {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(e) => {
                auth_log(format!("[Auth] Supabase listener error: {}", e));
                break;
            }
        };

        let request_method = request.method().clone();
        let request_target = request.url().to_string();
        let url = format!("http://localhost:12345{}", request_target);
        let parsed = match Url::parse(&url) {
            Ok(p) => p,
            Err(e) => {
                auth_log(format!(
                    "[Auth] Ignoring unparsable Supabase callback target: {}",
                    e
                ));
                let _ = request.respond(Response::from_string("Bad request"));
                continue;
            }
        };
        let path = parsed.path();
        let mut pairs: std::collections::HashMap<String, String> =
            parsed.query_pairs().into_owned().collect();
        if request_method == Method::Post && path == "/token" {
            let mut body = String::new();
            if request
                .as_reader()
                .take(64 * 1024)
                .read_to_string(&mut body)
                .is_err()
            {
                let _ = request.respond(Response::from_string("Bad request").with_status_code(400));
                continue;
            }
            pairs.extend(url::form_urlencoded::parse(body.as_bytes()).into_owned());
        }
        auth_log(format!(
            "[Auth] Supabase callback request received: {} {}",
            request_method.as_str(),
            path
        ));

        if let Some(error) = pairs.get("error") {
            let desc = pairs
                .get("error_description")
                .or_else(|| pairs.get("error_code"))
                .map(|s| s.as_str())
                .unwrap_or("");
            auth_log(format!(
                "[Auth] Supabase OAuth error on {}: {} - {}",
                path, error, desc
            ));
            let _ = request.respond(
                Response::from_string(supabase_login_error_html(error, desc)).with_header(
                    tiny_http::Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"text/html; charset=utf-8"[..],
                    )
                    .unwrap(),
                ),
            );
            break;
        }

        if path == "/callback" {
            if OAUTH_EPOCH.load(Ordering::SeqCst) != epoch {
                auth_log("[Auth] Epoch changed — discarding late callback request");
                let _ = request
                    .respond(Response::from_string("Login session expired").with_status_code(410));
                break;
            }
            let current = get_oauth_state();
            if !csrf_matches(pairs.get("state"), current.csrf_state.as_ref()) {
                auth_log("[Auth] Rejected Supabase callback with invalid state");
                let _ = request
                    .respond(Response::from_string("Invalid OAuth state").with_status_code(400));
                break;
            }
            auth_log("[Auth] Serving token capture page (form-submit method)...");
            let _ = request.respond(
                Response::from_string(capture_html).with_header(
                    tiny_http::Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"text/html; charset=utf-8"[..],
                    )
                    .unwrap(),
                ),
            );
            continue;
        }

        if path == "/token" {
            auth_log("[Auth] Supabase token callback received");
            if request_method != Method::Post {
                let _ = request
                    .respond(Response::from_string("Method not allowed").with_status_code(405));
                continue;
            }

            if OAUTH_EPOCH.load(Ordering::SeqCst) != epoch {
                auth_log("[Auth] Epoch changed before processing Supabase token — discarding");
                let _ = request
                    .respond(Response::from_string("Login session expired").with_status_code(410));
                break;
            }

            let oauth_state = take_oauth_state();
            if !csrf_matches(pairs.get("state"), oauth_state.csrf_state.as_ref()) {
                auth_log("[Auth] Rejected Supabase token POST with invalid state");
                let _ = request
                    .respond(Response::from_string("Invalid OAuth state").with_status_code(400));
                break;
            }

            if let Some(access_token) = pairs.get("access_token") {
                let provider = oauth_state.provider.unwrap_or_else(|| "google".to_string());
                auth_log(format!(
                    "[Auth] Supabase access token received (provider: {}, refresh_token: {})",
                    provider,
                    pairs.contains_key("refresh_token")
                ));

                match fetch_supabase_user(access_token) {
                    Ok(user) => {
                        if OAUTH_EPOCH.load(Ordering::SeqCst) != epoch {
                            auth_log("[Auth] Epoch changed after fetching user — discarding");
                            break;
                        }
                        let session = AuthSession {
                            user,
                            access_token: access_token.clone(),
                            refresh_token: pairs.get("refresh_token").cloned(),
                            provider,
                        };
                        if let Err(e) = save_auth_session(&session) {
                            auth_log(format!("[Auth] Could not persist Supabase session: {e}"));
                            let _ = request.respond(
                                Response::from_string("Could not save session")
                                    .with_status_code(500),
                            );
                            break;
                        }
                        auth_log("[Auth] Supabase login successful");
                        let _ = request.respond(
                            Response::from_string(supabase_login_success_html()).with_header(
                                tiny_http::Header::from_bytes(
                                    &b"Content-Type"[..],
                                    &b"text/html"[..],
                                )
                                .unwrap(),
                            ),
                        );
                        break;
                    }
                    Err(e) => {
                        auth_log(format!("[Auth] Failed to fetch Supabase user: {}", e));
                        let _ = request.respond(Response::from_string(format!("Error: {}", e)));
                    }
                }
            } else if let Some(error) = pairs.get("error") {
                let desc = pairs
                    .get("error_description")
                    .map(|s| s.as_str())
                    .unwrap_or("");
                auth_log(format!(
                    "[Auth] OAuth error from Supabase: {} - {}",
                    error, desc
                ));
                let _ = request.respond(Response::from_string(format!(
                    "Auth Error: {} - {}",
                    error, desc
                )));
                break;
            } else {
                auth_log("[Auth] /token callback received without access_token or error");
            }
        } else {
            auth_log(format!(
                "[Auth] Ignoring unexpected Supabase callback path: {}",
                path
            ));
            let _ = request.respond(Response::from_string("Not found").with_status_code(404));
        }
    }
}

fn fetch_supabase_user(access_token: &str) -> Result<AuthUser, String> {
    let client = auth_http_client()?;
    let resp = client
        .get(format!("{}/auth/v1/user", supabase_url()))
        .header("apikey", supabase_anon_key())
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err("Failed to fetch user from Supabase".to_string());
    }

    let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;

    Ok(AuthUser {
        id: json["id"].as_str().unwrap_or_default().to_string(),
        email: json["email"].as_str().unwrap_or_default().to_string(),
        display_name: json["user_metadata"]["full_name"]
            .as_str()
            .or(json["user_metadata"]["name"].as_str())
            .unwrap_or("User")
            .to_string(),
        avatar_url: json["user_metadata"]["avatar_url"]
            .as_str()
            .map(String::from),
        provider: "google".to_string(),
    })
}

fn exchange_code(provider: &str, code: &str, verifier: &str) -> Result<AuthSession, String> {
    if !matches!(provider, "jira" | "linear") {
        return Err("Unsupported direct OAuth provider".to_string());
    }

    let conn = get_db_conn()?;
    let cloud_session = crate::sync::get_user_session_from_conn(&conn)
        .ok_or("Sign in to Flowmates before connecting an integration")?;
    let response = auth_http_client()?
        .post(format!("{}/functions/v1/oauth-exchange", supabase_url()))
        .header("apikey", supabase_anon_key())
        .header(
            "Authorization",
            format!("Bearer {}", cloud_session.access_token),
        )
        .json(&serde_json::json!({
            "provider": provider,
            "code": code,
            "code_verifier": verifier,
            "redirect_uri": REDIRECT_URL,
        }))
        .send()
        .map_err(|e| format!("Secure token exchange failed: {e}"))?;
    let status = response.status();
    let payload: serde_json::Value = response
        .json()
        .map_err(|e| format!("Invalid token-exchange response: {e}"))?;
    if !status.is_success() {
        return Err(payload["error"]
            .as_str()
            .unwrap_or("Token exchange failed")
            .to_string());
    }
    let access_token = payload["access_token"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("Token exchange returned no access token")?
        .to_string();
    let refresh_token = payload["refresh_token"].as_str().map(String::from);

    // Fetch user info
    let user = fetch_user_info(provider, &access_token)?;

    Ok(AuthSession {
        user,
        access_token,
        refresh_token,
        provider: provider.to_string(),
    })
}

fn fetch_user_info(provider: &str, access_token: &str) -> Result<AuthUser, String> {
    let http_client = auth_http_client()?;

    match provider {
        "google" => {
            let resp = http_client
                .get(GOOGLE.userinfo_url)
                .bearer_auth(access_token)
                .send()
                .map_err(|e| e.to_string())?;

            let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;

            Ok(AuthUser {
                id: json["sub"].as_str().unwrap_or_default().to_string(),
                email: json["email"].as_str().unwrap_or_default().to_string(),
                display_name: json["name"].as_str().unwrap_or_default().to_string(),
                avatar_url: json["picture"].as_str().map(String::from),
                provider: "google".to_string(),
            })
        }
        "jira" => {
            let resp = http_client
                .get(JIRA.userinfo_url)
                .bearer_auth(access_token)
                .send()
                .map_err(|e| e.to_string())?;

            let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;

            Ok(AuthUser {
                id: json["account_id"].as_str().unwrap_or_default().to_string(),
                email: json["email"].as_str().unwrap_or_default().to_string(),
                display_name: json["name"].as_str().unwrap_or_default().to_string(),
                avatar_url: json["picture"].as_str().map(String::from),
                provider: "jira".to_string(),
            })
        }
        "linear" => {
            let query = r#"{ "query": "{ viewer { id email name avatarUrl } }" }"#;
            let resp = http_client
                .post(LINEAR.userinfo_url)
                .bearer_auth(access_token)
                .header("Content-Type", "application/json")
                .body(query)
                .send()
                .map_err(|e| e.to_string())?;

            let json: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
            let viewer = &json["data"]["viewer"];

            Ok(AuthUser {
                id: viewer["id"].as_str().unwrap_or_default().to_string(),
                email: viewer["email"].as_str().unwrap_or_default().to_string(),
                display_name: viewer["name"].as_str().unwrap_or_default().to_string(),
                avatar_url: viewer["avatarUrl"].as_str().map(String::from),
                provider: "linear".to_string(),
            })
        }
        _ => Err("Unknown provider".to_string()),
    }
}

fn get_db_conn() -> Result<Connection, String> {
    let conn = Connection::open(crate::paths::db_path()?).map_err(|e| e.to_string())?;
    // Asegurar tabla `config` por si somos los primeros en abrir la DB (antes
    // de que agent::init_db corra). Sin esto, los INSERT posteriores también
    // fallan en silencio.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS config (key TEXT PRIMARY KEY, value TEXT)",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(conn)
}

fn save_jira_specific_tokens(session: &AuthSession) -> Result<(), String> {
    crate::jira::save_tokens(&session.access_token, session.refresh_token.as_deref())
}

fn save_auth_session(session: &AuthSession) -> Result<(), String> {
    let conn = get_db_conn()?;
    let previous_cloud_user =
        crate::sync::get_user_session_from_conn(&conn).map(|existing| existing.user_id);
    let json = serde_json::to_string(session).map_err(|e| e.to_string())?;
    let storage_key = match session.provider.as_str() {
        "jira" | "linear" => {
            let cloud_user = crate::sync::get_user_session_from_conn(&conn)
                .ok_or("Sign in to Flowmates before connecting an integration")?;
            format!("{}_auth_session:{}", session.provider, cloud_user.user_id)
        }
        _ => "auth_session".to_string(),
    };
    crate::secure_store::set_secret(&storage_key, &json)?;
    conn.execute("DELETE FROM config WHERE key = ?1", [&storage_key])
        .map_err(|e| e.to_string())?;
    if storage_key == "auth_session" {
        if previous_cloud_user
            .as_deref()
            .is_some_and(|user_id| user_id != session.user.id)
        {
            crate::entitlements::clear_entitlements(&conn)?;
        }
        crate::sync::save_user_session(
            session.user.id.clone(),
            None,
            session.access_token.clone(),
            session.refresh_token.clone(),
            session.user.email.clone(),
        )?;
    }
    log::info!("[Auth] Session saved in macOS Keychain");
    Ok(())
}

#[tauri::command]
pub fn get_auth_session() -> Result<Option<AuthSessionView>, String> {
    let conn = get_db_conn()?;
    let mut json = crate::secure_store::get_secret("auth_session")?;
    if json.is_none() {
        json = conn
            .query_row(
                "SELECT value FROM config WHERE key = 'auth_session'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok();
        if let Some(ref legacy) = json {
            crate::secure_store::set_secret("auth_session", legacy)?;
            conn.execute("DELETE FROM config WHERE key = 'auth_session'", [])
                .map_err(|e| e.to_string())?;
        }
    }

    match json {
        Some(j) => {
            Ok(serde_json::from_str::<AuthSession>(&j)
                .ok()
                .map(|session| AuthSessionView {
                    user: session.user,
                    provider: session.provider,
                }))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub fn logout() -> Result<(), String> {
    // Cancel any in-flight OAuth so late callbacks cannot recreate sessions
    OAUTH_EPOCH.fetch_add(1, Ordering::SeqCst);
    OAUTH_IN_PROGRESS.store(false, Ordering::SeqCst);
    clear_oauth_state();

    let conn = get_db_conn()?;
    let mut cloud_user_id = None;
    if let Some(session_json) = crate::secure_store::get_secret("auth_session")? {
        if let Ok(session) = serde_json::from_str::<AuthSession>(&session_json) {
            cloud_user_id = Some(session.user.id.clone());
            if let Ok(client) = auth_http_client() {
                let _ = client
                    .post(format!("{}/auth/v1/logout", supabase_url()))
                    .header("apikey", supabase_anon_key())
                    .header("Authorization", format!("Bearer {}", session.access_token))
                    .send();
            }
        }
    }
    if cloud_user_id.is_none() {
        cloud_user_id =
            crate::sync::get_user_session_from_conn(&conn).map(|session| session.user_id);
    }
    if let Some(user_id) = cloud_user_id.as_deref() {
        for account in [
            format!("jira_auth_session:{user_id}"),
            format!("linear_auth_session:{user_id}"),
            format!("jira_access_token:{user_id}"),
            format!("jira_refresh_token:{user_id}"),
        ] {
            crate::secure_store::delete_secret(&account)?;
        }
        conn.execute(
            "DELETE FROM config WHERE key = ?1",
            [format!("jira_cloud_id:{user_id}")],
        )
        .map_err(|e| e.to_string())?;
    }
    for key in [
        "auth_session",
        "user_session",
        "jira_auth_session",
        "linear_auth_session",
        "jira_access_token",
        "jira_refresh_token",
    ] {
        crate::secure_store::delete_secret(key)?;
    }
    conn.execute(
        "DELETE FROM config WHERE key IN (
            'auth_session', 'user_session', 'jira_auth_session', 'linear_auth_session',
            'jira_access_token', 'jira_refresh_token', 'jira_cloud_id', 'coach_chat_messages'
        )",
        [],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM config WHERE key LIKE 'coach_chat_messages:%'",
        [],
    )
    .map_err(|e| e.to_string())?;
    crate::entitlements::clear_entitlements(&conn)?;
    println!("[Auth] Logged out and cleared account-scoped sessions and chat history");
    Ok(())
}

#[tauri::command]
pub fn login_with_password(email: String, password: String) -> Result<AuthSessionView, String> {
    crate::sync_env::require_cloud()?;
    let normalized_email = email.trim();
    if normalized_email.is_empty()
        || normalized_email.len() > 320
        || !normalized_email.contains('@')
    {
        return Err("Enter a valid email address".to_string());
    }
    if password.is_empty() || password.len() > 4096 {
        return Err("Enter a valid password".to_string());
    }

    let response = auth_http_client()?
        .post(format!(
            "{}/auth/v1/token?grant_type=password",
            supabase_url()
        ))
        .header("apikey", supabase_anon_key())
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "email": normalized_email,
            "password": password,
        }))
        .send()
        .map_err(|e| format!("Cloud login failed: {e}"))?;
    let status = response.status();
    let payload: serde_json::Value = response
        .json()
        .map_err(|e| format!("Invalid cloud login response: {e}"))?;
    if !status.is_success() {
        return Err(payload["msg"]
            .as_str()
            .or_else(|| payload["error_description"].as_str())
            .or_else(|| payload["error"].as_str())
            .unwrap_or("Invalid email or password")
            .to_string());
    }

    let access_token = payload["access_token"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("Cloud login returned no access token")?
        .to_string();
    let refresh_token = payload["refresh_token"].as_str().map(String::from);
    let user_json = payload.get("user").ok_or("Cloud login returned no user")?;
    let user_id = user_json["id"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("Cloud login returned an invalid user")?
        .to_string();
    let user = AuthUser {
        id: user_id,
        email: user_json["email"]
            .as_str()
            .unwrap_or(normalized_email)
            .to_string(),
        display_name: user_json["user_metadata"]["full_name"]
            .as_str()
            .or_else(|| user_json["user_metadata"]["name"].as_str())
            .unwrap_or(normalized_email)
            .to_string(),
        avatar_url: user_json["user_metadata"]["avatar_url"]
            .as_str()
            .map(String::from),
        provider: "cloud".to_string(),
    };
    let session = AuthSession {
        user,
        access_token,
        refresh_token,
        provider: "cloud".to_string(),
    };
    save_auth_session(&session)?;
    Ok(session.into())
}

#[tauri::command]
pub fn is_logged_in() -> Result<bool, String> {
    Ok(get_auth_session()?.is_some())
}

#[tauri::command]
pub fn cancel_auth() -> Result<(), String> {
    OAUTH_EPOCH.fetch_add(1, Ordering::SeqCst);
    OAUTH_IN_PROGRESS.store(false, Ordering::SeqCst);
    clear_oauth_state();
    auth_log("[Auth] OAuth cancelled by user");
    Ok(())
}
