//! Realtime WebSocket — port do `channel(name)` de
//! `app/electron/supabase/client.js` (o "Phoenix-lite").
//!
//! ## Protocolo
//!
//! O servidor fala um subconjunto do protocolo do Phoenix:
//!
//! ```text
//! → {"topic":"friends-<id>","event":"phx_join",
//!    "payload":{"access_token":"<jwt>","config":{"postgres_changes":[]}},
//!    "ref":"1"}
//! ← {"event":"phx_reply","payload":{"status":"ok"}}      (join aceito)
//! ← {"event":"postgres_changes","payload":{...}}          (evento)
//! ```
//!
//! Não há heartbeat nem `phx_leave` no cliente Electron. A reconexão é fixa em
//! 3 s depois de um close, e só acontece se `subscribe` foi chamado e nada
//! chamou `unsubscribe` — reproduzido aqui no campo `assinado`.
//!
//! ## Por que três assinantes e não um
//!
//! O Electron abre um canal por recurso, porque o filtro é por tabela:
//!
//! | canal                  | tabela             | filtro              |
//! |------------------------|--------------------|---------------------|
//! | `friends-<me>`         | `friendships`      | `user_b=eq.<me>`    |
//! | `library-<uid>`        | `user_library`     | (nenhum)            |
//! | `achievements-<me>`    | `user_achievements`| (nenhum)            |
//!
//! O filtro viaja na mensagem de join (`config.postgres_changes`), não na URL.
//! O cliente Electron manda `postgres_changes: []` vazio e deixa o servidor
//! decidir — o comentário do original admite que o `filter` está declarado no
//! `.on()` mas nunca chega ao fio. Copiar o comportamento é o certo: mandar
//! filtro que o servidor não espera quebraria o join.
//!
//! ## Silêncio por design
//!
//! Como o Discord: sem sessão, sem rede ou sem servidor, tudo falha calado. O
//! realtime é frescor de UI, nunca requisito de boot. As buscas normais
//! (`friendsList`, `syncNow`) continuam sendo o caminho principal.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

/// Intervalo de reconexão. Igual ao `setTimeout(conn, 3000)` do JS.
pub const RECONEXAO: Duration = Duration::from_secs(3);
/// Limite para o join ser aceito antes de considerar a conexão morta.
pub const TIMEOUT_JOIN: Duration = Duration::from_secs(15);

/// O que o canal entrega quando chega um evento.
pub type Handler = Arc<dyn Fn(Value) + Send + Sync>;

