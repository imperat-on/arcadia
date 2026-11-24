//! Launcher nativo do Arcadia no Windows.
//!
//! `arcadia-native.exe` importa as DLLs do GStreamer estaticamente. O Windows
//! resolve esses imports antes de qualquer código Rust rodar, então as DLLs
//! precisam estar no `PATH` (ou ao lado do executável) já no `CreateProcess`.
//! Um `.exe` não consegue corrigir isso depois de iniciado.
//!
//! Este binário é o ponto de entrada do pacote: ele monta `PATH`/`GST_*`
//! apontando para o runtime embutido em `gstreamer\`, define as variáveis do
//! modo portátil ou instalado e só então inicia `arcadia-native.exe`.
//!
//! Fora do Windows ele apenas inicia o binário ao lado (não é empacotado).

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

const APP_BIN: &str = if cfg!(windows) {
    "arcadia-native.exe"
} else {
    "arcadia-native"
};
const INSTALLED_MARKER: &str = "installed.marker";
const GSTREAMER_DIR: &str = "gstreamer";
const GSTREAMER_BIN_DIR: &str = "bin";
const GSTREAMER_PLUGINS_DIR: &str = "gstreamer-1.0";
const GSTREAMER_SCANNER: &str = "gst-plugin-scanner.exe";
#[cfg(windows)]
const GSTREAMER_CORE_DLL: &str = "gobject-2.0-0.dll";

/// Ambiente e alvo calculados antes de criar o processo do app.
///
/// É uma função pura sobre o diretório do launcher para que os testes possam
/// validar a montagem sem abrir nenhuma janela.
#[derive(Debug, PartialEq, Eq)]
struct LaunchPlan {
    executable: PathBuf,
    working_dir: PathBuf,
    env: Vec<(OsString, OsString)>,
}

#[derive(Debug, PartialEq, Eq)]
enum PlanError {
    MissingApp(PathBuf),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingApp(path) => write!(
                formatter,
                "O executável do Arcadia não foi encontrado:\n{}\n\nExtraia o pacote completo antes de abrir o Arcadia.exe.",
                path.display()
            ),
        }
    }
}

fn build_plan(root: &Path) -> Result<LaunchPlan, PlanError> {
    let executable = root.join(APP_BIN);
    if !executable.is_file() {
        return Err(PlanError::MissingApp(executable));
    }

    let mut env = Vec::new();
    let gstreamer = root.join(GSTREAMER_DIR);
    let gstreamer_bin = gstreamer.join(GSTREAMER_BIN_DIR);
    let gstreamer_plugins = gstreamer.join("lib").join(GSTREAMER_PLUGINS_DIR);

    if gstreamer_bin.is_dir() {
        env.push((OsString::from("PATH"), prepend_path(&gstreamer_bin)));
    }

    if gstreamer_plugins.is_dir() {
        let plugins = gstreamer_plugins.into_os_string();
        // Os três nomes são usados por versões diferentes do GStreamer/plugins
        // e pelo próprio scanner. Manter os três isola o bundle do host.
        env.push((OsString::from("GST_PLUGIN_PATH"), plugins.clone()));
        env.push((OsString::from("GST_PLUGIN_PATH_1_0"), plugins.clone()));
        env.push((OsString::from("GST_PLUGIN_SYSTEM_PATH_1_0"), plugins));
    }

    let scanner_libexec = gstreamer
        .join("libexec")
        .join(GSTREAMER_PLUGINS_DIR)
        .join(GSTREAMER_SCANNER);
    let scanner_bin = gstreamer_bin.join(GSTREAMER_SCANNER);
    if scanner_libexec.is_file() {
        env.push((
            OsString::from("GST_PLUGIN_SCANNER"),
            scanner_libexec.into_os_string(),
        ));
    } else if scanner_bin.is_file() {
        env.push((
            OsString::from("GST_PLUGIN_SCANNER"),
            scanner_bin.into_os_string(),
        ));
    }

    if root.join(INSTALLED_MARKER).is_file() {
        // O instalador cria o marcador; o updater usa a presença dele para
        // selecionar o canal NSIS em vez de tratar a pasta como portátil.
        env.push((OsString::from("ARCADIA_PACKAGED"), OsString::from("1")));
    } else {
        env.push((
            OsString::from("PORTABLE_EXECUTABLE_DIR"),
            root.as_os_str().to_owned(),
        ));
        env.push((
            OsString::from("PORTABLE_EXECUTABLE_FILE"),
            executable.as_os_str().to_owned(),
        ));
    }

    Ok(LaunchPlan {
        executable,
        working_dir: root.to_path_buf(),
        env,
    })
}

