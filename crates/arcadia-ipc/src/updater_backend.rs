//! Backend concreto do auto-update (`arcadia_updater::AutoUpdater`).
//!
//! O crate `arcadia-updater` é puro por design: máquina de estados, detecção de
//! canal, comparação de versão — tudo com dependências injetadas, nenhuma rede
//! e nenhum filesystem. Este módulo é a "borda" que o comentário do crate
//! reserva ao `arcadia-ipc`: faz o HTTP, verifica o SHA-512 do manifesto e
//! substitui o artefato.
//!
//! ## Canais
//!
//! Só `appimage` e `nsis` atualizam. `portable`/`zip` não têm como se substituir
//! com segurança, e `fonte` (rodando do checkout) atualiza por git — caminho que
//! o `git_channel` já cobre.
//!
//! ## AppImage: por que não dá para sobrescrever em execução
//!
//! O kernel mantém o arquivo aberto (`ETXTBSY`). A troca é: baixar ao lado,
//! `chmod +x`, e gravar um script que espera o processo sair para mover. É o
//! mesmo mecanismo do `electron-updater`. Se a pasta for read-only (AppImage em
//! `/opt`), o resultado é erro com `ErrorAction::AbrirReleases`, e a UI mostra o
//! link — não uma tentativa que falha em silêncio.
//!
//! ## Verificação
//!
//! O manifesto publica `sha512` em base64. Todo download é conferido contra
//! ele ANTES de qualquer substituição. Hash divergente = aborta sem tocar no
//! artefato instalado.

use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine;
use sha2::{Digest, Sha512};

use arcadia_updater::packaged::{AutoUpdater, CheckForUpdatesResult, UpdateInfo, UpdaterEvent};

/// Timeout do fetch do manifesto e do download.
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Falhas de rede/formato do backend.
#[derive(Debug)]
pub enum DownloadError {
    Rede(String),
    Http(u16),
}

impl DownloadError {
    pub fn mensagem(&self) -> String {
        match self {
            Self::Rede(detalhe) => format!("falha de rede: {detalhe}"),
            Self::Http(codigo) => format!("servidor respondeu {codigo}"),
        }
    }
}

/// Baixa o manifesto do canal certo (`latest-linux.yml` / `latest.yml`).
fn fonte_do_manifesto(base: &str, channel: &str) -> Result<String, DownloadError> {
    let arquivo = if channel == "nsis" {
        arcadia_updater::manifest::LATEST_WINDOWS
    } else {
        arcadia_updater::manifest::LATEST_LINUX
    };
    let url = resolver_url(base, arquivo).map_err(DownloadError::Rede)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .map_err(|e| DownloadError::Rede(e.to_string()))?;
    let resposta = client
        .get(&url)
        .send()
        .map_err(|e| DownloadError::Rede(e.to_string()))?;
    if !resposta.status().is_success() {
        return Err(DownloadError::Http(resposta.status().as_u16()));
    }
    resposta
        .text()
        .map_err(|e| DownloadError::Rede(e.to_string()))
}

/// Cliente HTTP+FS que o `PackagedUpdater` usa em produção.
pub struct HttpAutoUpdater {
    /// Versão instalada, para o manifesto decidir se há update.
    pub current_version: String,
    /// Canal detectado (`appimage`, `nsis`, …).
    pub channel: String,
    /// URL base dos manifestos (`latest.yml` / `latest-linux.yml`).
    /// Vem do próprio release do GitHub; configurável para teste.
    pub manifest_url: String,
    /// Arquivo que será substituído (o AppImage em execução, o .exe do NSIS).
    pub target: PathBuf,
    /// Diretório de download (ao lado do alvo, para o rename ser atômico).
    pub work_dir: PathBuf,
    /// Eventos acumulados, drenados por `take_events`.
    pending: Vec<UpdaterEvent>,
    /// Info do update disponível, guardada entre `check` e `download`.
    info: Option<UpdateInfo>,
    /// SHA-512 esperado do arquivo a baixar (base64, do manifesto).
    sha512: Option<String>,
}

impl HttpAutoUpdater {
    pub fn new(
        current_version: impl Into<String>,
        channel: impl Into<String>,
        manifest_url: impl Into<String>,
        target: impl Into<PathBuf>,
        work_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            current_version: current_version.into(),
            channel: channel.into(),
            manifest_url: manifest_url.into(),
            target: target.into(),
            work_dir: work_dir.into(),
            pending: Vec::new(),
            info: None,
            sha512: None,
        }
    }

    fn empurrar(&mut self, event: UpdaterEvent) {
        self.pending.push(event);
    }
}

/// SHA-512 em base64, que é o formato do `sha512` no `latest*.yml`.
fn sha512_base64(bytes: &[u8]) -> String {
    let mut hasher = Sha512::new();
    hasher.update(bytes);
    base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
}

impl AutoUpdater for HttpAutoUpdater {
    fn configure(&mut self, _auto_download: bool, _auto_install_on_app_quit: bool) {
        // O Electron usava os dois como `false`: nada baixa nem instala sozinho.
        // A UI comanda cada passo, então não há estado a guardar aqui.
    }

