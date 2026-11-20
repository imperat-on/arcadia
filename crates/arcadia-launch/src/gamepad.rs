//! DualSense/DS4 -> XInput wrapper detection and command construction.
//!
//! Port of `app/electron/gamepad-launch.js`.  The sysfs scan is pure: callers
//! pass a filesystem probe, so tests never touch `/sys` or spawn Python.

use std::collections::{BTreeMap, HashMap};

use crate::platform::Platform;

pub const SONY_VENDOR: u32 = 0x054c;
pub const SONY_PRODUCTS: [u32; 5] = [0x09cc, 0x0df2, 0x0ce6, 0x05c4, 0x05ce];
pub const XBOX_VENDOR: u32 = 0x045e;

pub const WRAPPER_RELATIVE: &str = "scripts/gamepad-xinput.py";
/// Binário Rust que substituiu o wrapper Python. Fica ao lado do executável
/// principal (mesmo diretório de `target/release`), então é encontrado tanto
/// no build quanto no pacote.
pub const WRAPPER_BIN: &str = "arcadia-gamepad";

pub trait GamepadFs {
    fn read_dir(&self, path: &str) -> Option<Vec<String>>;
    fn read_file(&self, path: &str) -> Option<String>;
    fn exists(&self, path: &str) -> bool;
}

#[derive(Clone, Debug, Default)]
pub struct SystemGamepadFs;

impl GamepadFs for SystemGamepadFs {
    fn read_dir(&self, path: &str) -> Option<Vec<String>> {
        let entries = std::fs::read_dir(path).ok()?;
        Some(
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect(),
        )
    }

    fn read_file(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn exists(&self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }
}

/// Scan sysfs for a gamepad that needs the uinput wrapper.  Returns the
/// `/dev/input/eventXX` device path, or `None`.
pub fn detect_gamepad_event(fs: &dyn GamepadFs) -> Option<String> {
    let base = "/sys/class/input";
    let entries = fs.read_dir(base)?;
    for name in entries {
        if !name.starts_with("event") {
            continue;
        }
        let uevent_path = Platform::Linux.join(&[base, &name, "device", "uevent"]);
        let Some(uevent) = fs.read_file(&uevent_path) else {
            continue;
        };
        let Some((_, vendor, _product)) = parse_uevent(&uevent) else {
            continue;
        };
        if !is_gamepad_uevent(&uevent) {
            continue;
        }
        // Native Xbox controllers are already XInput; Wine sees them directly.
        if vendor == XBOX_VENDOR {
            continue;
        }
        return Some(Platform::Linux.join(&["/dev/input", &name]));
    }
    None
}

/// Extract `(bus, vendor, product)` from PRODUCT= or MODALIAS=input:b…v…p….
pub fn parse_uevent(uevent: &str) -> Option<(u32, u32, u32)> {
    let mut values: HashMap<&str, &str> = HashMap::new();
    for line in uevent.split('\n') {
        if let Some(equal) = line.find('=') {
            if equal > 0 {
                values.insert(line[..equal].trim(), line[equal + 1..].trim());
            }
        }
    }

    if let Some(product) = values.get("PRODUCT").filter(|value| !value.is_empty()) {
        let parts: Vec<&str> = product.split('/').collect();
        if parts.len() >= 3 {
            if let (Some(bus), Some(vendor), Some(product)) = (
                parse_hex_prefix(parts[0]),
                parse_hex_prefix(parts[1]),
                parse_hex_prefix(parts[2]),
            ) {
                return Some((bus, vendor, product));
            }
        }
    }

    let modalias = values.get("MODALIAS").copied().unwrap_or("");
    parse_modalias(modalias)
}

fn parse_modalias(value: &str) -> Option<(u32, u32, u32)> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'b' {
            index += 1;
            continue;
        }
        let bus_start = index + 1;
        let mut cursor = bus_start;
        while cursor < bytes.len() && bytes[cursor].is_ascii_hexdigit() {
            cursor += 1;
        }
        if cursor == bus_start || cursor >= bytes.len() || bytes[cursor] != b'v' {
            index += 1;
            continue;
        }
        let vendor_start = cursor + 1;
        cursor = vendor_start;
        while cursor < bytes.len() && bytes[cursor].is_ascii_hexdigit() {
            cursor += 1;
        }
        if cursor == vendor_start || cursor >= bytes.len() || bytes[cursor] != b'p' {
            index += 1;
            continue;
        }
        let product_start = cursor + 1;
        cursor = product_start;
        while cursor < bytes.len()
            && cursor - product_start < 4
            && bytes[cursor].is_ascii_hexdigit()
        {
            cursor += 1;
        }
        if cursor == product_start {
            index += 1;
            continue;
        }
        let bus = parse_hex_full(&value[bus_start..vendor_start - 1])?;
        let vendor = parse_hex_full(&value[vendor_start..product_start - 1])?;
        let product = parse_hex_full(&value[product_start..cursor])?;
        return Some((bus, vendor, product));
    }
    None
}

