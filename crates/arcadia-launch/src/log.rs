//! Per-game launch log with simple size rotation.
//!
//! Port of `app/electron/launch-log.js`.  The directory is always passed in;
//! there is no module-level state.  Header timestamps use the injected clock,
//! which makes the append format testable without freezing wall time.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::clock::{Clock, SystemClock};

pub const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchLogError(pub String);

impl std::fmt::Display for LaunchLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for LaunchLogError {}

pub struct LaunchLog {
    log_dir: PathBuf,
    clock: Box<dyn Clock>,
}

impl LaunchLog {
    pub fn new(log_dir: impl Into<PathBuf>) -> Result<Self, LaunchLogError> {
        Self::with_clock(log_dir, Box::new(SystemClock))
    }

    pub fn with_clock(
        log_dir: impl Into<PathBuf>,
        clock: Box<dyn Clock>,
    ) -> Result<Self, LaunchLogError> {
        let log_dir = log_dir.into();
        if log_dir.as_os_str().is_empty() {
            return Err(LaunchLogError("logDir é obrigatório".to_string()));
        }
        Ok(Self { log_dir, clock })
    }

    pub fn log_dir(&self) -> &Path {
        &self.log_dir
    }

    /// Open (or create) the game log and append a launch banner.
    pub fn open(&self, game_id: &str, command: &[String]) -> LaunchLogHandle {
        let name = safe_name(game_id);
        let log_path = self.log_dir.join(format!("{name}.log"));
        match self.open_inner(&log_path, command) {
            Ok(file) => LaunchLogHandle {
                path: log_path,
                file: Some(file),
                error: None,
            },
            Err(error) => LaunchLogHandle {
                path: PathBuf::new(),
                file: None,
                error: Some(error),
            },
        }
    }

    fn open_inner(&self, log_path: &Path, command: &[String]) -> Result<File, String> {
        std::fs::create_dir_all(&self.log_dir).map_err(|error| error.to_string())?;
        if let Ok(metadata) = std::fs::metadata(log_path) {
            if metadata.len() > MAX_LOG_BYTES {
                let mut old = log_path.as_os_str().to_os_string();
                old.push(".old");
                let old = PathBuf::from(old);
                rotate(log_path, &old);
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .map_err(|error| error.to_string())?;
        let command_json = serde_json::to_string(command).map_err(|error| error.to_string())?;
        let header = format!(
            "\n\n=== {} launch: {} ===\n",
            format_timestamp(self.clock.now_ms()),
            command_json
        );
        file.write_all(header.as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(file)
    }
}

fn rotate(log_path: &Path, old: &Path) {
    if std::fs::rename(log_path, old).is_ok() {
        return;
    }
    // On Windows, rename cannot replace an existing `.old`; the client's
    // renameSync overwrites on POSIX.  Preserve rotation in both cases.
    let _ = std::fs::remove_file(old);
    let _ = std::fs::rename(log_path, old);
}

pub struct LaunchLogHandle {
    pub path: PathBuf,
    pub file: Option<File>,
    pub error: Option<String>,
}

impl LaunchLogHandle {
    pub fn is_open(&self) -> bool {
        self.file.is_some()
    }

    /// The Node contract is `["ignore", fd, fd]`.
    pub fn take_stdio(&mut self) -> Option<(File, File)> {
        let file = self.file.as_ref()?;
        let stdout = file.try_clone().ok()?;
        let stderr = file.try_clone().ok()?;
        Some((stdout, stderr))
    }

    pub fn close(&mut self) {
        self.file.take();
    }
}

impl Drop for LaunchLogHandle {
    fn drop(&mut self) {
        self.file.take();
    }
}

pub fn safe_name(value: &str) -> String {
    let source = if value.is_empty() { "jogo" } else { value };
    source
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

/// RFC 3339 UTC with milliseconds, matching `Date.prototype.toISOString`.
pub fn format_timestamp(epoch_ms: i64) -> String {
    let seconds = epoch_ms.div_euclid(1_000);
    let millis = epoch_ms.rem_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;

    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = if month <= 2 { year + 1 } else { year };

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "arcadia-launch-{label}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_command_and_closes_descriptor() {
        let dir = temp_dir("log");
        let clock = ManualClock::new(1_767_225_600_000); // 2026-01-01T00:00:00.000Z
        let log = LaunchLog::with_clock(&dir, Box::new(clock)).unwrap();
        let mut opened = log.open(
            "steam:440",
            &["steam".into(), "steam://rungameid/440".into()],
        );
        assert!(opened.is_open());
        assert!(opened.path.to_string_lossy().ends_with("steam_440.log"));
        let text = std::fs::read_to_string(&opened.path).unwrap();
        assert!(text.contains("2026-01-01T00:00:00.000Z"));
        assert!(text.contains("launch: [\"steam\",\"steam://rungameid/440\"]"));
        assert!(opened.take_stdio().is_some());
        opened.close();
        opened.close();
        assert!(!opened.is_open());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rotates_logs_over_five_mebibytes() {
        let dir = temp_dir("rotate");
        let file = dir.join("game.log");
        let handle = File::create(&file).unwrap();
        handle.set_len(MAX_LOG_BYTES + 1).unwrap();
        drop(handle);
        let log = LaunchLog::new(&dir).unwrap();
        let mut opened = log.open("game", &["game".into()]);
        assert!(Path::new(&format!("{}.old", file.display())).exists());
        assert!(opened.path.exists());
        opened.close();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn safe_name_replaces_unsafe_characters() {
        assert_eq!(safe_name("steam:440"), "steam_440");
        assert_eq!(safe_name(""), "jogo");
        assert_eq!(safe_name("a/b\\c d"), "a_b_c_d");
        assert_eq!(safe_name("A.b_c-d"), "A.b_c-d");
    }

    #[test]
    fn timestamp_is_iso_utc_with_millis() {
        assert_eq!(format_timestamp(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            format_timestamp(1_767_225_600_123),
            "2026-01-01T00:00:00.123Z"
        );
        assert_eq!(
            format_timestamp(1_709_164_800_000),
            "2024-02-29T00:00:00.000Z"
        );
    }

    #[test]
    fn empty_log_dir_is_rejected() {
        assert!(LaunchLog::new("").is_err());
    }
}
