//! Bundle de executores de produção do `fixes:apply`.
//!
//! A borda IPC/Tauri monta o [`FixEngine`] uma vez e reaproveita os executores
//! enquanto a janela viver:
//!
//! ```ignore
//! let roots = single_root_roots(game_root, data_root);
//! let engine = FixEngine::new(catalog, Arc::new(StdFixFs), roots.clone());
//! let executors = FixExecutors::production(&roots);
//! let response = executors.apply(&engine, &request);
//! ```
//!
//! O download usa `reqwest` + rustls e grava sob `roots.data_tmp_dir()`; a
//! extração usa 7-Zip quando disponível e cai para o extrator ZIP em memória.
//! Nenhum caminho/temporário sai do data root.

use std::sync::Arc;

use crate::download::DownloadExecutor;
use crate::exec_archive::{SevenZipArchiveExtractor, ZipArchiveExtractor};
use crate::exec_http::{CancelHandle, HttpDownloadExecutor};
use crate::install::{ArchiveExtractor, PathRoots};
use crate::plan::{ApplyRequest, ApplyResponse, FixEngine};

/// Executores prontos para `FixEngine::apply`.
#[derive(Clone)]
pub struct FixExecutors {
    pub downloader: Arc<dyn DownloadExecutor>,
    pub extractor: Arc<dyn ArchiveExtractor>,
    cancel: Option<CancelHandle>,
}

impl std::fmt::Debug for FixExecutors {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FixExecutors")
            .finish_non_exhaustive()
    }
}

impl FixExecutors {
    pub fn new(
        downloader: Arc<dyn DownloadExecutor>,
        extractor: Arc<dyn ArchiveExtractor>,
    ) -> Self {
        Self {
            downloader,
            extractor,
            cancel: None,
        }
    }

    /// Conveniência para testes/borda que já possuem implementações concretas.
    pub fn from_parts<D, E>(downloader: D, extractor: E) -> Self
    where
        D: DownloadExecutor + 'static,
        E: ArchiveExtractor + 'static,
    {
        Self::new(Arc::new(downloader), Arc::new(extractor))
    }

    /// Produção: HTTP real + 7-Zip (se instalado) ou ZIP em memória.
    pub fn production(roots: &PathRoots) -> Self {
        let temp_root = roots.data_tmp_dir();
        let extractor: Arc<dyn ArchiveExtractor> =
            match SevenZipArchiveExtractor::production(temp_root) {
                Some(extractor) => Arc::new(extractor),
                None => Arc::new(ZipArchiveExtractor::new()),
            };
        let downloader = HttpDownloadExecutor::production();
        let cancel = Some(downloader.cancel_handle());
        Self {
            downloader: Arc::new(downloader),
            extractor,
            cancel,
        }
    }

    pub fn downloader(&self) -> &dyn DownloadExecutor {
        self.downloader.as_ref()
    }

    pub fn extractor(&self) -> &dyn ArchiveExtractor {
        self.extractor.as_ref()
    }

    /// `fixes:apply` completo com estes executores.
    pub fn apply(&self, engine: &FixEngine, request: &ApplyRequest) -> ApplyResponse {
        engine.apply(request, self.downloader(), self.extractor())
    }

    /// `fixes:cancel`: aborta o download HTTP em andamento (se o bundle foi
    /// criado por [`FixExecutors::production`]). O engine ainda precisa do
    /// `FixEngine::cancel` para apagar os temporários.
    pub fn cancel(&self) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
    }

    pub fn cancel_handle(&self) -> Option<CancelHandle> {
        self.cancel.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{FixCatalog, FixType};
    use crate::download::{DownloadError, DownloadEvent, DownloadPlan, DownloadedArchive};
    use crate::exec_archive::ZipArchiveExtractor;
    use crate::install::{is_fixed, StdFixFs};
    use crate::plan::single_root_roots;
    use serde_json::json;
    use std::io::{Cursor, Write};
    use std::path::Path;
    use std::sync::Arc;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    struct FakeDownloader {
        bytes: Vec<u8>,
    }

    impl DownloadExecutor for FakeDownloader {
        fn download(
            &self,
            plan: &DownloadPlan,
            progress: &mut dyn FnMut(DownloadEvent),
        ) -> Result<DownloadedArchive, DownloadError> {
            progress(DownloadEvent::downloading(0, self.bytes.len() as u64));
            if let Some(parent) = plan.destination.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&plan.destination, &self.bytes).unwrap();
            progress(DownloadEvent::downloading(
                self.bytes.len() as u64,
                self.bytes.len() as u64,
            ));
            Ok(DownloadedArchive::new(
                plan.destination.clone(),
                self.bytes.len() as u64,
                None,
            ))
        }
    }

    #[test]
    fn bundle_applies_a_zip_end_to_end_without_network_or_process() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("game");
        std::fs::create_dir_all(&game).unwrap();
        let catalog = FixCatalog::from_values(Some(&json!({"genericFixes": [730]})), None);
        let roots = single_root_roots(&game, temp.path().join("data"));
        let engine = FixEngine::new(catalog, Arc::new(StdFixFs), roots.clone());
        let executors = FixExecutors::from_parts(
            FakeDownloader {
                bytes: zip_bytes(&[
                    ("root.dll", b"root"),
                    ("sub/Game.dll", b"game"),
                    ("launcher.exe", b"launcher"),
                ]),
            },
            ZipArchiveExtractor::new(),
        );

        let request = ApplyRequest::new(
            "730",
            FixType::Generic,
            "https://files.luatools.work/GameBypasses/730.zip",
            game.to_string_lossy().to_string(),
        );
        let response = executors.apply(&engine, &request);
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({"ok": true})
        );
        assert_eq!(std::fs::read(game.join("root.dll")).unwrap(), b"root");
        assert_eq!(std::fs::read(game.join("sub/Game.dll")).unwrap(), b"game");
        assert!(is_fixed(engine.fs().as_ref(), &game));
    }

    #[test]
    fn production_bundle_keeps_temp_under_data_root() {
        let temp = tempfile::tempdir().unwrap();
        let roots = single_root_roots(temp.path().join("game"), temp.path().join("data"));
        let executors = FixExecutors::production(&roots);
        let _ = executors.downloader();
        let _ = executors.extractor();
        let handle = executors.cancel_handle().unwrap();
        executors.cancel();
        assert!(handle.is_cancelled());
        // Os construtores não criam processos nem tocam na rede.
        assert!(!Path::new(&roots.data_tmp_dir()).exists());
    }
}
