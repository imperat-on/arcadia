//! Discord Rich Presence — protocolo IPC local.
//!
//! Port 1:1 de `app/electron/discord-rpc.js`. O Discord não usa HTTP: abre um
//! socket Unix (`discord-ipc-N`) e troca frames binários.
//!
//! ## Protocolo
//!
//! Cada frame são 8 bytes de cabeçalho + corpo JSON UTF-8:
//!
//! ```text
//! offset 0..4   u32 LE   opcode (0 = handshake, 1 = frame, 2 = close)
//! offset 4..8   u32 LE   tamanho do corpo
//! offset 8..    bytes    corpo
//! ```
//!
//! Handshake: `{"v":1,"client_id":"..."}`, e o Discord responde opcode 1 com
//! `{"cmd":"DISPATCH","evt":"READY"}`. Só depois disso as atividades podem ser
//! enviadas, com `SET_ACTIVITY`.
//!
//! ## Silêncio por design
//!
//! Sem `discord_rich_presence` ligado, sem client id, ou sem Discord aberto,
//! todos os métodos não fazem nada. Presença é enfeite: nunca pode derrubar o
//! launch de um jogo nem atrasar a UI.
//!
//! ## Paths
//!
//! No Linux o socket fica em `$XDG_RUNTIME_DIR/discord-ipc-N` (N de 0 a 9), e
//! também no runtime privado do Flatpak do Vesktop. No Windows é um named pipe.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Client id padrão do Arcadia (o mesmo do Electron).
pub const DEFAULT_CLIENT_ID: &str = "1537346240579248219";
const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;

/// Fluxo conectado ao Discord: socket Unix no Linux, named pipe no Windows.
#[cfg(unix)]
type Fluxo = std::os::unix::net::UnixStream;

/// No Windows, o Discord expõe `\\.\pipe\discord-ipc-N`. O `File` do std abre
/// named pipes por esse caminho, então serve como fluxo.
#[cfg(windows)]
type Fluxo = std::fs::File;

/// Estado do socket, compartilhado entre a UI e a thread de conexão.
struct Estado {
    socket: Option<Fluxo>,
    conectado: bool,
    /// Atividade que chegou antes da conexão e será enviada ao conectar.
    pendente: Option<String>,
}

/// Cliente de Rich Presence.
///
/// É `Send + Sync`: pode viver no `AppState` e ser chamado de qualquer thread
/// do IPC. Toda falha é engolida — presença não é crítica.
pub struct DiscordRpc {
    /// Leitor da config, injetado para o crate não depender de `arcadia-store`.
    habilitado: Box<dyn Fn() -> Option<String> + Send + Sync>,
    estado: Mutex<Estado>,
    /// Evita reconectar em loop quando o Discord está fechado.
    tentou: AtomicBool,
}

impl DiscordRpc {
    /// `read_config` devolve o client id efetivo, ou `None` se a presença está
    /// desligada. Mesma ordem do Electron: env → config → default.
    pub fn new(habilitado: impl Fn() -> Option<String> + Send + Sync + 'static) -> Self {
        Self {
            habilitado: Box::new(habilitado),
            estado: Mutex::new(Estado {
                socket: None,
                conectado: false,
                pendente: None,
            }),
            tentou: AtomicBool::new(false),
        }
    }

    fn client_id(&self) -> Option<String> {
        (self.habilitado)()
    }

