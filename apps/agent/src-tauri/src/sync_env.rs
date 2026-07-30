//! Backend configuration, read from the environment.
//!
//! **No server address and no key is compiled into this binary.** A build that
//! ships without configuration has no cloud: sign-in, sync, integrations and
//! remote insights report themselves unavailable instead of reaching a default
//! host. That default is how a product silently keeps talking to infrastructure
//! that is no longer its own.
//!
//! Set `NEXT_PUBLIC_SUPABASE_URL` and `NEXT_PUBLIC_SUPABASE_ANON_KEY` — at
//! runtime, or at build time to bake them into a release.

/// Backend base URL, or empty when none is configured.
pub(crate) fn supabase_url() -> String {
    std::env::var("NEXT_PUBLIC_SUPABASE_URL")
        .or_else(|_| std::env::var("VITE_SUPABASE_URL"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| option_env!("NEXT_PUBLIC_SUPABASE_URL").map(str::to_string))
        .or_else(|| option_env!("VITE_SUPABASE_URL").map(str::to_string))
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

/// Public API key, or empty when none is configured.
pub(crate) fn supabase_anon_key() -> String {
    std::env::var("NEXT_PUBLIC_SUPABASE_ANON_KEY")
        .or_else(|_| std::env::var("VITE_SUPABASE_PUBLIC_KEY"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| option_env!("NEXT_PUBLIC_SUPABASE_ANON_KEY").map(str::to_string))
        .or_else(|| option_env!("VITE_SUPABASE_PUBLIC_KEY").map(str::to_string))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

/// Whether cloud features can be attempted at all.
///
/// Callers use this to fail with a legible message rather than build a URL
/// against an empty base and surface a transport error to the user.
pub(crate) fn cloud_configured() -> bool {
    !supabase_url().is_empty() && !supabase_anon_key().is_empty()
}

/// The message shown when a cloud feature is used on an unconfigured build.
pub(crate) const CLOUD_UNCONFIGURED: &str =
    "No backend is configured for this build. Cloud features — sign-in, sync, \
     integrations — stay unavailable until one is set. Local measurement and \
     local reports are unaffected.";

/// `Err(CLOUD_UNCONFIGURED)` when no backend is set, `Ok(())` otherwise.
pub(crate) fn require_cloud() -> Result<(), String> {
    cloud_configured()
        .then_some(())
        .ok_or_else(|| CLOUD_UNCONFIGURED.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respects_env_overrides() {
        temp_env::with_vars(
            [
                (
                    "NEXT_PUBLIC_SUPABASE_URL",
                    Some("https://custom.supabase.co"),
                ),
                ("NEXT_PUBLIC_SUPABASE_ANON_KEY", Some("pk-test")),
            ],
            || {
                assert_eq!(supabase_url(), "https://custom.supabase.co");
                assert_eq!(supabase_anon_key(), "pk-test");
                assert!(cloud_configured());
                assert!(require_cloud().is_ok());
            },
        );
    }

    #[test]
    fn a_trailing_slash_never_doubles_in_a_built_url() {
        temp_env::with_vars(
            [("NEXT_PUBLIC_SUPABASE_URL", Some("https://host.example/"))],
            || assert_eq!(supabase_url(), "https://host.example"),
        );
    }

    /// The point of this file: an unconfigured build reaches nobody.
    #[test]
    fn no_host_is_compiled_in() {
        temp_env::with_vars(
            [
                ("NEXT_PUBLIC_SUPABASE_URL", None::<&str>),
                ("VITE_SUPABASE_URL", None::<&str>),
                ("NEXT_PUBLIC_SUPABASE_ANON_KEY", None::<&str>),
                ("VITE_SUPABASE_PUBLIC_KEY", None::<&str>),
            ],
            || {
                assert!(supabase_url().is_empty(), "a default host would be a leak");
                assert!(
                    supabase_anon_key().is_empty(),
                    "a default key would be a leak"
                );
                assert!(!cloud_configured());
                assert!(require_cloud().is_err());
            },
        );
    }

    /// Whitespace-only configuration is absence, not configuration.
    #[test]
    fn blank_configuration_counts_as_absent() {
        temp_env::with_vars(
            [
                ("NEXT_PUBLIC_SUPABASE_URL", Some("   ")),
                ("NEXT_PUBLIC_SUPABASE_ANON_KEY", Some("")),
            ],
            || assert!(!cloud_configured()),
        );
    }
}
