//! Process probing and kill-tree decisions.
//!
//! Port of `app/electron/process-tools.js` plus the ownership checks from
//! `main.js`.  All policy functions operate on a [`ProcessProbe`] so tests can
//! use a fake process table.  Nothing in this module terminates a process.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::platform::Platform;

/// One row of a process table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub command_line: String,
    pub executable_path: String,
    pub start_time: Option<u64>,
    pub pgrp: Option<u32>,
    pub state: Option<String>,
}

impl ProcessInfo {
    pub fn new(pid: u32, name: impl Into<String>) -> Self {
        Self {
            pid,
            name: name.into(),
            command_line: String::new(),
            executable_path: String::new(),
            start_time: None,
            pgrp: None,
            state: None,
        }
    }

    pub fn with_command_line(mut self, command_line: impl Into<String>) -> Self {
        self.command_line = command_line.into();
        self
    }

    pub fn with_executable_path(mut self, path: impl Into<String>) -> Self {
        self.executable_path = path.into();
        self
    }

    pub fn with_identity(mut self, pgrp: u32, start_time: u64, state: &str) -> Self {
        self.pgrp = Some(pgrp);
        self.start_time = Some(start_time);
        self.state = Some(state.to_string());
        self
    }

    fn is_zombie(&self) -> bool {
        self.state
            .as_deref()
            .is_some_and(|state| state.contains('Z') || state.contains('X'))
    }
}

/// Test seam for the machine's process table.
pub trait ProcessProbe {
    fn is_pid_alive(&self, pid: u32) -> bool;
    fn processes(&self) -> Vec<ProcessInfo>;
}

/// In-memory process table used by tests and deterministic replay.
#[derive(Clone, Debug, Default)]
pub struct StaticProcessProbe {
    processes: Vec<ProcessInfo>,
    alive: BTreeSet<u32>,
}

impl StaticProcessProbe {
    pub fn new(processes: Vec<ProcessInfo>) -> Self {
        let alive = processes
            .iter()
            .filter(|process| !process.is_zombie())
            .map(|process| process.pid)
            .collect();
        Self { processes, alive }
    }

    pub fn with_alive(mut self, pids: impl IntoIterator<Item = u32>) -> Self {
        self.alive = pids.into_iter().collect();
        self
    }

    pub fn set_alive(&mut self, pid: u32, alive: bool) {
        if alive {
            self.alive.insert(pid);
        } else {
            self.alive.remove(&pid);
        }
    }
}

impl ProcessProbe for StaticProcessProbe {
    fn is_pid_alive(&self, pid: u32) -> bool {
        self.alive.contains(&pid)
    }

    fn processes(&self) -> Vec<ProcessInfo> {
        self.processes.clone()
    }
}

/// Real machine probe.  Read-only: it never signals a process.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemProcessProbe;

impl ProcessProbe for SystemProcessProbe {
    fn is_pid_alive(&self, pid: u32) -> bool {
        if pid <= 1 {
            return false;
        }
        #[cfg(windows)]
        {
            use std::process::{Command, Stdio};
            let Ok(output) = Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
                .stdin(Stdio::null())
                .output()
            else {
                return false;
            };
            let text = String::from_utf8_lossy(&output.stdout);
            !text.trim().is_empty() && text.contains(&pid.to_string())
        }
        #[cfg(unix)]
        {
            std::path::Path::new(&format!("/proc/{pid}")).exists()
        }
        #[cfg(not(any(windows, unix)))]
        {
            false
        }
    }

    fn processes(&self) -> Vec<ProcessInfo> {
        #[cfg(windows)]
        {
            use std::process::{Command, Stdio};
            let Ok(output) = Command::new("tasklist")
                .args(["/FO", "CSV", "/NH"])
                .stdin(Stdio::null())
                .output()
            else {
                return Vec::new();
            };
            let text = String::from_utf8_lossy(&output.stdout);
            text.lines()
                .filter_map(|line| {
                    let fields = parse_csv_line(line);
                    let pid = fields.get(1)?.trim().parse::<u32>().ok()?;
                    Some(ProcessInfo::new(pid, fields.first()?.trim_matches('"')))
                })
                .collect()
        }
        #[cfg(unix)]
        {
            let mut out = Vec::new();
            let Ok(entries) = std::fs::read_dir("/proc") else {
                return out;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(pid) = name.to_str().and_then(|v| v.parse::<u32>().ok()) else {
                    continue;
                };
                let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
                    .map(|v| v.trim().to_string())
                    .unwrap_or_default();
                let command_line = std::fs::read(format!("/proc/{pid}/cmdline"))
                    .map(|bytes| {
                        String::from_utf8_lossy(&bytes)
                            .replace('\0', " ")
                            .trim()
                            .to_string()
                    })
                    .unwrap_or_default();
                out.push(
                    ProcessInfo::new(pid, comm)
                        .with_command_line(command_line)
                        .with_executable_path(format!("/proc/{pid}/exe")),
                );
            }
            out
        }
        #[cfg(not(any(windows, unix)))]
        {
            Vec::new()
        }
    }
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in line.chars() {
        match ch {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(std::mem::take(&mut current));
            }
            other => current.push(other),
        }
    }
    fields.push(current);
    fields
}

