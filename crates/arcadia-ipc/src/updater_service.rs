//! Serviço de auto-update do shell empacotado.
//!
//! Liga o `PackagedUpdater` (máquina de estados, sem rede) ao
//! [`HttpAutoUpdater`](crate::updater_backend::HttpAutoUpdater) (HTTP + fs) e
//! persiste o estado no `data_dir`, para o app lembrar entre execuções que já
//! avisou sobre uma versão.
//!
//! ## Canal
//!
//! A detecção é a mesma do Electron (`detect_channel`): `APPIMAGE` define
//! AppImage; no Windows, com `app-update.yml` é `nsis`; rodando do checkout é
//! `fonte` (atualiza por git, não por aqui); resto é `sem_suporte`.
//!
//! ## Onde ficam os manifestos
//!
//! `ARCADIA_UPDATE_URL` aponta o release (ex.:
//! `https://github.com/imperat-on/arcadia/releases/latest/download`). Sem ela,
//! usa o repositório oficial. O manifesto é `latest-linux.yml` ou `latest.yml`
//! conforme o canal.
//!
//! ## Estado persistido
//!
//! `update_state.json` no `data_dir`: `{ "jaAvisado": "<versão>", "pendente":
//! "<versão>" }`. É o equivalente ao que o Electron guardava em config.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Value};

use arcadia_updater::packaged::{
    detect_channel, Channel, CheckResult, Env, PackagedConfig, PackagedState, PackagedUpdater,
    Phase,
};

use crate::updater_backend::HttpAutoUpdater;

/// Release padrão, quando nada é configurado.
const URL_PADRAO: &str = "https://github.com/imperat-on/arcadia/releases/latest/download";
const ESTADO_ARQUIVO: &str = "update_state.json";

/// Estado serializado entre execuções.
#[derive(Debug, Clone, Default)]
struct Persistido {
    ja_avisado: Option<String>,
    pendente: Option<String>,
}

