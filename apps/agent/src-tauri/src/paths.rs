//! Single source of truth for filesystem paths used by the agent.
//!
//! Motivation: hasta ahora cada módulo (`auth`, `sync`, `jira`, `linear`,
//! `agent`, `main`) construía por su cuenta `dirs::data_local_dir().unwrap().join("Flowmates")`
//! sin garantizar que el directorio existiese. En instalación fresca
//! (pre-login, pre-`initialize_agent`) el directorio no existe y cualquier
//! `Connection::open` o escritura de log fallaba silenciosamente.
//!
//! Todos los paths del runtime del usuario deben pasar por acá. Los paths
//! de recursos read-only bundlados con el instalador de Tauri se resuelven
//! vía `resource_local_llm_dir` y requieren `AppHandle`.

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde_json::json;
use tauri::{AppHandle, Manager};

const APP_DIR_NAME: &str = "Flowmates";
const DB_FILE: &str = "dev-agent.db";
const SERVER_LOG_FILE: &str = "server.log";
const AGENT_ERROR_LOG_FILE: &str = "agent_error.log";
const CRASH_LOG_FILE: &str = "crash.log";
const SCREENSHOTS_TMP_DIR: &str = "screenshots_tmp";

/// Carpeta local de datos de Flowmates dentro del perfil del usuario (creada si no existe).
///
/// Se resuelve con `dirs::data_local_dir()`: ruta real en disco, indépendante
/// de la carpeta de instalación de l'application.
pub fn app_data_dir() -> Result<PathBuf, String> {
    let base = dirs::data_local_dir().ok_or_else(|| "No local data dir available".to_string())?;
    let dir = base.join(APP_DIR_NAME);
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create {:?}: {}", dir, e))?;
    }
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("Failed to secure {:?}: {}", dir, e))?;
    Ok(dir)
}

/// Path a `dev-agent.db`. Crea el directorio padre si hace falta.
pub fn db_path() -> Result<PathBuf, String> {
    let path = app_data_dir()?.join(DB_FILE);
    secure_existing_private_file(&path)?;
    Ok(path)
}

/// Variante infalible para sitios donde no podemos propagar Result (panic hooks,
/// static init). En ese caso cae a `.` que es subóptimo pero no panica.
pub fn db_path_or_fallback() -> PathBuf {
    db_path().unwrap_or_else(|_| PathBuf::from(DB_FILE))
}

pub fn server_log_path() -> Result<PathBuf, String> {
    private_log_path(SERVER_LOG_FILE)
}

pub fn auth_log_path() -> Result<PathBuf, String> {
    private_log_path("auth.log")
}

pub fn agent_error_log_path() -> Result<PathBuf, String> {
    private_log_path(AGENT_ERROR_LOG_FILE)
}

pub fn crash_log_path_or_fallback() -> PathBuf {
    private_log_path(CRASH_LOG_FILE).unwrap_or_else(|_| PathBuf::from(CRASH_LOG_FILE))
}

fn secure_existing_private_file(path: &Path) -> Result<(), String> {
    if path.exists() {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("Failed to secure {:?}: {error}", path))?;
    }
    Ok(())
}

fn private_log_path(filename: &str) -> Result<PathBuf, String> {
    let path = app_data_dir()?.join(filename);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&path)
        .map_err(|error| format!("Failed to create private log {:?}: {error}", path))?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Failed to secure private log {:?}: {error}", path))?;
    Ok(path)
}

/// PNG de capture temporaires pour le diagnostic, dans le même arbre privé que la base locale.
pub fn screenshots_tmp_dir() -> Result<PathBuf, String> {
    let dir = app_data_dir()?.join(SCREENSHOTS_TMP_DIR);
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create {:?}: {}", dir, e))?;
    }
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("Failed to secure {:?}: {}", dir, e))?;
    Ok(dir)
}