/// `isProcessAlive`: invalid PIDs are never alive.
pub fn is_process_alive(probe: &dyn ProcessProbe, pid: u32) -> bool {
    if pid <= 1 {
        return false;
    }
    probe.is_pid_alive(pid)
}

/// `isProcessRunning`: Windows compares image names case-insensitively; Unix
/// behaves like `pgrep -f` (regex over the full command line).
pub fn is_process_running(probe: &dyn ProcessProbe, pattern: &str, platform: Platform) -> bool {
    if pattern.is_empty() {
        return false;
    }
    match platform {
        Platform::Windows => {
            let needle = pattern.to_ascii_lowercase();
            probe
                .processes()
                .iter()
                .any(|process| process.name.to_ascii_lowercase() == needle)
        }
        Platform::Linux => probe.processes().iter().any(|process| {
            matches_literal_pattern(pattern, &process.command_line)
                || matches_literal_pattern(pattern, &process.name)
        }),
    }
}

/// The frozen client only passes literal substrings (and one `|` alternation)
/// to `pgrep -f`.  Match that shape without dragging a full regex engine into
/// the launch pipeline; backslash escapes are reduced to their literal byte.
fn matches_literal_pattern(pattern: &str, text: &str) -> bool {
    pattern.split('|').any(|alternative| {
        if alternative.is_empty() {
            return false;
        }
        let literal = unescape_pattern_literal(alternative);
        text.contains(&literal)
    })
}

fn unescape_pattern_literal(pattern: &str) -> String {
    let mut output = String::with_capacity(pattern.len());
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                output.push(escaped);
            }
        } else {
            output.push(ch);
        }
    }
    output
}

pub fn is_steam_running(probe: &dyn ProcessProbe, platform: Platform) -> bool {
    match platform {
        Platform::Windows => is_process_running(probe, "steam.exe", platform),
        Platform::Linux => probe
            .processes()
            .iter()
            .any(|process| process.name == "steam"),
    }
}

/// On Windows the Steam client stays in the tray after a game closes, so a
/// more specific check is needed for playtime/session teardown.
pub fn is_steam_game_running(probe: &dyn ProcessProbe, platform: Platform) -> bool {
    if platform != Platform::Windows {
        return false;
    }
    probe.processes().iter().any(|process| {
        process
            .executable_path
            .replace('\\', "/")
            .to_ascii_lowercase()
            .contains("steamapps/common")
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KillSignal {
    Term,
    Kill,
}

impl KillSignal {
    pub fn from_name(value: &str) -> Self {
        if value == "SIGKILL" || value == "9" || value == "KILL" || value == "kill" {
            KillSignal::Kill
        } else {
            KillSignal::Term
        }
    }
}

/// Concrete decision produced before any syscall/command is executed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KillTreePlan {
    Taskkill {
        pid: u32,
        force: bool,
    },
    /// Signal the process group `-pgid`.
    ProcessGroup {
        pgid: u32,
        signal: KillSignal,
    },
}

impl KillTreePlan {
    pub fn program(&self) -> &'static str {
        match self {
            KillTreePlan::Taskkill { .. } => "taskkill",
            KillTreePlan::ProcessGroup { .. } => "kill",
        }
    }

    pub fn args(&self) -> Vec<String> {
        match self {
            KillTreePlan::Taskkill { pid, force } => {
                let mut args = vec!["/PID".to_string(), pid.to_string(), "/T".to_string()];
                if *force {
                    args.push("/F".to_string());
                }
                args
            }
            KillTreePlan::ProcessGroup { pgid, signal } => vec![
                match signal {
                    KillSignal::Term => "-SIGTERM".to_string(),
                    KillSignal::Kill => "-SIGKILL".to_string(),
                },
                format!("-{pgid}"),
            ],
        }
    }
}

/// `killProcessTree` decision.  PIDs <= 1 are rejected before any action.
pub fn plan_kill_tree(pid: u32, signal: KillSignal, platform: Platform) -> Option<KillTreePlan> {
    if pid <= 1 {
        return None;
    }
    match platform {
        Platform::Windows => Some(KillTreePlan::Taskkill {
            pid,
            force: signal == KillSignal::Kill,
        }),
        Platform::Linux => Some(KillTreePlan::ProcessGroup { pgid: pid, signal }),
    }
}

/// Linux `/proc/<pid>/stat` ownership snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub state: String,
    pub pgrp: u32,
    pub starttime: u64,
}

