//! Platform-parametric path helpers.
//!
//! The Electron client runs on Windows and Linux.  Several pure helpers in it
//! use Node's `path` module, whose separator is chosen by the *host* platform.
//! Rust code that must be testable on either host therefore accepts a
//! [`Platform`] explicitly instead of calling `std::path` directly.  String
//! paths also keep the POSIX-only Linux contracts (Steam, UMU, systemd,
//! gamescope) deterministic when the test suite runs on Windows.

use serde::{Deserialize, Serialize};

/// Target platform for a launch decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Linux,
}

impl Platform {
    /// The platform this build is running on.
    pub fn current() -> Self {
        if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }

    pub fn is_windows(self) -> bool {
        self == Platform::Windows
    }

    pub fn is_unix(self) -> bool {
        !self.is_windows()
    }

    pub fn separator(self) -> char {
        match self {
            Platform::Windows => '\\',
            Platform::Linux => '/',
        }
    }

    pub fn path_list_separator(self) -> char {
        match self {
            Platform::Windows => ';',
            Platform::Linux => ':',
        }
    }

    /// `path.join` semantics for the target platform.
    pub fn join(self, parts: &[&str]) -> String {
        join(self.separator(), parts)
    }

    /// `path.basename` semantics for the target platform.
    pub fn basename(self, value: &str) -> String {
        match self {
            Platform::Windows => {
                let normalized = value.replace('/', "\\");
                basename(&normalized, '\\')
            }
            Platform::Linux => basename(value, '/'),
        }
    }

    /// `path.dirname` semantics for the target platform.
    pub fn dirname(self, value: &str) -> String {
        match self {
            Platform::Windows => {
                let normalized = value.replace('/', "\\");
                dirname(&normalized, '\\')
            }
            Platform::Linux => dirname(value, '/'),
        }
    }

    /// True when `Path::is_absolute` would be true on the target platform.
    pub fn is_absolute(self, value: &str) -> bool {
        match self {
            Platform::Windows => {
                let bytes = value.as_bytes();
                (bytes.len() >= 3
                    && bytes[0].is_ascii_alphabetic()
                    && bytes[1] == b':'
                    && (bytes[2] == b'\\' || bytes[2] == b'/'))
                    || value.starts_with("\\\\")
            }
            Platform::Linux => value.starts_with('/'),
        }
    }

    /// `path.resolve(base, value)` with a caller supplied base directory.
    pub fn resolve(self, base: &str, value: &str) -> String {
        let (base, value) = match self {
            Platform::Windows => (base.replace('/', "\\"), value.replace('/', "\\")),
            Platform::Linux => (base.to_string(), value.to_string()),
        };
        if self.is_absolute(&value) {
            normalize(&value, self.separator())
        } else {
            let joined = self.join(&[&base, &value]);
            normalize(&joined, self.separator())
        }
    }
}

/// Lower-case and strip one trailing `.exe`.
pub fn strip_exe(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    lower
        .strip_suffix(".exe")
        .map(|v| v.to_string())
        .unwrap_or(lower)
}

/// Strip only a lower-case `.exe`, mirroring `String(value).replace(/\.exe$/, "")`
/// in the Electron launch policy.
pub fn strip_lower_exe(value: &str) -> String {
    value
        .strip_suffix(".exe")
        .map(|v| v.to_string())
        .unwrap_or_else(|| value.to_string())
}

fn join(separator: char, parts: &[&str]) -> String {
    let mut output = String::new();
    for part in parts {
        if part.is_empty() {
            continue;
        }
        if output.is_empty() {
            output.push_str(part);
            continue;
        }
        while output.ends_with(separator) {
            output.pop();
        }
        let leading = part.trim_start_matches(separator);
        if output.is_empty() && separator == '/' {
            output.push('/');
        } else if !output.is_empty() {
            output.push(separator);
        }
        output.push_str(leading);
    }
    output
}

