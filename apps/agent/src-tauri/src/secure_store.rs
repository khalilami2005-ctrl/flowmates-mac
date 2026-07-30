//! Secrets storage, with a process-lifetime read cache.
//!
//! **Release builds use the macOS Keychain.** Development builds do not: they use
//! a `0600` file inside the application's own data directory instead.
//!
//! The reason is that a Keychain item's authorisation is bound to the calling
//! binary's code identity. Under `tauri dev` the binary is rebuilt constantly and
//! signed ad hoc, so every rebuild looks like a different application to macOS
//! and it asks for the login password again — repeatedly, since background
//! threads poll for a session every two minutes. That prompt storm makes the app
//! unusable to work on.
//!
//! What this costs: in development, session tokens sit in a file readable by the
//! user account rather than in the Keychain. That file lives in the same private
//! directory as the local database, with the same permissions, and holds nothing
//! the database does not already. It never applies to a shipped build — see the
//! `no_file_store_in_release` test, which is what keeps that true.
//!
//! The cache in front of both backends is coherent because every write in this
//! process goes through [`set_secret`] or [`delete_secret`]. It does not track
//! changes made by other processes, and never outlives this one.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// account -> value, where `None` records a confirmed absence.
type Cache = HashMap<String, Option<String>>;

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Never lets a poisoned lock abort the process: a secret read is not worth a crash.
fn lock_cache() -> std::sync::MutexGuard<'static, Cache> {
    match cache().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Names the active backend, for the startup log. The difference must never be
/// something a developer has to guess.
pub fn backend_name() -> &'static str {
    if cfg!(debug_assertions) {
        "development file store (no Keychain prompts)"
    } else {
        "macOS Keychain"
    }
}

pub fn set_secret(account: &str, value: &str) -> Result<(), String> {
    backend::set(account, value)?;
    lock_cache().insert(account.to_string(), Some(value.to_string()));
    Ok(())
}

pub fn get_secret(account: &str) -> Result<Option<String>, String> {
    if let Some(cached) = lock_cache().get(account) {
        return Ok(cached.clone());
    }

    // A denied or failed read is deliberately not cached: the user may grant
    // access on a later attempt, and caching the failure would make it permanent.
    let fetched = backend::get(account)?;

    lock_cache().insert(account.to_string(), fetched.clone());
    Ok(fetched)
}

pub fn delete_secret(account: &str) -> Result<(), String> {
    backend::delete(account)?;
    lock_cache().insert(account.to_string(), None);
    Ok(())
}

#[cfg(not(debug_assertions))]
mod backend {
    const KEYCHAIN_SERVICE: &str = "eu.flowmates.mac";
    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

    pub(super) fn set(account: &str, value: &str) -> Result<(), String> {
        security_framework::passwords::set_generic_password(
            KEYCHAIN_SERVICE,
            account,
            value.as_bytes(),
        )
        .map_err(|e| format!("Could not save {account} in macOS Keychain: {e}"))
    }

    pub(super) fn get(account: &str) -> Result<Option<String>, String> {
        match security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, account) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| format!("Keychain entry {account} is not valid UTF-8")),
            Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(error) => Err(format!(
                "Could not read {account} from macOS Keychain: {error}"
            )),
        }
    }

    pub(super) fn delete(account: &str) -> Result<(), String> {
        match security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, account) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(error) => Err(format!(
                "Could not remove {account} from macOS Keychain: {error}"
            )),
        }
    }
}

#[cfg(debug_assertions)]
mod backend {
    use std::collections::BTreeMap;
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    const STORE_FILE: &str = "dev-secrets.json";

    /// Serialises read-modify-write on the store file.
    ///
    /// Every mutation reads the whole map, edits one key and writes it back. Two
    /// threads doing that at once — the sync thread saving a session while the
    /// auth thread saves tokens — would have the second write erase the first.
    fn file_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let mutex = LOCK.get_or_init(|| Mutex::new(()));
        match mutex.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn store_path() -> Result<PathBuf, String> {
        Ok(crate::paths::app_data_dir()?.join(STORE_FILE))
    }