impl ProcessIdentity {
    pub fn new(pid: u32, state: impl Into<String>, pgrp: u32, starttime: u64) -> Self {
        Self {
            pid,
            state: state.into(),
            pgrp,
            starttime,
        }
    }

    pub fn is_known_owner(&self) -> bool {
        self.pid > 1
            && self.pgrp == self.pid
            && self.starttime > 0
            && !self.state.contains('Z')
            && !self.state.contains('X')
    }

    pub fn is_terminal(&self) -> bool {
        self.state.contains('Z') || self.state.contains('X')
    }
}

/// `processIdentityMatches`: start time plus process group prove ownership.
pub fn identities_match(owned: &ProcessIdentity, current: Option<&ProcessIdentity>) -> bool {
    if !owned.is_known_owner() {
        return false;
    }
    let Some(current) = current else {
        return false;
    };
    if !current.is_known_owner() || current.is_terminal() {
        return false;
    }
    current.pgrp == owned.pgrp && current.starttime == owned.starttime && current.pgrp == owned.pid
}

/// Signal decision for an already-owned game.  Windows always falls back to
/// `taskkill` (the client does the same); Unix requires identity proof.
pub fn plan_owned_signal(
    platform: Platform,
    owned: &ProcessIdentity,
    current: Option<&ProcessIdentity>,
    signal: KillSignal,
) -> Option<KillTreePlan> {
    if owned.pid <= 1 {
        return None;
    }
    match platform {
        Platform::Windows => Some(KillTreePlan::Taskkill {
            pid: owned.pid,
            force: signal == KillSignal::Kill,
        }),
        Platform::Linux => {
            if !identities_match(owned, current) {
                return None;
            }
            Some(KillTreePlan::ProcessGroup {
                pgid: owned.pid,
                signal,
            })
        }
    }
}

pub fn steam_app_id(game_id: &str) -> Option<String> {
    let digits = game_id.strip_prefix("steam:")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(digits.to_string())
}

pub fn steam_stop_uri(app_id: &str) -> String {
    format!("steam://stopgameid/{app_id}")
}

/// How the client asks Steam to stop a game.  Windows has no protocol-handler
/// resolution in `spawn`, so the URI goes through `cmd /C start`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SteamStopPlan {
    WindowsProtocol { uri: String },
    Unix { program: String, args: Vec<String> },
}

