//! Política de fila/integridade e clientes de runtime dos downloads.
//!
//! Oráculo congelado (`app/electron`):
//! - `download-queue-policy.js` / `download-integrity.js` — lógica pura;
//! - `downloadmanager.js` — estado persistido em `downloads.json`;
//! - `aria2.js` — JSON-RPC do daemon aria2c;
//! - `depotdownloader.js` + `steamstore.js` — argv/instalação do runtime;
//! - `torrent.js` + `torrent_rpc/*.py` — protocolo JSON-lines do worker.
//!
//! O crate não spawna processos nem baixa conteúdo. HTTP, stdio, sinais e
//! escrita em disco são traits injetados (com implementações prontas para o
//! JSON-RPC local e para gravação atômica); a borda Tauri executa.

mod js;

pub mod aria2;
pub mod depot;
pub mod integrity;
pub mod manager;
pub mod queue;
pub mod torrent;

pub use aria2::*;
pub use depot::*;
pub use integrity::*;
pub use manager::*;
pub use queue::*;
pub use torrent::*;

/// Mantido do stub original para consumidores que já verificavam o crate.
pub fn crate_ready() -> bool {
    true
}
