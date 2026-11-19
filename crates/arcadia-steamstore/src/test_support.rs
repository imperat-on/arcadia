//! Duplo de `Fetcher` para testes: rotas por substring de URL e registro das
//! requisições. Nenhum teste toca a rede real.

use std::sync::Mutex;

use arcadia_catalog::http::{FetchError, Fetcher, Method, Request, Response};
use async_trait::async_trait;
use serde_json::Value;

struct Route {
    contains: String,
    method: Option<Method>,
    response: Response,
    error: Option<String>,
}

#[derive(Default)]
pub struct FixtureFetcher {
    routes: Mutex<Vec<Route>>,
    calls: Mutex<Vec<Request>>,
}

impl FixtureFetcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn json(self, contains: &str, value: Value) -> Self {
        self.json_status(contains, 200, value)
    }

    pub fn json_status(self, contains: &str, status: u16, value: Value) -> Self {
        self.routes.lock().unwrap().push(Route {
            contains: contains.to_string(),
            method: None,
            response: Response::json(status, &value),
            error: None,
        });
        self
    }

    pub fn text(self, contains: &str, status: u16, body: &str) -> Self {
        self.routes.lock().unwrap().push(Route {
            contains: contains.to_string(),
            method: None,
            response: Response::text(status, body),
            error: None,
        });
        self
    }

    pub fn method(self, method: Method, contains: &str, status: u16, body: &str) -> Self {
        self.routes.lock().unwrap().push(Route {
            contains: contains.to_string(),
            method: Some(method),
            response: Response::text(status, body),
            error: None,
        });
        self
    }

    pub fn error(self, contains: &str, message: &str) -> Self {
        self.routes.lock().unwrap().push(Route {
            contains: contains.to_string(),
            method: None,
            response: Response::new(0, Vec::new()),
            error: Some(message.to_string()),
        });
        self
    }

    pub fn calls(&self) -> Vec<Request> {
        self.calls.lock().unwrap().clone()
    }

    pub fn urls(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .map(|request| request.url)
            .collect()
    }
}

#[async_trait]
impl Fetcher for FixtureFetcher {
    async fn fetch(&self, request: Request) -> Result<Response, FetchError> {
        self.calls.lock().unwrap().push(request.clone());
        let routes = self.routes.lock().unwrap();
        for route in routes.iter() {
            if !request.url.contains(&route.contains) {
                continue;
            }
            if let Some(method) = route.method {
                if method != request.method {
                    continue;
                }
            }
            if let Some(error) = &route.error {
                return Err(FetchError::new(error.clone()));
            }
            return Ok(route.response.clone());
        }
        Err(FetchError::new(format!("sem rota para {}", request.url)))
    }
}