/// Prepõe `dir` ao `PATH` herdado, sem perder as entradas existentes.
fn prepend_path(dir: &Path) -> OsString {
    let mut entries = vec![dir.to_path_buf()];
    if let Some(path) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(entries).unwrap_or_else(|_| dir.as_os_str().to_owned())
}

/// O runtime pode estar embutido, no `PATH` do sistema ou em `System32`.
/// Quando não está em nenhum deles, o app morreria no loader com a mensagem
/// opaca do Windows; aqui conseguimos explicar o que falta.
#[cfg(windows)]
fn gstreamer_available(root: &Path) -> bool {
    if root
        .join(GSTREAMER_DIR)
        .join(GSTREAMER_BIN_DIR)
        .join(GSTREAMER_CORE_DLL)
        .is_file()
    {
        return true;
    }

    let mut directories: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    // O loader também procura no diretório do executável e em %SystemRoot%
    // antes do PATH; espelhar isso evita um falso negativo em instalações
    // fora do bundle.
    directories.push(root.to_path_buf());
    directories.push(root.join(GSTREAMER_DIR).join(GSTREAMER_BIN_DIR));
    if let Some(windows_dir) = std::env::var_os("SystemRoot") {
        directories.push(Path::new(&windows_dir).to_path_buf());
        directories.push(Path::new(&windows_dir).join("System32"));
    }
    directories
        .into_iter()
        .any(|directory| directory.join(GSTREAMER_CORE_DLL).is_file())
}

#[cfg(windows)]
fn missing_gstreamer_message(root: &Path) -> String {
    format!(
        "O runtime do GStreamer não foi encontrado.\n\nEsperado: {}\n\nExtraia o zip completo (não mova só o Arcadia.exe) ou instale o GStreamer 1.28 MSVC x86_64 e adicione a pasta bin ao PATH.",
        root.join(GSTREAMER_DIR).join(GSTREAMER_BIN_DIR).join(GSTREAMER_CORE_DLL).display()
    )
}

fn run(root: &Path) -> Result<(), String> {
    let plan = build_plan(root).map_err(|error| error.to_string())?;

    #[cfg(windows)]
    if !gstreamer_available(root) {
        return Err(missing_gstreamer_message(root));
    }

    let mut command = Command::new(&plan.executable);
    command
        .args(std::env::args_os().skip(1))
        .current_dir(&plan.working_dir);
    for (key, value) in &plan.env {
        command.env(key, value);
    }
    // O app já foi empacotado como subsistema console em versões antigas. Sem
    // esta flag, o Windows abre uma janela de console atrás da interface.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Não foi possível iniciar o Arcadia: {error}"))
}

fn main() {
    let root = std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));

    if let Err(error) = run(&root) {
        report_error(&error);
        std::process::exit(1);
    }
}

fn report_error(message: &str) {
    eprintln!("[arcadia-launcher] {message}");
    #[cfg(windows)]
    windows_message_box::error(message);
}

