//! Seletor nativo de arquivos/pastas compartilhado pelas telas.
//!
//! O diálogo real fica atrás de [`pick`] e nunca é aberto em teste. No Linux
//! usamos `zenity` ou `kdialog`; não dependemos de `rfd`/GTK porque o workspace
//! também contém a UI Tauri, cujo plugin de diálogo habilita a feature `gtk3`
//! do `rfd` e conflita com o backend XDG.

use std::path::PathBuf;

/// O que o seletor está escolhendo. O id do emulador acompanha os alvos de
/// emulação para que um resultado atrasado não preencha o editor de outro
/// emulador.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickPurpose {
    Avatar,
    Background,
    Banner,
    EmulatorExecutable(String),
    EmulatorCore(String),
    EmulatorBios(String),
    RomFolder(String),
    /// Pronto para a fatia de lançamento por emulador; hoje não há campo de ROM
    /// única no editor nativo.
    #[allow(dead_code)]
    RomFile(String),
}

/// Campo/estado de destino de um [`PickPurpose`], independente do diálogo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickTarget {
    Avatar,
    Background,
    Banner,
    EmulatorExecutable,
    EmulatorCore,
    EmulatorBios,
    RomFolder,
    RomFile,
}

impl PickPurpose {
    pub fn target(&self) -> PickTarget {
        match self {
            Self::Avatar => PickTarget::Avatar,
            Self::Background => PickTarget::Background,
            Self::Banner => PickTarget::Banner,
            Self::EmulatorExecutable(_) => PickTarget::EmulatorExecutable,
            Self::EmulatorCore(_) => PickTarget::EmulatorCore,
            Self::EmulatorBios(_) => PickTarget::EmulatorBios,
            Self::RomFolder(_) => PickTarget::RomFolder,
            Self::RomFile(_) => PickTarget::RomFile,
        }
    }

    pub fn emulator_id(&self) -> Option<&str> {
        match self {
            Self::EmulatorExecutable(id)
            | Self::EmulatorCore(id)
            | Self::EmulatorBios(id)
            | Self::RomFolder(id)
            | Self::RomFile(id) => Some(id),
            Self::Avatar | Self::Background | Self::Banner => None,
        }
    }

    pub fn is_folder(&self) -> bool {
        matches!(self, Self::EmulatorBios(_) | Self::RomFolder(_))
    }

    pub fn is_image(&self) -> bool {
        matches!(self, Self::Avatar | Self::Background | Self::Banner)
    }
}

/// Abre o diálogo nativo e devolve `None` quando o usuário cancela.
///
/// `zenity`/`kdialog` são processos externos; a chamada bloqueante roda em
/// `spawn_blocking` para não travar a thread do iced. Sem nenhum dos dois,
/// devolvemos um erro legível em vez de um "cancelado" silencioso.
pub async fn pick(purpose: PickPurpose) -> Result<Option<PathBuf>, String> {
    tokio::task::spawn_blocking(move || pick_blocking(&purpose))
        .await
        .map_err(|error| format!("seletor nativo falhou: {error}"))?
}

#[cfg(target_os = "linux")]
fn pick_blocking(purpose: &PickPurpose) -> Result<Option<PathBuf>, String> {
    let (program, args) = if command_in_path("zenity").is_some() {
        ("zenity", zenity_args(purpose))
    } else if command_in_path("kdialog").is_some() {
        ("kdialog", kdialog_args(purpose))
    } else {
        return Err(
            "nenhum seletor nativo disponível; instale zenity ou kdialog (ou digite o caminho manualmente)"
                .to_string(),
        );
    };

    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("não foi possível abrir {program}: {error}"))?;

    // zenity/kdialog usam código 1 para cancelamento; qualquer saída sem
    // caminho também é tratada como cancelamento.
    if !output.status.success() {
        return Ok(None);
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(path)))
    }
}

#[cfg(not(target_os = "linux"))]
fn pick_blocking(_purpose: &PickPurpose) -> Result<Option<PathBuf>, String> {
    Err("seletor nativo disponível apenas no Linux; digite o caminho manualmente".to_string())
}

#[cfg(target_os = "linux")]
fn zenity_args(purpose: &PickPurpose) -> Vec<String> {
    let mut args = vec![
        "--file-selection".to_string(),
        "--title=ARCADIA".to_string(),
    ];
    if purpose.is_folder() {
        args.push("--directory".to_string());
    }
    if purpose.is_image() {
        args.push(
            "--file-filter=Images | *.png *.jpg *.jpeg *.webp *.gif *.bmp *.avif".to_string(),
        );
    }
    args
}

#[cfg(target_os = "linux")]
fn kdialog_args(purpose: &PickPurpose) -> Vec<String> {
    let mut args = vec!["--title".to_string(), "ARCADIA".to_string()];
    if purpose.is_folder() {
        args.push("--getexistingdirectory".to_string());
        args.push(".".to_string());
    } else {
        args.push("--getopenfilename".to_string());
        args.push(".".to_string());
        if purpose.is_image() {
            args.push("Images (*.png *.jpg *.jpeg *.webp *.gif *.bmp *.avif)".to_string());
        }
    }
    args
}

#[cfg(target_os = "linux")]
fn command_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purposes_map_to_targets_and_runtime_kind() {
        assert_eq!(PickPurpose::Avatar.target(), PickTarget::Avatar);
        assert!(PickPurpose::Avatar.is_image());
        assert!(!PickPurpose::Avatar.is_folder());

        let core = PickPurpose::EmulatorCore("retroarch".to_string());
        assert_eq!(core.target(), PickTarget::EmulatorCore);
        assert_eq!(core.emulator_id(), Some("retroarch"));
        assert!(!core.is_image());

        let bios = PickPurpose::EmulatorBios("pcsx2".to_string());
        assert!(bios.is_folder());
        assert_eq!(bios.emulator_id(), Some("pcsx2"));

        let folder = PickPurpose::RomFolder("dolphin".to_string());
        assert!(folder.is_folder());
        assert_eq!(folder.target(), PickTarget::RomFolder);

        let rom = PickPurpose::RomFile("dolphin".to_string());
        assert_eq!(rom.target(), PickTarget::RomFile);
        assert_eq!(rom.emulator_id(), Some("dolphin"));

        assert_eq!(PickPurpose::Background.emulator_id(), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn zenity_args_use_directory_and_image_filters() {
        let folder = zenity_args(&PickPurpose::RomFolder("dolphin".to_string()));
        assert!(folder.contains(&"--directory".to_string()));
        assert!(!folder.iter().any(|arg| arg.contains("*.png")));

        let image = zenity_args(&PickPurpose::Avatar);
        assert!(image.iter().any(|arg| arg.contains("*.png")));
        assert!(!image.contains(&"--directory".to_string()));
    }
}