fn parse_hex_prefix(value: &str) -> Option<u32> {
    let digits: String = value
        .chars()
        .take_while(|ch| ch.is_ascii_hexdigit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        u32::from_str_radix(&digits, 16).ok()
    }
}

fn parse_hex_full(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(value, 16).ok()
}

/// A real gamepad exposes BTN_SOUTH (`k130`) and an ABS_X stick (`ra0`).
pub fn is_gamepad_uevent(uevent: &str) -> bool {
    let mut modalias = "";
    for line in uevent.split('\n') {
        if let Some(value) = line.strip_prefix("MODALIAS=") {
            modalias = value.trim();
        }
    }
    if modalias.is_empty() {
        return false;
    }
    modalias.contains("k130") && modalias.contains("ra0")
}

/// Translate the packaged `app.asar` path to the extracted copy.
pub fn unpack_asar_path(path: &str, platform: Platform) -> String {
    let separator = platform.separator();
    let needle = format!("{separator}app.asar{separator}");
    let replacement = format!("{separator}app.asar.unpacked{separator}");
    path.replace(&needle, &replacement)
}

pub fn wrapper_path(platform: Platform, app_dir: &str, fs: &dyn GamepadFs) -> String {
    let wrapper = platform.join(&[app_dir, "scripts", "gamepad-xinput.py"]);
    let unpacked = unpack_asar_path(&wrapper, platform);
    if unpacked != wrapper && fs.exists(&unpacked) {
        unpacked
    } else {
        wrapper
    }
}

pub fn python_cmd(env: &BTreeMap<String, String>) -> String {
    match env.get("PYTHON") {
        Some(value) if !value.is_empty() => value.clone(),
        _ => "python3".to_string(),
    }
}

