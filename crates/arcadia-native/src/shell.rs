//! Implementação de `arcadia_ipc::Shell` para o app nativo.
//!
//! Nesta fatia os métodos de janela são no-op: o controle de janela ainda é do
//! SO (decorações nativas). `emit` só registra no stderr; quando houver um canal
//! de eventos iced, ele será ligado aqui sem mudar o contrato do IPC.

use arcadia_ipc::Shell;
use serde_json::Value;

#[derive(Debug, Default)]
pub struct NativeShell;

impl NativeShell {
    pub fn new() -> Self {
        Self
    }
}

impl Shell for NativeShell {
    fn minimize(&self) {}

    fn toggle_maximize(&self) {}

    fn close(&self) {}

    fn quit(&self) {
        eprintln!("[native-shell] quit solicitado pelo IPC");
    }

    fn set_fullscreen(&self, _on: bool) -> Result<(), String> {
        Ok(())
    }

    fn toggle_fullscreen(&self) -> Result<(), String> {
        Ok(())
    }

    fn is_focused(&self) -> bool {
        true
    }

    fn boot_video(&self) -> Option<Vec<u8>> {
        // O vídeo de boot (GStreamer/mpv) fica para a onda de mídia.
        None
    }

    fn emit(&self, event: &str, payload: Value) {
        let text = payload.to_string();
        let preview: String = text.chars().take(240).collect();
        eprintln!("[native-shell] evento {event}: {preview}");
    }
}
