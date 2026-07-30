fn main() {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let _ = dotenv::from_filename(dir.join(".env"));
        }
    }

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
