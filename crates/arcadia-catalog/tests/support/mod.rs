//! Duplos de teste compartilhados: fetcher com fila de respostas e auth
//! roteirizado. Nenhum teste toca a rede real.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arcadia_catalog::catalog::AuthProvider;
use arcadia_catalog::http::{FetchError, Fetcher, Request, Response};
use async_trait::async_trait;
use serde_json::Value;

/// Fetcher que registra requisições e devolve respostas da fila.
pub struct ScriptedFetcher {
    requests: Mutex<Vec<Request>>,
    responses: Mutex<VecDeque<Result<Response, FetchError>>>,
    delay: Duration,
}

impl ScriptedFetcher {
    pub fn new(responses: Vec<Response>) -> Arc<Self> {
        Self::with_results(responses.into_iter().map(Ok).collect())
    }

    pub fn with_results(responses: Vec<Result<Response, FetchError>>) -> Arc<Self> {
        Self::with_delay(responses, Duration::ZERO)
    }

    pub fn with_delay(responses: Vec<Result<Response, FetchError>>, delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into()),
            delay,
        })
    }

    pub fn push(&self, response: Response) {
        self.responses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push_back(Ok(response));
    }

    pub fn push_error(&self, message: &str) {
        self.responses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push_back(Err(FetchError::new(message)));
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn request_count(&self) -> usize {
        self.requests().len()
    }

    pub fn remaining(&self) -> usize {
        self.responses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }
}

#[async_trait]
impl Fetcher for ScriptedFetcher {
    async fn fetch(&self, request: Request) -> Result<Response, FetchError> {
        self.requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request);
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        self.responses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .unwrap_or_else(|| Err(FetchError::new("mock sem resposta")))
    }
}

/// Auth que muda de token no primeiro refresh e conta chamadas.
#[derive(Debug, Default)]
pub struct ScriptedAuth {
    token: AtomicUsize,
    refreshes: AtomicUsize,
}

impl ScriptedAuth {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn refresh_count(&self) -> usize {
        self.refreshes.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl AuthProvider for ScriptedAuth {
    fn headers(&self) -> Vec<(String, String)> {
        let token = self.token.load(Ordering::SeqCst) + 1;
        vec![
            ("apikey".into(), "anon-key".into()),
            ("authorization".into(), format!("Bearer token-{token}")),
        ]
    }

    async fn refresh(&self) -> bool {
        self.token.fetch_add(1, Ordering::SeqCst);
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        true
    }
}

pub fn json_response(value: Value) -> Response {
    Response::json(200, &value)
}

pub fn json_status(status: u16, value: Value) -> Response {
    Response::json(status, &value)
}

pub fn header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

pub fn body_text(req: &Request) -> String {
    String::from_utf8_lossy(req.body.as_deref().unwrap_or_default()).into_owned()
}
