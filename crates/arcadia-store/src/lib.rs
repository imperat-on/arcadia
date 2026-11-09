//! Persistência local do Arcadia — mesmo `dataDir`, mesmos arquivos, mesmos
//! formatos do cliente Electron. Nenhum campo desconhecido é descartado.

pub mod accounts;
pub mod atomic;
pub mod backup;
pub mod config;
pub mod library;
pub mod owned;
pub mod paths;
pub mod session;

pub use paths::{account_path, data_path, resolve_data_dir};
