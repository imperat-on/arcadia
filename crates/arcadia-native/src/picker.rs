//! Seletor nativo de arquivos/pastas compartilhado pelas telas.
//!
//! O diálogo real fica atrás de [`pick`] e nunca é aberto em teste. No Linux o
//! `rfd` usa o portal XDG (sem GTK) e, se o portal não responder, cai para o
//! `zenity` — os dois caminhos são internos à biblioteca.

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
/// O `rfd` não distingue cancelamento de falha do portal no valor de retorno;
/// por isso a disponibilidade mínima é conferida antes. Sem barramento de
/// sessão e sem zenity/kdialog, devolvemos um erro legível em vez de um
/// "cancelado" silencioso.
pub async fn pick(purpose: PickPurpose) -> Result<Option<PathBuf>, String> {
    ensure_dialog_available().await?;

    let folder = purpose.is_folder();
    let image = purpose.is_image();
    let dialog = rfd::AsyncFileDialog::new().set_title("ARCADIA");
    let dialog = if image {
        dialog.add_filter(
            "Images",
            &["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"],
        )
    } else {
        dialog
    };

    let handle = if folder {
        dialog.pick_folder().await
    } else {
        dialog.pick_file().await
    };
    Ok(handle.map(|handle| handle.path().to_path_buf()))
}

/// O portal XDG é D-Bus; um barramento de sessão é o sinal disponível sem
/// adicionar um cliente D-Bus. Zenity/kdialog cobrem o caso sem portal.
#[cfg(target_os = "linux")]
async fn ensure_dialog_available() -> Result<(), String> {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        return Ok(());
    }
    if command_in_path("zenity").is_some() || command_in_path("kdialog").is_some() {
        return Ok(());
    }
    Err(
        "nenhum seletor nativo disponível (portal XDG, zenity ou kdialog não encontrados)"
            .to_string(),
    )
}

#[cfg(not(target_os = "linux"))]
async fn ensure_dialog_available() -> Result<(), String> {
    Ok(())
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
}
