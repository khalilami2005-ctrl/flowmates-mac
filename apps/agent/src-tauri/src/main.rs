/// Loads `.env.local` from the repository root, in development builds only.
///
/// Vite already feeds the renderer from that file; without this the Rust side
/// stayed blind in `tauri dev`, so cloud commands reported themselves
/// unconfigured while the renderer held a perfectly good configuration.
///
/// Deliberately `debug_assertions`-only: a shipped binary must never read paths
/// relative to a repository that does not exist on the user's machine. Release
/// keeps a single override, a `.env` beside the executable.
#[cfg(debug_assertions)]
fn load_repo_env() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut dir = exe.parent().map(|p| p.to_path_buf());
    while let Some(current) = dir {
        for name in [".env.local", ".env"] {
            let candidate = current.join(name);
            if candidate.is_file() {
                let _ = dotenv::from_filename(&candidate);
            }
        }
        if current.join("pnpm-workspace.yaml").is_file() {
            return;
        }
        dir = current.parent().map(|p| p.to_path_buf());
    }
}

fn main() {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let _ = dotenv::from_filename(dir.join(".env"));
        }
    }

    #[cfg(debug_assertions)]
    load_repo_env();

    // State the backend situation once, out loud. Silence here used to mean a
    // developer could not tell an unconfigured build from a misconfigured one.
    eprintln!("[Flowmates] backend: {}", app_lib::backend_status());
    eprintln!("[Flowmates] secrets: {}", app_lib::secret_store_name());

    std::panic::set_hook(Box::new(|info| {
        let msg = format!(
            "[{}] PANIC: {}\nLocation: {}\n\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            info.payload()
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
                .unwrap_or("<non-string panic>"),
            info.location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "<unknown>".into()),
        );
        eprintln!("{}", msg);
        log::error!("{}", msg);
        let path = app_lib::paths::crash_log_path_or_fallback();
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&path)
        {
            let _ = f.write_all(msg.as_bytes());
        }
    }));

    app_lib::run();
}