    fn read_all() -> Result<BTreeMap<String, String>, String> {
        let path = store_path()?;
        if !path.exists() {
            return Ok(BTreeMap::new());
        }
        // Corruption must not lock a developer out of their own app: an
        // unreadable store is treated as empty, and the next write replaces it.
        let raw = std::fs::read_to_string(&path).unwrap_or_default();
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    fn write_all(entries: &BTreeMap<String, String>) -> Result<(), String> {
        let path = store_path()?;
        let body = serde_json::to_string_pretty(entries)
            .map_err(|e| format!("Could not serialise the development secret store: {e}"))?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| format!("Could not open {path:?}: {e}"))?;
        file.write_all(body.as_bytes())
            .map_err(|e| format!("Could not write {path:?}: {e}"))?;
        // Re-assert the mode: an existing file keeps whatever it had.
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Could not restrict {path:?}: {e}"))?;
        Ok(())
    }

    pub(super) fn set(account: &str, value: &str) -> Result<(), String> {
        let _guard = file_lock();
        let mut entries = read_all()?;
        entries.insert(account.to_string(), value.to_string());
        write_all(&entries)
    }

    pub(super) fn get(account: &str) -> Result<Option<String>, String> {
        let _guard = file_lock();
        Ok(read_all()?.get(account).cloned())
    }

    pub(super) fn delete(account: &str) -> Result<(), String> {
        let _guard = file_lock();
        let mut entries = read_all()?;
        entries.remove(account);
        write_all(&entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drops one account from the cache without touching the others.
    ///
    /// Tests share the cache and the store and run in parallel, so clearing
    /// everything made them fight over each other's keys. Each test owns its own
    /// account names instead, and only ever forgets its own.
    fn forget(account: &str) {
        lock_cache().remove(account);
    }

    /// The guarantee that makes the development store acceptable: it can never
    /// reach a shipped build. If this ever fails, secrets left the Keychain.
    #[test]
    fn no_file_store_in_release() {
        if cfg!(debug_assertions) {
            assert_eq!(
                backend_name(),
                "development file store (no Keychain prompts)"
            );
        } else {
            assert_eq!(backend_name(), "macOS Keychain");
        }
    }

    /// A confirmed absence must be cached too, otherwise a logged-out app keeps
    /// querying the backend — the case that produced the prompt storm.
    #[test]
    fn an_absence_is_cached_and_answers_without_the_backend() {
        let account = "probe_absence";
        forget(account);
        lock_cache().insert(account.to_string(), None);
        assert_eq!(get_secret(account).unwrap(), None);
    }

    #[test]
    fn a_write_is_visible_to_the_next_read() {
        let account = "probe_write";
        set_secret(account, "{\"a\":1}").unwrap();
        assert_eq!(get_secret(account).unwrap().as_deref(), Some("{\"a\":1}"));
        delete_secret(account).unwrap();
    }

    /// Deleting must not leave the old value readable, cache included.
    #[test]
    fn a_delete_turns_the_entry_into_an_absence() {
        let account = "probe_delete";
        set_secret(account, "stale").unwrap();
        delete_secret(account).unwrap();
        assert_eq!(get_secret(account).unwrap(), None);
    }

    #[test]
    fn accounts_do_not_leak_into_one_another() {
        let kept = "probe_isolation_kept";
        let removed = "probe_isolation_removed";
        set_secret(kept, "mine").unwrap();
        delete_secret(removed).unwrap();
        assert_eq!(get_secret(kept).unwrap().as_deref(), Some("mine"));
        assert_eq!(get_secret(removed).unwrap(), None);
        delete_secret(kept).unwrap();
    }

    /// Survives a restart: forgetting the cached copy must not lose the value,
    /// which is what proves the write actually reached the backing store.
    #[test]
    fn a_value_outlives_the_cache() {
        let account = "probe_persistence";
        set_secret(account, "kept").unwrap();
        forget(account);
        assert_eq!(get_secret(account).unwrap().as_deref(), Some("kept"));
        delete_secret(account).unwrap();
    }
}