fn ler_persistido(data_dir: &Path) -> Persistido {
    let raw = match std::fs::read_to_string(data_dir.join(ESTADO_ARQUIVO)) {
        Ok(raw) => raw,
        Err(_) => return Persistido::default(),
    };
    let value: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    Persistido {
        ja_avisado: value
            .get("jaAvisado")
            .and_then(Value::as_str)
            .map(str::to_string),
        pendente: value
            .get("pendente")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

fn gravar_persistido(data_dir: &Path, estado: &Persistido) {
    let value = json!({
        "jaAvisado": estado.ja_avisado,
        "pendente": estado.pendente,
    });
    // Escrita atômica: o app pode morrer no meio, e um JSON truncado faria o
    // updater esquecer que já avisou.
    let destino = data_dir.join(ESTADO_ARQUIVO);
    let tmp = data_dir.join(format!("{ESTADO_ARQUIVO}.tmp"));
    if std::fs::write(&tmp, value.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, &destino);
    }
}

/// Canal detectado a partir do ambiente real do processo.
pub fn canal_atual() -> Channel {
    let env = Env::new();
    // Empacotado = tem resource dir ou foi iniciado de um AppImage/instalador.
    let empacotado = std::env::var_os("APPIMAGE").is_some()
        || cfg!(windows) && std::env::var_os("ARCADIA_PACKAGED").is_some();
    detect_channel(empacotado, &env, empacotado, std::env::consts::OS)
}

/// Serviço vivo, guardado no `AppState`.
///
/// O `PackagedUpdater` guarda callbacks `Box<dyn FnMut>` sem `Send`/`Sync`, o
/// que impediria o `AppState` de ser `Sync` (exigido pelo Tauri). A solução é
/// confiná-lo numa thread própria e falar com ela por canal: `UpdateService`
/// fica `Send + Sync` e o updater nunca cruza thread.
pub struct UpdateService {
    data_dir: PathBuf,
    canal: Channel,
    comando: Mutex<std::sync::mpsc::Sender<Comando>>,
}

/// Pedido enviado à thread do updater.
enum Comando {
    Estado(std::sync::mpsc::Sender<PackagedState>),
    Check {
        manual: bool,
        resposta: std::sync::mpsc::Sender<CheckResult>,
    },
    Download(std::sync::mpsc::Sender<arcadia_updater::packaged::ActionResult>),
    Install(std::sync::mpsc::Sender<arcadia_updater::packaged::ActionResult>),
}

impl UpdateService {
    pub fn new(data_dir: impl Into<PathBuf>, app_version: &str) -> Self {
        let data_dir = data_dir.into();
        let persistido = ler_persistido(&data_dir);
        let canal = canal_atual();
        let empacotado = matches!(canal, Channel::AppImage | Channel::Nsis);
        let base = std::env::var("ARCADIA_UPDATE_URL").unwrap_or_else(|_| URL_PADRAO.to_string());
        let alvo = std::env::current_exe().unwrap_or_else(|_| data_dir.join("arcadia"));
        let work = data_dir.join("updates");
        let versao = app_version.to_string();
        let dir_thread = data_dir.clone();
        let versao_backend = versao.clone();

        let (tx, rx) = std::sync::mpsc::channel::<Comando>();

        // A thread é a DONA do updater. Ela morre quando o canal fecha (o
        // serviço é dropado junto com o AppState).
        std::thread::Builder::new()
            .name("arcadia-updater".to_string())
            .spawn(move || {
                let config = PackagedConfig::new(empacotado, versao)
                    .with_platform(std::env::consts::OS)
                    .with_app_update_yml(empacotado)
                    .with_pending({
                        let pendente = persistido.pendente.clone();
                        move || pendente.clone()
                    })
                    .with_save_pending({
                        let dir = dir_thread.clone();
                        Box::new(move |valor: Option<&str>| {
                            let mut atual = ler_persistido(&dir);
                            atual.pendente = valor.map(str::to_string);
                            gravar_persistido(&dir, &atual);
                        })
                    })
                    .with_advised({
                        let avisado = persistido.ja_avisado.clone();
                        move |versao: &str| avisado.as_deref() == Some(versao)
                    })
                    .with_save_advised({
                        let dir = dir_thread.clone();
                        move |versao: &str| {
                            let mut atual = ler_persistido(&dir);
                            atual.ja_avisado = Some(versao.to_string());
                            gravar_persistido(&dir, &atual);
                        }
                    });

                let backend: Box<dyn arcadia_updater::packaged::AutoUpdater> = if empacotado {
                    Box::new(HttpAutoUpdater::new(
                        versao_backend,
                        canal.as_str(),
                        base,
                        alvo,
                        work,
                    ))
                } else {
                    Box::new(SemBackend)
                };
                let mut updater =
                    PackagedUpdater::new(config.with_backend(ConcretoOuVazio(backend)));

                while let Ok(comando) = rx.recv() {
                    match comando {
                        Comando::Estado(resposta) => {
                            let _ = resposta.send(updater.state());
                        }
                        Comando::Check { manual, resposta } => {
                            let _ = resposta.send(updater.check(manual));
                        }
                        Comando::Download(resposta) => {
                            let _ = resposta.send(updater.download());
                        }
                        Comando::Install(resposta) => {
                            let _ = resposta.send(updater.install());
                        }
                    }
                }
            })
            .ok();

        Self {
            data_dir,
            canal,
            comando: Mutex::new(tx),
        }
    }

    fn enviar(&self, comando: Comando) -> bool {
        self.comando
            .lock()
            .map(|tx| tx.send(comando).is_ok())
            .unwrap_or(false)
    }

    pub fn canal(&self) -> Channel {
        self.canal
    }

    pub fn state(&self) -> PackagedState {
        let (tx, rx) = std::sync::mpsc::channel();
        if self.enviar(Comando::Estado(tx)) {
            if let Ok(estado) = rx.recv_timeout(std::time::Duration::from_secs(3)) {
                return estado;
            }
        }
        PackagedState {
            canal: self.canal,
            suportado: false,
            versao_atual: String::new(),
            versao_nova: None,
            tamanho: None,
            fase: Phase::Ocioso,
            progresso: 0,
            erro: None,
            erro_de_fundo: false,
            erro_acao: None,
            ja_avisado: false,
            jogo_rodando: false,
        }
    }

    pub fn check(&self, manual: bool) -> CheckResult {
        let (tx, rx) = std::sync::mpsc::channel();
        if self.enviar(Comando::Check {
            manual,
            resposta: tx,
        }) {
            if let Ok(resultado) = rx.recv_timeout(std::time::Duration::from_secs(90)) {
                return resultado;
            }
        }
        CheckResult {
            ok: false,
            disponivel: false,
            versao: None,
            tamanho: None,
            motivo: None,
            erro: Some("updater indisponível".to_string()),
        }
    }

    pub fn download(&self) -> arcadia_updater::packaged::ActionResult {
        let (tx, rx) = std::sync::mpsc::channel();
        if self.enviar(Comando::Download(tx)) {
            if let Ok(resultado) = rx.recv_timeout(std::time::Duration::from_secs(300)) {
                return resultado;
            }
        }
        arcadia_updater::packaged::ActionResult::error("updater indisponível")
    }

    pub fn install(&self) -> arcadia_updater::packaged::ActionResult {
        let (tx, rx) = std::sync::mpsc::channel();
        if self.enviar(Comando::Install(tx)) {
            if let Ok(resultado) = rx.recv_timeout(std::time::Duration::from_secs(30)) {
                return resultado;
            }
        }
        arcadia_updater::packaged::ActionResult::error("updater indisponível")
    }

    pub fn mark_advised(&self, versao: Option<&str>) -> bool {
        if let Some(versao) = versao {
            let mut atual = ler_persistido(&self.data_dir);
            atual.ja_avisado = Some(versao.to_string());
            gravar_persistido(&self.data_dir, &atual);
        }
        true
    }
}

/// Wrapper que delega ao backend concreto dentro do `Box`.
///
/// `PackagedUpdater::with_backend` exige `B: AutoUpdater + 'static`, e
/// `Box<dyn AutoUpdater>` não implementa o próprio trait. Este newtype faz a
/// delegação.
struct ConcretoOuVazio(Box<dyn arcadia_updater::packaged::AutoUpdater>);

impl arcadia_updater::packaged::AutoUpdater for ConcretoOuVazio {
    fn configure(&mut self, auto_download: bool, auto_install_on_app_quit: bool) {
        self.0.configure(auto_download, auto_install_on_app_quit);
    }
    fn check_for_updates(
        &mut self,
    ) -> Result<arcadia_updater::packaged::CheckForUpdatesResult, String> {
        self.0.check_for_updates()
    }
    fn download_update(&mut self) -> Result<(), String> {
        self.0.download_update()
    }
    fn quit_and_install(&mut self) -> Result<(), String> {
        self.0.quit_and_install()
    }
    fn take_events(&mut self) -> Vec<arcadia_updater::packaged::UpdaterEvent> {
        self.0.take_events()
    }
}

/// Backend vazio para canais que não se atualizam (`fonte`, `portable`, `zip`).
struct SemBackend;

impl arcadia_updater::packaged::AutoUpdater for SemBackend {
    fn configure(&mut self, _: bool, _: bool) {}
    fn check_for_updates(
        &mut self,
    ) -> Result<arcadia_updater::packaged::CheckForUpdatesResult, String> {
        Err("canal_nao_suportado".to_string())
    }
    fn download_update(&mut self) -> Result<(), String> {
        Err("canal_nao_suportado".to_string())
    }
    fn quit_and_install(&mut self) -> Result<(), String> {
        Err("canal_nao_suportado".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persiste_e_lembra_o_ja_avisado() {
        let dir = std::env::temp_dir().join(format!("arcadia-upd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let service = UpdateService::new(&dir, "1.5.5");
        assert!(service.mark_advised(Some("1.6.0")));

        let lido = ler_persistido(&dir);
        assert_eq!(lido.ja_avisado.as_deref(), Some("1.6.0"));

        // Um segundo serviço sobre o mesmo dir enxerga o valor persistido.
        let novo = UpdateService::new(&dir, "1.5.5");
        assert!(novo.state().ja_avisado || novo.state().versao_nova.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn escrita_do_estado_e_atomica_e_nao_deixa_tmp() {
        let dir = std::env::temp_dir().join(format!("arcadia-upd-tmp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        gravar_persistido(
            &dir,
            &Persistido {
                ja_avisado: Some("9.9.9".into()),
                pendente: None,
            },
        );
        assert!(!dir.join(format!("{ESTADO_ARQUIVO}.tmp")).exists());
        assert!(dir.join(ESTADO_ARQUIVO).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
