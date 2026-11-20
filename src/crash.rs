//! Crash report: a panic (in any thread) is written to `crash.log` before the process ends
//! (release builds abort on panic), and told at the next start.

use crate::config;
use std::io::Write;
use std::path::PathBuf;

/// The report of the crashes, in the configuration folder.
pub fn log_path() -> PathBuf {
    config::config_dir().join("crash.log")
}

/// Left by a crash, removed once told.
fn pending_path() -> PathBuf {
    config::config_dir().join("crash.pending")
}

/// Records the panics in `crash.log`, then lets the default hook print them.
pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let report = format!(
            "=== florm {} ({}) — {secs} (unix)\nthread '{thread}' : {what}\nat {at}\n{}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::backtrace::Backtrace::force_capture()
        );
        write_report(&report);
        default(info);
    }));
}

/// Appends `report` to the log, keeping only its end (the last crashes).
fn write_report(report: &str) {
    let path = log_path();
    let _ = std::fs::create_dir_all(config::config_dir());
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let keep = old.len().saturating_sub(200_000);
    let start = (keep..=old.len()).find(|&i| old.is_char_boundary(i)).unwrap_or(old.len());
    if let Ok(mut f) = std::fs::File::create(&path) {
        let _ = f.write_all(&old.as_bytes()[start..]);
        let _ = f.write_all(report.as_bytes());
    }
    let _ = std::fs::write(pending_path(), "");
}

/// True once after a crash: the next start tells it.
pub fn take_pending() -> bool {
    std::fs::remove_file(pending_path()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_kept_and_told_once() {
        let _lock = config::TEST_CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("florm_crash_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("FLORM_CONFIG", &dir);
        assert!(!take_pending());
        write_report("premier\n");
        write_report("deuxième\n");
        assert_eq!(std::fs::read_to_string(log_path()).unwrap(), "premier\ndeuxième\n");
        assert!(take_pending());
        assert!(!take_pending());
        std::env::remove_var("FLORM_CONFIG");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