#[cfg(windows)]
mod windows_message_box {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(
            window: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            message_type: u32,
        ) -> i32;
    }

    pub fn error(message: &str) {
        let text = wide(message);
        let caption = wide("Arcadia");
        // MB_OK | MB_ICONERROR
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                0x0000_0010,
            );
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arcadia-launcher-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"test").unwrap();
    }

    fn gst_root(root: &Path) -> PathBuf {
        root.join(GSTREAMER_DIR)
    }

    fn gst_bin(root: &Path) -> PathBuf {
        gst_root(root).join(GSTREAMER_BIN_DIR)
    }

    fn gst_plugins(root: &Path) -> PathBuf {
        gst_root(root).join("lib").join(GSTREAMER_PLUGINS_DIR)
    }

    fn gst_scanner_libexec(root: &Path) -> PathBuf {
        gst_root(root)
            .join("libexec")
            .join(GSTREAMER_PLUGINS_DIR)
            .join(GSTREAMER_SCANNER)
    }

    fn gst_scanner_bin(root: &Path) -> PathBuf {
        gst_bin(root).join(GSTREAMER_SCANNER)
    }

    fn env_value<'a>(plan: &'a LaunchPlan, key: &str) -> Option<&'a OsString> {
        plan.env
            .iter()
            .find_map(|(name, value)| (name == key).then_some(value))
    }

    #[test]
    fn missing_app_is_an_actionable_error() {
        let root = scratch("missing");
        let error = build_plan(&root).unwrap_err();
        assert!(matches!(error, PlanError::MissingApp(_)));
        assert!(error.to_string().contains(APP_BIN));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn portable_layout_sets_gstreamer_and_portable_env() {
        let root = scratch("portable");
        touch(&root.join(APP_BIN));
        touch(&gst_bin(&root).join("gstreamer-1.0-0.dll"));
        touch(&gst_plugins(&root).join("libgstcoreelements.dll"));
        touch(&gst_scanner_libexec(&root));

        let plan = build_plan(&root).expect("plano portátil");

        assert_eq!(plan.executable, root.join(APP_BIN));
        assert_eq!(plan.working_dir, root);

        let path = env_value(&plan, "PATH").expect("PATH");
        let first = std::env::split_paths(path).next().unwrap();
        assert_eq!(first, gst_bin(&root));

        assert_eq!(
            env_value(&plan, "GST_PLUGIN_PATH"),
            Some(&gst_plugins(&root).into_os_string())
        );
        assert_eq!(
            env_value(&plan, "GST_PLUGIN_PATH_1_0"),
            Some(&gst_plugins(&root).into_os_string())
        );
        assert_eq!(
            env_value(&plan, "GST_PLUGIN_SYSTEM_PATH_1_0"),
            Some(&gst_plugins(&root).into_os_string())
        );
        assert_eq!(
            env_value(&plan, "GST_PLUGIN_SCANNER"),
            Some(&gst_scanner_libexec(&root).into_os_string())
        );
        assert_eq!(
            env_value(&plan, "PORTABLE_EXECUTABLE_DIR"),
            Some(&root.as_os_str().to_owned())
        );
        assert_eq!(
            env_value(&plan, "PORTABLE_EXECUTABLE_FILE"),
            Some(&root.join(APP_BIN).into_os_string())
        );
        assert!(env_value(&plan, "ARCADIA_PACKAGED").is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn installed_marker_selects_packaged_channel() {
        let root = scratch("installed");
        touch(&root.join(APP_BIN));
        touch(&root.join(INSTALLED_MARKER));

        let plan = build_plan(&root).expect("plano instalado");
        assert_eq!(
            env_value(&plan, "ARCADIA_PACKAGED"),
            Some(&OsString::from("1"))
        );
        assert!(env_value(&plan, "PORTABLE_EXECUTABLE_DIR").is_none());
        assert!(env_value(&plan, "PORTABLE_EXECUTABLE_FILE").is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn scanner_falls_back_to_bin_when_libexec_is_absent() {
        let root = scratch("scanner");
        touch(&root.join(APP_BIN));
        touch(&gst_scanner_bin(&root));

        let plan = build_plan(&root).expect("plano com scanner em bin");
        assert_eq!(
            env_value(&plan, "GST_PLUGIN_SCANNER"),
            Some(&gst_scanner_bin(&root).into_os_string())
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_without_gstreamer_keeps_host_env() {
        let root = scratch("no-gst");
        touch(&root.join(APP_BIN));

        let plan = build_plan(&root).expect("plano sem GStreamer");
        assert!(env_value(&plan, "PATH").is_none());
        assert!(env_value(&plan, "GST_PLUGIN_PATH").is_none());
        assert!(env_value(&plan, "GST_PLUGIN_SCANNER").is_none());

        let _ = fs::remove_dir_all(root);
    }
}