/// Detects write failures before SQLite initialization.
pub fn verify_app_dir_filesystem_writable() -> Result<(), String> {
    let dir = app_data_dir()?;
    let probe = dir.join(format!(
        ".flowmates_fs_write_probe_{}",
        uuid::Uuid::new_v4().simple()
    ));
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&probe)
        .map_err(|e| {
        format!(
            "Cannot write application data under {} ({e}). Check the folder ownership and macOS privacy permissions.",
            dir.display()
        )
    })?;
    if let Err(error) = file.write_all(b"ok") {
        let _ = std::fs::remove_file(&probe);
        return Err(format!("Cannot write application data probe: {error}"));
    }
    drop(file);
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// Elimina capturas de depuración `capture_*` más antiguas que `max_age` (retención / cumplimiento).
pub fn prune_screenshots_tmp_older_than(max_age: std::time::Duration) -> Result<usize, String> {
    use std::time::SystemTime;

    let dir = screenshots_tmp_dir()?;
    let now = SystemTime::now();
    let mut removed = 0usize;
    let entries =
        std::fs::read_dir(&dir).map_err(|e| format!("Failed to read screenshots_tmp: {e}"))?;
    for ent in entries.filter_map(Result::ok) {
        let name = ent.file_name();
        let s = name.to_string_lossy();
        if !s.starts_with("capture_") {
            continue;
        }
        let Ok(meta) = ent.metadata() else {
            continue;
        };
        let Ok(mtime) = meta.modified() else {
            continue;
        };
        let Ok(elapsed) = now.duration_since(mtime) else {
            continue;
        };
        if elapsed > max_age {
            let p = ent.path();
            if std::fs::remove_file(&p).is_ok() {
                removed += 1;
            }
        }
    }
    Ok(removed)
}

/// Resuelve el directorio de recursos bundlados donde vive `local_llm/`.
///
/// Dans l'application macOS, Tauri place `bundle.resources` sous le dossier
/// de ressources du bundle. En dev, on retombe sur le layout du dépôt.
pub fn resource_local_llm_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("resource_dir unavailable: {}", e))?;

    let bundled = resource_dir.join("local_llm");
    if bundled.join("Qwen3-VL-2B-Instruct-Q3_K_M.gguf").exists()
        && bundled
            .join("mmproj-Qwen3VL-2B-Instruct-Q8_0.gguf")
            .exists()
    {
        return Ok(bundled);
    }

    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        for _ in 0..8 {
            let candidate = dir.join("local_llm");
            if candidate.join("Qwen3-VL-2B-Instruct-Q3_K_M.gguf").exists()
                && candidate
                    .join("mmproj-Qwen3VL-2B-Instruct-Q8_0.gguf")
                    .exists()
            {
                return Ok(candidate);
            }
            if !dir.pop() {
                break;
            }
        }
    }

    Err(format!(
        "local_llm runtime not found (looked in bundled resources at {:?} and dev tree)",
        bundled
    ))
}

pub fn resource_llama_server_path(app: &AppHandle) -> Result<PathBuf, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("resource_dir unavailable: {e}"))?;

    if let Some(contents_dir) = resource_dir.parent() {
        let bundled_sidecar = contents_dir.join("MacOS").join("llama-server");
        if bundled_sidecar.is_file() {
            return Ok(bundled_sidecar);
        }
    }

    if let Ok(executable) = std::env::current_exe() {
        if let Some(executable_dir) = executable.parent() {
            let copied_sidecar = executable_dir.join("llama-server");
            if copied_sidecar.is_file() {
                return Ok(copied_sidecar);
            }
        }

        let mut dir = executable.parent().map(PathBuf::from).unwrap_or_default();
        for _ in 0..8 {
            let candidate = dir.join("local_llm").join("bin").join("llama-server");
            if candidate.is_file() {
                return Ok(candidate);
            }
            if !dir.pop() {
                break;
            }
        }
    }

    Err("signed llama-server sidecar not found in the app bundle or development tree".to_string())
}

fn sanitize_pdf_filename(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.len() < 5 || trimmed.len() > 128 || !trimmed.to_ascii_lowercase().ends_with(".pdf") {
        return Err("Invalid PDF filename".to_string());
    }
    let stem = &trimmed[..trimmed.len() - 4];
    if stem.is_empty()
        || !stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        || stem == "."
        || stem == ".."
    {
        return Err("Invalid PDF filename".to_string());
    }
    Ok(format!("{stem}.pdf"))
}