impl SteamStopPlan {
    pub fn command_line(&self) -> String {
        match self {
            SteamStopPlan::WindowsProtocol { uri } => format!("start \"\" \"{uri}\""),
            SteamStopPlan::Unix { program, args } => std::iter::once(program.clone())
                .chain(args.iter().cloned())
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
}

pub fn plan_steam_stop(
    platform: Platform,
    steam_command: &str,
    game_id: &str,
) -> Option<SteamStopPlan> {
    let app_id = steam_app_id(game_id)?;
    let uri = steam_stop_uri(&app_id);
    match platform {
        Platform::Windows => Some(SteamStopPlan::WindowsProtocol { uri }),
        Platform::Linux => Some(SteamStopPlan::Unix {
            program: steam_command.to_string(),
            args: vec![uri],
        }),
    }
}

/// Small helper used by the session machine to snapshot several probes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessLookup {
    pub by_pid: BTreeMap<u32, ProcessInfo>,
}

impl ProcessLookup {
    pub fn from_probe(probe: &dyn ProcessProbe) -> Self {
        Self {
            by_pid: probe
                .processes()
                .into_iter()
                .map(|process| (process.pid, process))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> StaticProcessProbe {
        StaticProcessProbe::new(vec![
            ProcessInfo::new(10, "explorer.exe").with_executable_path("C:\\Windows\\explorer.exe"),
            ProcessInfo::new(20, "steam.exe")
                .with_executable_path("C:\\Program Files (x86)\\Steam\\steam.exe"),
            ProcessInfo::new(30, "game.exe")
                .with_executable_path("D:\\SteamLibrary\\steamapps\\common\\Game\\game.exe"),
            ProcessInfo::new(40, "steam")
                .with_command_line("/usr/bin/steam -silent steam://rungameid/440"),
            ProcessInfo::new(50, "game").with_command_line("/games/steamapps/common/Game/game"),
        ])
    }

    #[test]
    fn invalid_pids_are_never_alive() {
        let probe = table();
        assert!(!is_process_alive(&probe, 0));
        assert!(!is_process_alive(&probe, 1));
        assert!(is_process_alive(&probe, 20));
        assert!(!is_process_alive(&probe, 999));
    }

    #[test]
    fn detects_running_patterns_on_both_platforms() {
        let probe = table();
        assert!(is_process_running(&probe, "steam.exe", Platform::Windows));
        assert!(is_process_running(
            &probe,
            r"steamapps/common/|lutris/runners",
            Platform::Linux
        ));
        assert!(!is_process_running(
            &probe,
            r"not-there|also-not",
            Platform::Linux
        ));
    }

    #[test]
    fn steam_client_and_game_detection() {
        let probe = table();
        assert!(is_steam_running(&probe, Platform::Windows));
        assert!(is_steam_running(&probe, Platform::Linux));
        assert!(is_steam_game_running(&probe, Platform::Windows));
        assert!(!is_steam_game_running(&probe, Platform::Linux));
    }

    #[test]
    fn kill_tree_decisions_match_taskkill_and_group_semantics() {
        assert_eq!(plan_kill_tree(0, KillSignal::Term, Platform::Windows), None);
        assert_eq!(
            plan_kill_tree(20, KillSignal::Term, Platform::Windows),
            Some(KillTreePlan::Taskkill {
                pid: 20,
                force: false
            })
        );
        assert_eq!(
            plan_kill_tree(20, KillSignal::Kill, Platform::Windows)
                .unwrap()
                .args(),
            vec!["/PID", "20", "/T", "/F"]
        );
        assert_eq!(
            plan_kill_tree(20, KillSignal::Term, Platform::Linux),
            Some(KillTreePlan::ProcessGroup {
                pgid: 20,
                signal: KillSignal::Term
            })
        );
    }

    #[test]
    fn ownership_requires_matching_start_time_and_group() {
        let owned = ProcessIdentity::new(20, "S", 20, 555);
        assert!(owned.is_known_owner());
        assert!(identities_match(
            &owned,
            Some(&ProcessIdentity::new(20, "S", 20, 555))
        ));
        assert!(!identities_match(
            &owned,
            Some(&ProcessIdentity::new(20, "S", 20, 999))
        ));
        assert!(!identities_match(
            &owned,
            Some(&ProcessIdentity::new(20, "S", 21, 555))
        ));
        assert!(!identities_match(&owned, None));
        assert_eq!(
            plan_owned_signal(
                Platform::Linux,
                &owned,
                Some(&ProcessIdentity::new(20, "S", 20, 555)),
                KillSignal::Term
            ),
            Some(KillTreePlan::ProcessGroup {
                pgid: 20,
                signal: KillSignal::Term
            })
        );
        assert_eq!(
            plan_owned_signal(
                Platform::Linux,
                &owned,
                Some(&ProcessIdentity::new(20, "S", 20, 1)),
                KillSignal::Term
            ),
            None
        );
    }

    #[test]
    fn steam_stop_never_uses_a_name_pattern() {
        assert_eq!(steam_app_id("steam:440"), Some("440".into()));
        assert_eq!(steam_app_id("custom:440"), None);
        assert_eq!(
            plan_steam_stop(Platform::Linux, "/usr/bin/steam", "steam:440"),
            Some(SteamStopPlan::Unix {
                program: "/usr/bin/steam".into(),
                args: vec!["steam://stopgameid/440".into()]
            })
        );
        assert_eq!(
            plan_steam_stop(Platform::Windows, "steam.exe", "steam:440")
                .unwrap()
                .command_line(),
            "start \"\" \"steam://stopgameid/440\""
        );
    }
}
