//! Política de fila e integridade de downloads (port dos módulos puros do
//! Electron).
//!
//! Oráculo congelado:
//! - `app/electron/download-queue-policy.js`
//! - `app/electron/download-integrity.js`
//!
//! O crate não spawna processos, não lê/escreve arquivos e não usa timers.
//! Ele decide ordem, slots, pause/resume/cancel/retry, validação de
//! tamanho/hash, aceitação de parciais, offset de resume e limpeza; a borda
//! (Tauri/Electron) executa.

mod js;

pub mod integrity;
pub mod queue;

pub use integrity::*;
pub use queue::*;

/// Mantido do stub original para consumidores que já verificavam o crate.
pub fn crate_ready() -> bool {
    true
}
