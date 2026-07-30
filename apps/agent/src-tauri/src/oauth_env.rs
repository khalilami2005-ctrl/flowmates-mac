//! Public OAuth client identifiers used to build provider authorization URLs.
//! Client secrets are deliberately never compiled into the desktop app; token
//! exchange happens in the authenticated `oauth-exchange` Edge Function.

fn first_non_empty<const N: usize>(keys: [&str; N]) -> Option<String> {
    for k in keys {
        if let Ok(v) = std::env::var(k) {
            if !v.trim().is_empty() {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

pub fn jira_client_id() -> String {
    first_non_empty(["TAURI_JIRA_CLIENT_ID", "VITE_JIRA_CLIENT_ID"])
        .or_else(|| option_env!("TAURI_JIRA_CLIENT_ID").map(str::to_string))
        .or_else(|| option_env!("VITE_JIRA_CLIENT_ID").map(str::to_string))
        .unwrap_or_default()
}

pub fn linear_client_id() -> String {
    first_non_empty(["TAURI_LINEAR_CLIENT_ID", "VITE_LINEAR_CLIENT_ID"])
        .or_else(|| option_env!("TAURI_LINEAR_CLIENT_ID").map(str::to_string))
        .or_else(|| option_env!("VITE_LINEAR_CLIENT_ID").map(str::to_string))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jira_client_id_prefers_tauri_env() {
        temp_env::with_vars(
            [
                ("TAURI_JIRA_CLIENT_ID", Some("from-tauri")),
                ("VITE_JIRA_CLIENT_ID", Some("from-vite")),
            ],
            || {
                assert_eq!(jira_client_id(), "from-tauri");
            },
        );
    }

    #[test]
    fn linear_client_id_falls_back_to_vite() {
        temp_env::with_vars(
            [
                ("TAURI_LINEAR_CLIENT_ID", None::<&str>),
                ("VITE_LINEAR_CLIENT_ID", Some("linear-vite")),
            ],
            || {
                assert_eq!(linear_client_id(), "linear-vite");
            },
        );
    }
}