fn unique_download_path(downloads: &std::path::Path, filename: &str) -> PathBuf {
    let mut path = downloads.join(filename);
    if !path.exists() {
        return path;
    }
    let stem = filename.strip_suffix(".pdf").unwrap_or(filename);
    for n in 2..=99 {
        let candidate = format!("{stem}_{n}.pdf");
        path = downloads.join(&candidate);
        if !path.exists() {
            return path;
        }
    }
    let stamp = chrono::Local::now().format("%H%M%S");
    downloads.join(format!("{stem}_{stamp}.pdf"))
}

/// Guarda un PDF en la carpeta Descargas del usuario y devuelve la ruta absoluta escrita.
#[tauri::command]
pub fn save_pdf_to_downloads(filename: String, bytes: Vec<u8>) -> Result<String, String> {
    const MAX_PDF_BYTES: usize = 50 * 1024 * 1024;
    if bytes.len() < 8 || bytes.len() > MAX_PDF_BYTES || !bytes.starts_with(b"%PDF-") {
        return Err("Invalid PDF payload".to_string());
    }
    let downloads = dirs::download_dir()
        .ok_or_else(|| "Downloads folder not available on this system".to_string())?;
    let safe = sanitize_pdf_filename(&filename)?;
    let path = unique_download_path(&downloads, &safe);
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .map_err(|error| format!("Failed to create PDF: {error}"))?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(format!("Failed to save PDF: {error}"));
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Failed to secure PDF: {error}"))?;
    Ok(path.to_string_lossy().to_string())
}

fn canonical_path_in_roots(path: &Path, roots: &[PathBuf]) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("Only absolute paths can be opened".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("Path does not exist or cannot be resolved: {error}"))?;
    let allowed = roots.iter().any(|root| {
        root.canonicalize()
            .map(|root| canonical.starts_with(root))
            .unwrap_or(false)
    });
    if !allowed {
        return Err("Opening this path is not allowed".to_string());
    }
    Ok(canonical)
}

/// Abre la carpeta que contiene `path` (si es un archivo, abre su directorio padre).
#[tauri::command]
pub fn open_path_in_file_manager(path: String) -> Result<(), String> {
    let requested = PathBuf::from(path);
    let mut roots = vec![app_data_dir()?];
    if let Some(downloads) = dirs::download_dir() {
        roots.push(downloads);
    }
    let canonical = canonical_path_in_roots(&requested, &roots)?;
    let target = if canonical.is_file() {
        canonical
            .parent()
            .map(|parent| parent.to_path_buf())
            .ok_or_else(|| "File has no containing directory".to_string())?
    } else {
        canonical
    };
    open::that(&target).map_err(|e| format!("Could not open folder: {e}"))
}

#[tauri::command]
pub fn get_flowmates_user_paths() -> Result<serde_json::Value, String> {
    let dir = app_data_dir()?;
    Ok(json!({
        "appDataDir": dir.to_string_lossy(),
        "serverLog": server_log_path()?.to_string_lossy(),
        "authLog": auth_log_path()?.to_string_lossy(),
        "agentErrorLog": agent_error_log_path()?.to_string_lossy(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_filename_rejects_path_components_and_normalizes_extension() {
        assert_eq!(
            sanitize_pdf_filename("Weekly_Report.PDF").unwrap(),
            "Weekly_Report.pdf"
        );
        assert!(sanitize_pdf_filename("../report.pdf").is_err());
        assert!(sanitize_pdf_filename("report/other.pdf").is_err());
        assert!(sanitize_pdf_filename("report.txt").is_err());
    }

    #[test]
    fn open_path_validation_rejects_escape_and_symlink_escape() {
        let temp = tempfile::tempdir().unwrap();
        let allowed = temp.path().join("allowed");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let inside_file = allowed.join("report.pdf");
        let outside_file = outside.join("secret.txt");
        std::fs::write(&inside_file, b"%PDF-1.7").unwrap();
        std::fs::write(&outside_file, b"secret").unwrap();

        assert_eq!(
            canonical_path_in_roots(&inside_file, std::slice::from_ref(&allowed)).unwrap(),
            inside_file.canonicalize().unwrap()
        );
        assert!(canonical_path_in_roots(&outside_file, std::slice::from_ref(&allowed)).is_err());

        let link = allowed.join("escape");
        std::os::unix::fs::symlink(&outside_file, &link).unwrap();
        assert!(canonical_path_in_roots(&link, &[allowed]).is_err());
    }
}