fn basename(value: &str, separator: char) -> String {
    let trimmed = trim_trailing_separators(value, separator);
    if trimmed.is_empty() {
        return String::new();
    }
    match trimmed.rfind(separator) {
        Some(index) => trimmed[index + separator.len_utf8()..].to_string(),
        None => trimmed.to_string(),
    }
}

fn dirname(value: &str, separator: char) -> String {
    let trimmed = trim_trailing_separators(value, separator);
    if trimmed.is_empty() {
        return String::new();
    }
    match trimmed.rfind(separator) {
        Some(0) => separator.to_string(),
        Some(index) => {
            if index == 2
                && trimmed.as_bytes().get(1) == Some(&b':')
                && trimmed.as_bytes().get(2) == Some(&(separator as u8))
            {
                trimmed[..3].to_string()
            } else {
                trimmed[..index].to_string()
            }
        }
        None => ".".to_string(),
    }
}

fn trim_trailing_separators(value: &str, separator: char) -> &str {
    if value.chars().all(|c| c == separator) {
        return value;
    }
    value.trim_end_matches(separator)
}

fn normalize(value: &str, separator: char) -> String {
    let (root, rest) = split_root(value, separator);
    let mut stack: Vec<&str> = Vec::new();
    for component in rest.split(separator) {
        match component {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    let joined = stack.join(&separator.to_string());
    if root.is_empty() {
        if joined.is_empty() {
            ".".to_string()
        } else {
            joined
        }
    } else if root.ends_with(separator) {
        format!("{root}{joined}")
    } else {
        format!("{root}{separator}{joined}")
    }
}

fn split_root(value: &str, separator: char) -> (String, &str) {
    if separator == '\\' {
        let bytes = value.as_bytes();
        if bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/')
        {
            return (
                format!("{}:\\", (bytes[0] as char).to_ascii_uppercase()),
                &value[3..],
            );
        }
        if let Some(stripped) = value.strip_prefix("\\\\") {
            return ("\\\\".to_string(), stripped);
        }
        (String::new(), value)
    } else if let Some(rest) = value.strip_prefix('/') {
        ("/".to_string(), rest)
    } else {
        (String::new(), value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_like_node_on_both_platforms() {
        assert_eq!(
            Platform::Linux.join(&["/home/z", ".steam", "steam"]),
            "/home/z/.steam/steam"
        );
        assert_eq!(Platform::Linux.join(&["/", "run", "user"]), "/run/user");
        assert_eq!(
            Platform::Windows.join(&["C:\\", "Program Files (x86)", "Steam"]),
            "C:\\Program Files (x86)\\Steam"
        );
        assert_eq!(
            Platform::Windows.join(&["D:\\Steam", "steamapps"]),
            "D:\\Steam\\steamapps"
        );
    }

    #[test]
    fn basename_and_dirname() {
        assert_eq!(Platform::Linux.basename("/usr/bin/steam"), "steam");
        assert_eq!(
            Platform::Windows.basename("C:\\Games\\steam.exe"),
            "steam.exe"
        );
        assert_eq!(Platform::Linux.dirname("/games/foo.exe"), "/games");
        assert_eq!(Platform::Windows.dirname("C:\\Games\\foo.exe"), "C:\\Games");
        assert_eq!(Platform::Windows.dirname("C:\\foo.exe"), "C:\\");
        assert_eq!(Platform::Linux.dirname("foo"), ".");
    }

    #[test]
    fn resolves_paths() {
        assert_eq!(
            Platform::Linux.resolve("/games", "demo.exe"),
            "/games/demo.exe"
        );
        assert_eq!(
            Platform::Linux.resolve("/games", "/opt/demo.exe"),
            "/opt/demo.exe"
        );
        assert_eq!(
            Platform::Windows.resolve("C:\\Games", "demo.exe"),
            "C:\\Games\\demo.exe"
        );
        assert_eq!(
            Platform::Windows.resolve("C:\\Games", "D:\\Other\\demo.exe"),
            "D:\\Other\\demo.exe"
        );
    }
}