/// Um canal realtime vivo.
///
/// `subscribe` sobe a tarefa; `unsubscribe` a derruba. Um `RealtimeChannel`
/// não é reutilizável depois de `unsubscribe` — o Electron também não.
pub struct RealtimeChannel {
    topic: String,
    ws_url: String,
    token: String,
    assinado: Arc<AtomicBool>,
    /// Contador de `ref`; o Phoenix usa string, o JS fixa "1", mas um contador
    /// evita colisão se o servidor correlacionar respostas.
    proximo_ref: Arc<AtomicU64>,
    handlers: Arc<Mutex<Vec<Handler>>>,
    /// Handle da tarefa, para o `unsubscribe` poder abortar.
    tarefa: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl RealtimeChannel {
    /// Monta o canal a partir da URL HTTP do backend (`http`→`ws`).
    ///
    /// O caminho é `/realtime/v1/websocket`, igual ao JS.
    pub fn new(base_http: &str, topic: impl Into<String>, token: impl Into<String>) -> Self {
        let ws_url = format!(
            "{}/realtime/v1/websocket",
            base_http
                .replacen("http://", "ws://", 1)
                .replacen("https://", "wss://", 1)
        );
        Self {
            topic: topic.into(),
            ws_url,
            token: token.into(),
            assinado: Arc::new(AtomicBool::new(false)),
            proximo_ref: Arc::new(AtomicU64::new(1)),
            handlers: Arc::new(Mutex::new(Vec::new())),
            tarefa: Mutex::new(None),
        }
    }

    pub fn topic(&self) -> &str {
        &self.topic
    }

    /// URL `ws://` efetiva (para diagnóstico e teste).
    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    /// Registra um handler de `postgres_changes`. Só vale antes de `subscribe`,
    /// como no JS (o original até documenta esse erro).
    pub fn on_change(&self, handler: impl Fn(Value) + Send + Sync + 'static) {
        if let Ok(mut lista) = self.handlers.lock() {
            lista.push(Arc::new(handler));
        }
    }

    /// Sobe a conexão e mantém viva até `unsubscribe`.
    pub fn subscribe(self: &Arc<Self>, sink: crate::EventSink) {
        if self.assinado.swap(true, Ordering::SeqCst) {
            return; // já assinado: nada a fazer
        }
        let canal = self.clone();
        let tarefa = tokio::spawn(async move {
            canal.loop_conexao(sink).await;
        });
        if let Ok(mut guard) = self.tarefa.lock() {
            *guard = Some(tarefa);
        }
    }

    /// Derruba a conexão e impede reconexão.
    pub fn unsubscribe(&self) {
        self.assinado.store(false, Ordering::SeqCst);
        if let Ok(mut guard) = self.tarefa.lock() {
            if let Some(tarefa) = guard.take() {
                tarefa.abort();
            }
        }
    }

    pub fn is_subscribed(&self) -> bool {
        self.assinado.load(Ordering::SeqCst)
    }

    /// Loop externo: conecta, serve, e reconecta em 3 s enquanto assinado.
    async fn loop_conexao(self: Arc<Self>, sink: crate::EventSink) {
        while self.assinado.load(Ordering::SeqCst) {
            if let Err(erro) = self.sessao(&sink).await {
                // Silencioso: realtime nunca é motivo de erro visível.
                let _ = erro;
            }
            if !self.assinado.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(RECONEXAO).await;
        }
    }

    /// Uma conexão: abre, faz o join, entrega eventos até cair.
    async fn sessao(&self, sink: &crate::EventSink) -> Result<(), String> {
        let (mut socket, resposta) = tokio_tungstenite::connect_async(&self.ws_url)
            .await
            .map_err(|erro| format!("connect: {erro}"))?;
        // Confirmar upgrade 101 não é necessário, mas evita seguir num socket
        // que o servidor recusou por outro motivo.
        if resposta.status().as_u16() != 101 {
            return Err(format!("upgrade recusado: {}", resposta.status()));
        }

        let join = json!({
            "topic": self.topic,
            "event": "phx_join",
            "payload": {
                "access_token": self.token,
                "config": { "postgres_changes": [] }
            },
            "ref": self.proximo_ref.fetch_add(1, Ordering::Relaxed).to_string()
        });
        socket
            .send(Message::Text(join.to_string().into()))
            .await
            .map_err(|erro| format!("join: {erro}"))?;

        // Espera o phx_reply com prazo. Sem isso, um servidor que aceita o
        // socket mas nunca responde deixaria o canal pendurado para sempre.
        let prazo = tokio::time::Instant::now() + TIMEOUT_JOIN;
        let mut confirmado = false;

        loop {
            let restante = prazo.saturating_duration_since(tokio::time::Instant::now());
            let mensagem = if confirmado {
                match socket.next().await {
                    Some(Ok(mensagem)) => mensagem,
                    Some(Err(erro)) => return Err(format!("socket: {erro}")),
                    None => return Err("socket fechou".to_string()),
                }
            } else {
                match tokio::time::timeout(restante, socket.next()).await {
                    Ok(Some(Ok(mensagem))) => mensagem,
                    Ok(Some(Err(erro))) => return Err(format!("socket: {erro}")),
                    Ok(None) => return Err("socket fechou antes do join".to_string()),
                    Err(_) => return Err("join sem resposta".to_string()),
                }
            };

            match mensagem {
                Message::Text(texto) => {
                    let Ok(valor) = serde_json::from_str::<Value>(&texto) else {
                        continue;
                    };
                    match valor.get("event").and_then(Value::as_str) {
                        Some("phx_reply") => {
                            let ok = valor
                                .get("payload")
                                .and_then(|payload| payload.get("status"))
                                .and_then(Value::as_str)
                                == Some("ok");
                            if !ok {
                                // O JS faz `ws.close()` no join recusado; aqui
                                // sair do loop já derruba a conexão.
                                return Err("join recusado".to_string());
                            }
                            confirmado = true;
                        }
                        Some("postgres_changes") => {
                            let payload = valor.get("payload").cloned().unwrap_or(Value::Null);
                            let handlers = self
                                .handlers
                                .lock()
                                .map(|lista| lista.clone())
                                .unwrap_or_default();
                            for handler in handlers {
                                handler(payload.clone());
                            }
                            // A UI também recebe o evento por nome de canal,
                            // para o renderer não precisar saber o tópico.
                            sink("friends:changed", payload);
                        }
                        _ => {}
                    }
                }
                Message::Ping(dados) => {
                    let _ = socket.send(Message::Pong(dados)).await;
                }
                Message::Close(_) => return Err("servidor fechou".to_string()),
                _ => {}
            }
        }
    }
}

impl std::fmt::Debug for RealtimeChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealtimeChannel")
            .field("topic", &self.topic)
            .field("assinado", &self.is_subscribed())
            .finish_non_exhaustive()
    }
}