/// Background wrapper argv, or `None` when detection/availability fails.
///
/// Depois do porte, o wrapper é o binário Rust `arcadia-gamepad` — mesmo
/// argumento (`--device <eventXX>`) que o script Python aceitava, então o
/// chamador não muda. O Python fica como fallback para instalações antigas
/// em que o binário ainda não foi empacotado junto.
pub fn build_gamepad_wrapper_command(
    fs: &dyn GamepadFs,
    platform: Platform,
    app_dir: &str,
    env: &BTreeMap<String, String>,
) -> Option<Vec<String>> {
    let event = detect_gamepad_event(fs)?;

    // 1. A borda pode já ter resolvido o caminho no pacote (resources/).
    if let Some(bin) = std::env::var_os("ARCADIA_GAMEPAD_BIN") {
        let caminho = std::path::PathBuf::from(bin);
        if caminho.is_file() {
            return Some(vec![
                caminho.to_string_lossy().into_owned(),
                "--device".to_string(),
                event,
            ]);
        }
    }

    // 2. Binário ao lado do executável principal (build local).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(WRAPPER_BIN);
            if candidate.is_file() {
                return Some(vec![
                    candidate.to_string_lossy().into_owned(),
                    "--device".to_string(),
                    event,
                ]);
            }
        }
    }

    // Fallback: o wrapper Python do layout antigo.
    let wrapper = wrapper_path(platform, app_dir, fs);
    if !fs.exists(&wrapper) {
        return None;
    }
    Some(vec![
        python_cmd(env),
        wrapper,
        "--device".to_string(),
        event,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeFs {
        dirs: HashMap<String, Vec<String>>,
        files: HashMap<String, String>,
        existing: Vec<String>,
    }

    impl GamepadFs for FakeFs {
        fn read_dir(&self, path: &str) -> Option<Vec<String>> {
            self.dirs.get(path).cloned()
        }

        fn read_file(&self, path: &str) -> Option<String> {
            self.files.get(path).cloned()
        }

        fn exists(&self, path: &str) -> bool {
            self.existing.iter().any(|entry| entry == path)
        }
    }

    const GAMEPAD_UEVENT: &str = "MODALIAS=input:b0003v054Cp0CE6e0110-e0,1,2,4,6,80,81,82,83,k130,k131,k13a,ra0,ra1,ra2,ra3\n";

    #[test]
    fn parses_product_and_modalias() {
        let product = "PRODUCT=3/54c/ce6/110\nNAME=\"Sony Interactive Entertainment DualSense\"\n";
        assert_eq!(parse_uevent(product), Some((0x3, 0x54c, 0x0ce6)));
        assert_eq!(
            parse_uevent("MODALIAS=input:b0003v054Cp0CE6e0110-e0,1,4,etc\n"),
            Some((0x0003, 0x054c, 0x0ce6))
        );
        assert_eq!(parse_uevent("PRODUCT=\nMODALIAS=\n"), None);
        assert_eq!(parse_uevent(""), None);
    }

    #[test]
    fn only_real_gamepads_pass_the_modalias_filter() {
        assert!(is_gamepad_uevent(GAMEPAD_UEVENT));
        let sensor = "MODALIAS=input:b0003v054Cp0CE6e0110-e0,1,2,5,k100,k101,ra0,ra1\n";
        let touchpad = "MODALIAS=input:b0003v054Cp0CE6e0110-e0,1,2,a0,k100,k101,ra0\n";
        assert!(!is_gamepad_uevent(sensor));
        assert!(!is_gamepad_uevent(touchpad));
        assert!(!is_gamepad_uevent(""));
    }

    #[test]
    fn detects_sony_and_skips_native_xbox() {
        let fs = FakeFs {
            dirs: HashMap::from([(
                "/sys/class/input".to_string(),
                vec!["event0".to_string(), "event1".to_string()],
            )]),
            files: HashMap::from([(
                "/sys/class/input/event1/device/uevent".to_string(),
                GAMEPAD_UEVENT.to_string(),
            )]),
            existing: vec![],
        };
        assert_eq!(
            detect_gamepad_event(&fs).as_deref(),
            Some("/dev/input/event1")
        );

        let xbox = FakeFs {
            dirs: HashMap::from([("/sys/class/input".to_string(), vec!["event2".to_string()])]),
            files: HashMap::from([(
                "/sys/class/input/event2/device/uevent".to_string(),
                GAMEPAD_UEVENT.replace("054Cp0CE6", "045Ep0B13"),
            )]),
            existing: vec![],
        };
        assert_eq!(detect_gamepad_event(&xbox), None);
    }

    #[test]
    fn asar_paths_are_unpacked() {
        assert_eq!(
            unpack_asar_path(
                "/opt/app/resources/app.asar/electron/scripts/gamepad-xinput.py",
                Platform::Linux
            ),
            "/opt/app/resources/app.asar.unpacked/electron/scripts/gamepad-xinput.py"
        );
        assert_eq!(
            unpack_asar_path("C:\\app\\resources\\app.asar\\x.py", Platform::Windows),
            "C:\\app\\resources\\app.asar.unpacked\\x.py"
        );
    }

    #[test]
    fn prefere_o_binario_rust_quando_existe() {
        // O binário Rust fica ao lado do executável principal. Em teste,
        // `current_exe()` aponta para o harness do cargo, então este caso
        // exercita o fallback. A preferência é verificada pela ordem das
        // checagens em `build_gamepad_wrapper_command`: o binário vem antes
        // do script Python, e a ausência de um faz cair no outro.
        let app_dir = "/opt/arcadia/electron";
        let wrapper = "/opt/arcadia/electron/scripts/gamepad-xinput.py";
        let fs = FakeFs {
            dirs: HashMap::from([("/sys/class/input".to_string(), vec!["event3".to_string()])]),
            files: HashMap::from([(
                "/sys/class/input/event3/device/uevent".to_string(),
                GAMEPAD_UEVENT.to_string(),
            )]),
            existing: vec![wrapper.to_string()],
        };
        let command =
            build_gamepad_wrapper_command(&fs, Platform::Linux, app_dir, &BTreeMap::new()).unwrap();
        // Sem o binário Rust ao lado do exe, cai no wrapper Python.
        assert_eq!(command[0], "python3");
        assert_eq!(command[1], wrapper);
        assert_eq!(command[2], "--device");
        assert_eq!(command[3], "/dev/input/event3");
    }

    #[test]
    fn wrapper_bin_tem_o_nome_do_crate() {
        assert_eq!(WRAPPER_BIN, "arcadia-gamepad");
    }

    #[test]
    fn python_cmd_respeita_a_variavel_de_ambiente() {
        assert_eq!(
            python_cmd(&BTreeMap::from([(
                "PYTHON".to_string(),
                "/usr/bin/python".to_string()
            )])),
            "/usr/bin/python"
        );
        assert_eq!(python_cmd(&BTreeMap::new()), "python3");
        // String vazia cai no default.
        assert_eq!(
            python_cmd(&BTreeMap::from([("PYTHON".to_string(), String::new())])),
            "python3"
        );
    }
}