    fn check_for_updates(&mut self) -> Result<CheckForUpdatesResult, String> {
        // Só canais capazes de se substituir checam.
        if !matches!(self.channel.as_str(), "appimage" | "nsis") {
            return Err("canal_nao_suportado".to_string());
        }

        let manifest = fonte_do_manifesto(&self.manifest_url, &self.channel)
            .map_err(|error: DownloadError| error.mensagem())?;

        // `parse_latest` é o mesmo parser do Electron: aceita o subconjunto
        // estável do electron-builder e valida campos obrigatórios.
        let parsed = arcadia_updater::manifest::parse_latest(&manifest)
            .map_err(|error| format!("manifesto inválido: {error:?}"))?;

        if !arcadia_updater::version::is_newer_version(&parsed.version, &self.current_version) {
            self.empurrar(UpdaterEvent::UpdateNotAvailable);
            return Ok(CheckForUpdatesResult::new(None));
        }

        let info = UpdateInfo::new(parsed.version.clone(), parsed.files.clone());
        self.sha512 = Some(parsed.sha512.clone());
        self.info = Some(info.clone());
        self.empurrar(UpdaterEvent::UpdateAvailable(info.clone()));
        Ok(CheckForUpdatesResult::with_info(info))
    }

    fn download_update(&mut self) -> Result<(), String> {
        let info = self
            .info
            .clone()
            .ok_or_else(|| "nenhum update verificado".to_string())?;
        let esperado = self
            .sha512
            .clone()
            .ok_or_else(|| "manifesto sem sha512".to_string())?;

        // O manifesto traz URL relativa ao release; resolve contra a base.
        let relativo = info
            .files
            .first()
            .map(|file| file.url.clone())
            .ok_or_else(|| "manifesto sem arquivo".to_string())?;
        let url = resolver_url(&self.manifest_url, &relativo)?;

        std::fs::create_dir_all(&self.work_dir).map_err(|e| e.to_string())?;
        let destino = self
            .work_dir
            .join(relativo.rsplit('/').next().unwrap_or("arcadia-update"));

        // Download em streaming com progresso, para a UI não ficar parada num
        // arquivo de 147 MiB.
        let bytes = baixar_com_progresso(&url, |percent| {
            self.pending
                .push(UpdaterEvent::DownloadProgress { percent });
        })
        .map_err(|error| error.mensagem())?;

        // Confere o hash ANTES de escrever: um arquivo corrompido nunca chega
        // ao disco, e o artefato atual fica intacto.
        let obtido = sha512_base64(&bytes);
        if obtido != esperado {
            let erro = "hash do download não confere com o manifesto".to_string();
            self.empurrar(UpdaterEvent::Error(erro.clone()));
            return Err(erro);
        }

        std::fs::write(&destino, &bytes).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&destino, std::fs::Permissions::from_mode(0o755));
        }

        self.empurrar(UpdaterEvent::UpdateDownloaded(info));
        Ok(())
    }

    fn quit_and_install(&mut self) -> Result<(), String> {
        let info = self
            .info
            .clone()
            .ok_or_else(|| "nenhum update baixado".to_string())?;
        let relativo = info
            .files
            .first()
            .map(|file| file.url.clone())
            .ok_or_else(|| "manifesto sem arquivo".to_string())?;
        let baixado = self
            .work_dir
            .join(relativo.rsplit('/').next().unwrap_or("arcadia-update"));

        if !baixado.is_file() {
            return Err("arquivo baixado não encontrado".to_string());
        }

        // Verifica que dá para escrever ANTES de prometer a troca. AppImage em
        // /opt ou pasta read-only cai aqui com erro acionável.
        let pasta = self
            .target
            .parent()
            .ok_or_else(|| "alvo sem pasta".to_string())?;
        if !pode_escrever(pasta) {
            let erro = "sem permissão de escrita na pasta do app";
            self.empurrar(UpdaterEvent::Error(erro.to_string()));
            return Err(erro.to_string());
        }

        #[cfg(unix)]
        {
            // Script destacado: o processo em execução não pode ser sobrescrito
            // (ETXTBSY), então ele espera o pai sair e só então move.
            let script = self.work_dir.join("arcadia-apply-update.sh");
            let conteudo = format!(
                "#!/bin/sh\n\
                 # Gerado pelo Arcadia. Espera o app sair e troca o AppImage.\n\
                 pid={pid}\n\
                 while kill -0 \"$pid\" 2>/dev/null; do sleep 0.2; done\n\
                 mv -f '{baixado}' '{alvo}' && chmod +x '{alvo}'\n\
                 rm -f \"$0\"\n",
                pid = std::process::id(),
                baixado = baixado.display(),
                alvo = self.target.display(),
            );
            std::fs::write(&script, conteudo).map_err(|e| e.to_string())?;
            let mut perms = std::fs::metadata(&script)
                .map_err(|e| e.to_string())?
                .permissions();
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).map_err(|e| e.to_string())?;

            std::process::Command::new("/bin/sh")
                .arg(&script)
                .spawn()
                .map_err(|e| format!("não foi possível agendar a troca: {e}"))?;
        }

        #[cfg(windows)]
        {
            // O NSIS/Inno Setup substitui os arquivos e fecha o app atual.
            // /SILENT evita a UI extra; os flags de close são necessários
            // porque o executável em execução está na pasta de destino.
            std::process::Command::new(&baixado)
                .args([
                    "/SILENT",
                    "/NORESTART",
                    "/CLOSEAPPLICATIONS",
                    "/FORCECLOSEAPPLICATIONS",
                ])
                .spawn()
                .map_err(|e| format!("não foi possível iniciar o instalador: {e}"))?;
        }

        Ok(())
    }

    fn take_events(&mut self) -> Vec<UpdaterEvent> {
        std::mem::take(&mut self.pending)
    }
}