/// Conjunto de canais do app. Espelha o que o Electron assina no boot.
pub struct Realtime {
    canais: Mutex<Vec<Arc<RealtimeChannel>>>,
    /// Sink de eventos da janela, injetado por `set_event_sink`.
    sink: Mutex<Option<crate::EventSink>>,
}

impl Realtime {
    pub fn new() -> Self {
        Self {
            canais: Mutex::new(Vec::new()),
            sink: Mutex::new(None),
        }
    }

    /// Guarda o sink da janela. Chamado por `AppState::set_event_sink`.
    pub fn set_sink(&self, sink: crate::EventSink) {
        if let Ok(mut guard) = self.sink.lock() {
            *guard = Some(sink);
        }
    }

    /// Assina os três canais do Electron de uma vez, para a sessão atual.
    ///
    /// Sem sessão, sem sink ou sem backend, não faz nada — realtime é frescor,
    /// nunca requisito.
    pub fn assinar_sessao(&self, base_http: &str, user_id: &str, token: &str) {
        let Some(sink) = self.sink.lock().ok().and_then(|guard| guard.clone()) else {
            return;
        };
        // Os três tópicos que o Electron abre, na mesma ordem.
        for topic in [
            format!("friends-{user_id}"),
            format!("library-{user_id}"),
            format!("achievements-{user_id}"),
        ] {
            self.assinar(base_http, &topic, token, sink.clone());
        }
    }

    /// Cria e assina um canal. Devolve o handle para `unsubscribe`.
    pub fn assinar(
        &self,
        base_http: &str,
        topic: &str,
        token: &str,
        sink: crate::EventSink,
    ) -> Arc<RealtimeChannel> {
        let canal = Arc::new(RealtimeChannel::new(base_http, topic, token));
        canal.subscribe(sink);
        if let Ok(mut lista) = self.canais.lock() {
            lista.push(canal.clone());
        }
        canal
    }

    /// Derruba todos os canais (logout, troca de conta).
    pub fn parar_tudo(&self) {
        if let Ok(mut lista) = self.canais.lock() {
            for canal in lista.drain(..) {
                canal.unsubscribe();
            }
        }
    }

    /// Quantos canais estão vivos (para teste e diagnóstico).
    pub fn ativos(&self) -> usize {
        self.canais
            .lock()
            .map(|lista| lista.iter().filter(|c| c.is_subscribed()).count())
            .unwrap_or(0)
    }
}

impl Default for Realtime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_http_vira_ws_e_o_caminho_e_o_do_phoenix() {
        let canal = RealtimeChannel::new("https://exemplo.dev", "friends-1", "tok");
        assert_eq!(canal.ws_url, "wss://exemplo.dev/realtime/v1/websocket");

        let canal = RealtimeChannel::new("http://127.0.0.1:3000", "x", "tok");
        assert_eq!(canal.ws_url, "ws://127.0.0.1:3000/realtime/v1/websocket");
    }

    #[test]
    fn url_que_ja_e_ws_nao_e_tocada() {
        // `replacen("http://", ...)` não casa com "ws://", então passa direto.
        let canal = RealtimeChannel::new("ws://127.0.0.1:3000", "x", "tok");
        assert_eq!(canal.ws_url, "ws://127.0.0.1:3000/realtime/v1/websocket");
    }

    #[test]
    fn handler_e_registrado_uma_vez_por_chamada() {
        let canal = RealtimeChannel::new("http://x", "t", "tok");
        canal.on_change(|_| {});
        canal.on_change(|_| {});
        assert_eq!(canal.handlers.lock().unwrap().len(), 2);
    }

    #[test]
    fn unsubscribe_antes_de_subscribe_nao_quebra() {
        let canal = RealtimeChannel::new("http://x", "t", "tok");
        canal.unsubscribe();
        assert!(!canal.is_subscribed());
    }

    #[test]
    fn ref_comeca_em_um_e_avanca() {
        let canal = RealtimeChannel::new("http://x", "t", "tok");
        assert_eq!(canal.proximo_ref.load(Ordering::Relaxed), 1);
        canal.proximo_ref.fetch_add(1, Ordering::Relaxed);
        assert_eq!(canal.proximo_ref.load(Ordering::Relaxed), 2);
    }
}
