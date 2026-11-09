//! Duplos de teste: fetcher com fila de respostas e writer em memória.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;

use crate::art::ArtWriter;
use crate::error::FetchError;
use crate::http::{Fetcher, Request, Response};

/// Fetcher que registra requisições e devolve respostas da fila.
pub struct MockFetcher {
    requests: Mutex<Vec<Request>>,
    responses: Mutex<VecDeque<std::result::Result<Response, FetchError>>>,
}

impl MockFetcher {
    pub fn new(responses: Vec<Response>) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into_iter().map(Ok).collect()),
        })
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

#[async_trait]
impl Fetcher for MockFetcher {
    async fn fetch(&self, request: Request) -> std::result::Result<Response, FetchError> {
        self.requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request);
        let response = self
            .responses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front();
        response.unwrap_or_else(|| Err(FetchError::new("mock sem resposta")))
    }
}

pub fn json_response(value: Value) -> Response {
    Response::json(200, &value)
}

/// Writer que guarda os arquivos em memória.
#[derive(Default)]
pub struct RecordingWriter {
    files: Mutex<Vec<(PathBuf, Vec<u8>)>>,
}

impl RecordingWriter {
    pub fn files(&self) -> Vec<(PathBuf, Vec<u8>)> {
        self.files
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl ArtWriter for RecordingWriter {
    fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        self.files
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((path.to_path_buf(), bytes.to_vec()));
        Ok(())
    }
}