/// `true` se a pasta aceita escrita (checa de fato, não só permissão teórica).
fn pode_escrever(dir: &Path) -> bool {
    let probe = dir.join(".arcadia-update-probe");
    match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&probe)
    {
        Ok(mut file) => {
            let ok = file.write_all(b"ok").is_ok();
            drop(file);
            let _ = std::fs::remove_file(&probe);
            ok
        }
        Err(_) => false,
    }
}

/// Resolve a URL do arquivo contra a base do manifesto.
fn resolver_url(manifest_url: &str, arquivo: &str) -> Result<String, String> {
    if arquivo.starts_with("http://") || arquivo.starts_with("https://") {
        return Ok(arquivo.to_string());
    }
    let base = manifest_url
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .ok_or_else(|| format!("URL base inválida: {manifest_url}"))?;
    Ok(format!("{base}/{arquivo}"))
}

/// Baixa com progresso. Usa o cliente reqwest do crate de API (rustls).
fn baixar_com_progresso(
    url: &str,
    mut progresso: impl FnMut(f64),
) -> Result<Vec<u8>, DownloadError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .map_err(|e| DownloadError::Rede(e.to_string()))?;
    let mut resposta = client
        .get(url)
        .send()
        .map_err(|e| DownloadError::Rede(e.to_string()))?;
    if !resposta.status().is_success() {
        return Err(DownloadError::Http(resposta.status().as_u16()));
    }
    let total = resposta.content_length().unwrap_or(0);
    let mut buf: Vec<u8> = Vec::with_capacity(total as usize);
    let mut lido = 0u64;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        use std::io::Read;
        let n = resposta
            .read(&mut chunk)
            .map_err(|e| DownloadError::Rede(e.to_string()))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        lido += n as u64;
        if total > 0 {
            progresso((lido as f64 / total as f64) * 100.0);
        }
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha512_base64_bate_com_vetor_conhecido() {
        // SHA-512("abc") em base64 — conferido com `sha512sum | xxd -r -p | base64`.
        let esperado = "3a81oZNherrMQXNJriBBMRLm+k6JqX6iCp7u5ktV05ohkpkqJ0/BqDa6PCOj/uu9RU1EI2Q86A4qmslPpUyknw==";
        assert_eq!(sha512_base64(b"abc"), esperado);
    }

    #[test]
    fn resolver_url_absoluta_e_relativa() {
        let base = "https://github.com/x/y/releases/download/v1/latest-linux.yml";
        assert_eq!(
            resolver_url(base, "https://outro/arquivo").unwrap(),
            "https://outro/arquivo"
        );
        assert_eq!(
            resolver_url(base, "Arcadia.AppImage").unwrap(),
            "https://github.com/x/y/releases/download/v1/Arcadia.AppImage"
        );
    }

    #[test]
    fn canal_nao_suportado_recusa_check() {
        let mut updater = HttpAutoUpdater::new(
            "1.0.0",
            "portable",
            "https://exemplo/latest.yml",
            "/tmp/nao-usado",
            "/tmp",
        );
        assert_eq!(
            updater.check_for_updates().unwrap_err(),
            "canal_nao_suportado"
        );
    }

    #[test]
    fn pode_escrever_detecta_pasta_somente_leitura() {
        let dir = std::env::temp_dir().join(format!("arcadia-up-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(pode_escrever(&dir));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&dir).unwrap().permissions();
            perms.set_mode(0o500); // r-x------
            std::fs::set_permissions(&dir, perms).unwrap();
            // Root ignora permissão; o teste só vale para usuário comum.
            if libc_geteuid() != 0 {
                assert!(!pode_escrever(&dir), "pasta read-only deveria recusar");
            }
            let mut restore = std::fs::metadata(&dir).unwrap().permissions();
            restore.set_mode(0o700);
            let _ = std::fs::set_permissions(&dir, restore);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    fn libc_geteuid() -> u32 {
        // Evita dependência nova só para um teste.
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status
                    .lines()
                    .find(|line| line.starts_with("Uid:"))
                    .and_then(|line| line.split_whitespace().nth(2))
                    .and_then(|uid| uid.parse().ok())
            })
            .unwrap_or(1000)
    }
}