    /// Caminhos candidatos, na ordem do Electron: runtime do usuário, depois o
    /// runtime privado do Flatpak (Vesktop), N de 0 a 9.
    #[cfg(unix)]
    fn socket_paths() -> Vec<PathBuf> {
        let runtime = std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let uid = std::env::var("UID").unwrap_or_else(|_| "1000".to_string());
                PathBuf::from(format!("/run/user/{uid}"))
            });
        let mut paths = Vec::new();
        for i in 0..10 {
            paths.push(runtime.join(format!("discord-ipc-{i}")));
        }
        let flatpak = runtime.join(".flatpak/dev.vencord.Vesktop/xdg-run");
        for i in 0..10 {
            paths.push(flatpak.join(format!("discord-ipc-{i}")));
        }
        paths
    }

    /// No Windows são named pipes, não arquivos — só o caminho do pipe.
    #[cfg(windows)]
    fn socket_paths() -> Vec<PathBuf> {
        (0..10)
            .map(|i| PathBuf::from(format!(r"\\.\pipe\discord-ipc-{i}")))
            .collect()
    }

    /// Abre a conexão. Cada plataforma tem seu tipo de fluxo.
    #[cfg(unix)]
    fn abrir(path: &std::path::Path) -> Option<Fluxo> {
        std::os::unix::net::UnixStream::connect(path).ok()
    }

    #[cfg(windows)]
    fn abrir(path: &std::path::Path) -> Option<Fluxo> {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .ok()
    }

    /// Monta um frame: 8 bytes de cabeçalho + corpo.
    fn frame(opcode: u32, corpo: &str) -> Vec<u8> {
        let body = corpo.as_bytes();
        let mut out = Vec::with_capacity(8 + body.len());
        out.extend_from_slice(&opcode.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        out
    }

    /// Conecta no primeiro socket que aceitar. Silencioso em falha.
    fn conectar(&self, client_id: &str) {
        let mut guard = match self.estado.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if guard.conectado || guard.socket.is_some() {
            return;
        }
        for path in Self::socket_paths() {
            let Some(mut stream) = Self::abrir(&path) else {
                continue;
            };
            // Handshake: sem ele o Discord ignora tudo que vier depois.
            let hello = format!("{{\"v\":1,\"client_id\":\"{client_id}\"}}");
            if stream
                .write_all(&Self::frame(OP_HANDSHAKE, &hello))
                .is_err()
            {
                continue;
            }
            definir_timeout(&stream, Some(std::time::Duration::from_millis(1500)));
            // Lê a resposta de READY. Se não vier, tenta o próximo socket.
            let mut cabecalho = [0u8; 8];
            if stream.read_exact(&mut cabecalho).is_err() {
                continue;
            }
            let opcode =
                u32::from_le_bytes([cabecalho[0], cabecalho[1], cabecalho[2], cabecalho[3]]);
            let tamanho =
                u32::from_le_bytes([cabecalho[4], cabecalho[5], cabecalho[6], cabecalho[7]])
                    as usize;
            if tamanho > 64 * 1024 {
                continue;
            }
            let mut corpo = vec![0u8; tamanho];
            if stream.read_exact(&mut corpo).is_err() {
                continue;
            }
            if opcode != OP_FRAME {
                continue;
            }
            definir_timeout(&stream, None);
            guard.socket = Some(stream);
            guard.conectado = true;
            // Drena a atividade que estava esperando conexão.
            if let Some(activity) = guard.pendente.take() {
                drop(guard);
                self.enviar_atividade(&activity);
                return;
            }
            return;
        }
        self.tentou.store(true, Ordering::Relaxed);
    }

    /// Envia `SET_ACTIVITY`. `activity` já é o JSON pronto; `null` limpa.
    fn enviar_atividade(&self, activity: &str) -> bool {
        let client_id = match self.client_id() {
            Some(id) => id,
            None => return false,
        };
        // Sem conexão, guarda uma pendência e tenta conectar uma vez.
        {
            let guard = match self.estado.lock() {
                Ok(guard) => guard,
                Err(_) => return false,
            };
            if !guard.conectado {
                drop(guard);
                if let Ok(mut guard) = self.estado.lock() {
                    guard.pendente = Some(activity.to_string());
                }
                self.conectar(&client_id);
                return false;
            }
        }

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let payload = format!(
            "{{\"cmd\":\"SET_ACTIVITY\",\"nonce\":\"{nonce}\",\"args\":{{\"pid\":{pid},\"activity\":{activity}}}}}",
            pid = std::process::id(),
        );
        let mut guard = match self.estado.lock() {
            Ok(guard) => guard,
            Err(_) => return false,
        };
        let Some(socket) = guard.socket.as_mut() else {
            guard.pendente = Some(activity.to_string());
            return false;
        };
        if socket.write_all(&Self::frame(OP_FRAME, &payload)).is_err() {
            guard.socket = None;
            guard.conectado = false;
            return false;
        }
        true
    }

    /// Marca "jogando <título>". Silencioso se a presença está desligada.
    pub fn set_game(&self, titulo: &str, launcher: &str) {
        if self.client_id().is_none() {
            return;
        }
        let inicio = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // Escapa aspas e barras para o JSON não quebrar. Título de jogo pode
        // ter qualquer coisa.
        let detalhe = json_escape(titulo);
        let estado = json_escape(&format!("Arcadia | {launcher}"));
        let activity = format!(
            "{{\"details\":\"{detalhe}\",\"state\":\"{estado}\",\"timestamps\":{{\"start\":{inicio}}}}}"
        );
        self.enviar_atividade(&activity);
    }

    /// Limpa a presença (jogo fechou).
    pub fn clear(&self) {
        let Ok(mut guard) = self.estado.lock() else {
            return;
        };
        guard.pendente = None;
        if guard.conectado {
            drop(guard);
            self.enviar_atividade("null");
        } else {
            guard.socket = None;
        }
    }
}

/// `true` se o fluxo suporta timeout de leitura (Unix sim, pipe do Windows não).
#[cfg(unix)]
fn definir_timeout(stream: &Fluxo, timeout: Option<std::time::Duration>) {
    let _ = stream.set_read_timeout(timeout);
}

/// Named pipe do Windows não tem `set_read_timeout` no std; o handshake lê
/// direto. Se o Discord não responder, o `read_exact` bloqueia até o pipe
/// fechar — por isso a conexão roda fora do caminho crítico.
#[cfg(windows)]
fn definir_timeout(_stream: &Fluxo, _timeout: Option<std::time::Duration>) {}

/// Escape mínimo de string JSON (aspas, barra e controle).
fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_tem_cabecalho_de_8_bytes_e_tamanho_certo() {
        let corpo = "{\"v\":1}";
        let frame = DiscordRpc::frame(OP_HANDSHAKE, corpo);
        assert_eq!(frame.len(), 8 + corpo.len());
        assert_eq!(
            u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]),
            0
        );
        assert_eq!(
            u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]) as usize,
            corpo.len()
        );
        assert_eq!(&frame[8..], corpo.as_bytes());
    }

    #[test]
    fn json_escape_protege_aspas_e_controle() {
        assert_eq!(json_escape("Assassin's Creed"), "Assassin's Creed");
        assert_eq!(json_escape("diz \"oi\""), "diz \\\"oi\\\"");
        assert_eq!(json_escape("linha\nnova"), "linha\\nnova");
        assert_eq!(json_escape("barra\\invertida"), "barra\\\\invertida");
    }

    #[test]
    fn desligado_nao_abre_socket_nem_quebra() {
        // Sem config habilitada, tudo é no-op silencioso.
        let rpc = DiscordRpc::new(|| None);
        rpc.set_game("Qualquer Jogo", "steam");
        rpc.clear();
        let guard = rpc.estado.lock().unwrap();
        assert!(!guard.conectado);
        assert!(guard.socket.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn socket_paths_cobrem_runtime_e_flatpak() {
        let paths = DiscordRpc::socket_paths();
        assert_eq!(paths.len(), 20, "10 do runtime + 10 do flatpak");
        assert!(paths[0].to_string_lossy().ends_with("discord-ipc-0"));
        assert!(paths
            .iter()
            .any(|p| p.to_string_lossy().contains(".flatpak/dev.vencord.Vesktop")));
    }

    #[cfg(windows)]
    #[test]
    fn socket_paths_do_windows_sao_named_pipes() {
        let paths = DiscordRpc::socket_paths();
        assert_eq!(paths.len(), 10);
        assert!(paths[0].to_string_lossy().starts_with(r"\\.\pipe\"));
    }
}
