//! Contas Steam: leitura de `loginusers.vdf`/`config.vdf` e vínculo com a
//! conta do Arcadia.
//!
//! Porte de `app/electron/vdf.js` (contas) e da parte pura de
//! `app/electron/steam-account.js`. Tudo aqui devolve dados: nenhuma função
//! escreve nos arquivos da Steam. A gravação do vínculo (`steam_account.json`)
//! fica com o chamador, que recebe o link pronto em
//! [`CaptureStatus::vinculo_inicial`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::js::js_trim;
use crate::vdf::{parse, VdfError, VdfObject};

/// `76561197960265728n`: base do SteamID64 para o id de conta (pasta em
/// `userdata/`).
pub const BASE_STEAM_ID: i128 = 76_561_197_960_265_728;
/// Nome do arquivo de vínculo na pasta da conta do Arcadia.
pub const VINCULO: &str = "steam_account.json";
/// Mesma janela do "Capturar agora" do cliente Electron (120 s).
pub const JANELA_FORCA_MS: i64 = 120_000;
/// Componente usado quando o chamador não pede uma passada por nome.
pub const DEFAULT_COMPONENT: &str = "__default__";

/// Uma conta listada em `config/loginusers.vdf`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SteamAccount {
    /// SteamID64 (17 dígitos).
    pub steamid: String,
    pub account_name: String,
    pub persona: String,
    pub remember_password: bool,
    pub most_recent: bool,
    /// `Timestamp` em segundos, quando presente e numérico.
    pub timestamp: Option<i64>,
}

impl SteamAccount {
    fn from_block(steamid: &str, block: &VdfObject) -> Self {
        Self {
            steamid: steamid.to_string(),
            account_name: block
                .get("AccountName")
                .map(|value| value.js_string())
                .unwrap_or_default(),
            persona: block
                .get("PersonaName")
                .map(|value| value.js_string())
                .unwrap_or_default(),
            remember_password: block
                .get("RememberPassword")
                .map(|value| value.js_string() == "1")
                .unwrap_or(false),
            most_recent: block
                .get("MostRecent")
                .map(|value| value.js_string() == "1")
                .unwrap_or(false),
            timestamp: block.get("Timestamp").and_then(timestamp_value),
        }
    }

    /// `true` quando o SteamID64 tem exatamente 17 dígitos ASCII.
    pub fn has_valid_steamid(&self) -> bool {
        is_steamid64(&self.steamid)
    }

    pub fn account_id(&self) -> String {
        steamid_to_account_id(&self.steamid)
    }

    pub fn as_ref(&self) -> AccountRef {
        AccountRef::from(self)
    }
}

fn timestamp_value(value: &crate::vdf::VdfValue) -> Option<i64> {
    let number = value.js_number()?;
    if !number.is_finite() {
        return None;
    }
    Some(number as i64)
}

fn is_steamid64(value: &str) -> bool {
    value.len() == 17 && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Mesma lista de `contasDoLoginUsers`: só chaves de 17 dígitos cujo valor é
/// um bloco.
pub fn login_users_from_root(root: &VdfObject) -> Vec<SteamAccount> {
    let Some(users) = root.get_object("users") else {
        return Vec::new();
    };
    users
        .iter()
        .filter_map(|(steamid, value)| {
            if !is_steamid64(steamid) {
                return None;
            }
            let block = value.as_object()?;
            Some(SteamAccount::from_block(steamid, block))
        })
        .collect()
}

/// Contas de `loginusers.vdf`, na ordem do arquivo.
pub fn parse_login_users(text: &str) -> Result<Vec<SteamAccount>, VdfError> {
    Ok(login_users_from_root(&parse(text)?))
}

/// A conta com `MostRecent=1` (a primeira, se houver mais de uma).
pub fn active_from_accounts(accounts: &[SteamAccount]) -> Option<SteamAccount> {
    accounts.iter().find(|account| account.most_recent).cloned()
}

/// Conta ativa de um texto de `loginusers.vdf`, sem o fallback de primeira
/// conta (espelha `contaAtivaDoLoginUsers`).
pub fn active_login_user(text: &str) -> Result<Option<SteamAccount>, VdfError> {
    Ok(active_from_accounts(&parse_login_users(text)?))
}

/// Conta escolhida por `contaSteamAtiva`: a ativa ou, sem `MostRecent`, a
/// primeira conhecida (`sem_most_recent = true`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedLoginUser {
    pub account: SteamAccount,
    pub sem_most_recent: bool,
}

pub fn select_active_from_accounts(accounts: &[SteamAccount]) -> Option<SelectedLoginUser> {
    if let Some(account) = accounts.iter().find(|account| account.most_recent) {
        return Some(SelectedLoginUser {
            account: account.clone(),
            sem_most_recent: false,
        });
    }
    accounts.first().cloned().map(|account| SelectedLoginUser {
        account,
        sem_most_recent: true,
    })
}

pub fn select_active_login_user(text: &str) -> Result<Option<SelectedLoginUser>, VdfError> {
    Ok(select_active_from_accounts(&parse_login_users(text)?))
}

/// Conta declarada em `config/config.vdf`.
///
/// O bloco real fica em
/// `InstallConfigStore/Software/Valve/Steam/Accounts/<nome>`, com `SteamID`
/// (ou `SteamID64`) dentro. `AutoLoginUser`/`RememberPassword` ficam no nível
/// de `Steam`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigAccounts {
    pub auto_login_user: Option<String>,
    pub remember_password: bool,
    pub accounts: Vec<SteamAccount>,
}

pub fn config_accounts_from_root(root: &VdfObject) -> ConfigAccounts {
    let Some(steam) = root
        .get_object("InstallConfigStore")
        .and_then(|store| store.get_object("Software"))
        .and_then(|software| software.get_object("Valve"))
        .and_then(|valve| valve.get_object("Steam"))
    else {
        return ConfigAccounts::default();
    };

    let auto_login_user = steam
        .get("AutoLoginUser")
        .map(|value| js_trim(&value.js_string()).to_string())
        .filter(|value| !value.is_empty());
    let remember_password = steam
        .get("RememberPassword")
        .map(|value| value.js_string() == "1")
        .unwrap_or(false);

    let accounts = steam
        .get_object("Accounts")
        .map(|accounts| {
            accounts
                .iter()
                .filter_map(|(name, value)| {
                    let block = value.as_object()?;
                    let steamid = ["SteamID", "SteamID64", "SteamId"]
                        .iter()
                        .find_map(|key| {
                            block
                                .get(key)
                                .map(|value| js_trim(&value.js_string()).to_string())
                                .filter(|value| !value.is_empty())
                        })
                        .unwrap_or_default();
                    let mut account = SteamAccount::from_block(&steamid, block);
                    // A chave do bloco é o nome canônico da conta.
                    account.account_name = name.to_string();
                    // O bloco pode omitir a flag; aí vale a do nível de Steam.
                    if !block.contains_key("RememberPassword") {
                        account.remember_password = remember_password;
                    }
                    Some(account)
                })
                .collect()
        })
        .unwrap_or_default();

    ConfigAccounts {
        auto_login_user,
        remember_password,
        accounts,
    }
}

pub fn parse_config_accounts(text: &str) -> Result<ConfigAccounts, VdfError> {
    Ok(config_accounts_from_root(&parse(text)?))
}

/// `config/loginusers.vdf` de uma raiz Steam.
pub fn login_users_path(steam_root: &Path) -> PathBuf {
    steam_root.join("config").join("loginusers.vdf")
}

/// `config/config.vdf` de uma raiz Steam.
pub fn config_vdf_path(steam_root: &Path) -> PathBuf {
    steam_root.join("config").join("config.vdf")
}

/// Pasta `userdata/<id-da-conta>` de um SteamID64.
pub fn userdata_account_dir(steam_root: &Path, steamid: &str) -> Option<PathBuf> {
    let account_id = steamid_to_account_id(steamid);
    if account_id.is_empty() {
        return None;
    }
    Some(steam_root.join("userdata").join(account_id))
}

/// `userdata/<id-da-conta>/config/localconfig.vdf`.
pub fn local_config_path(steam_root: &Path, steamid: &str) -> Option<PathBuf> {
    Some(
        userdata_account_dir(steam_root, steamid)?
            .join("config")
            .join("localconfig.vdf"),
    )
}

/// SteamID64 → id de conta (`BigInt(steamid) - BASE_STEAM_ID`).
/// Devolve `""` quando o texto não é um inteiro válido.
pub fn steamid_to_account_id(steamid: &str) -> String {
    match parse_big_integer(steamid) {
        Some(value) => (value - BASE_STEAM_ID).to_string(),
        None => String::new(),
    }
}

/// Id de conta → SteamID64 (`BASE_STEAM_ID + BigInt(id)`).
pub fn account_id_to_steamid(account_id: &str) -> String {
    match parse_big_integer(account_id) {
        Some(value) => (BASE_STEAM_ID + value).to_string(),
        None => String::new(),
    }
}

/// `BigInt(texto)` para as formas que aparecem nos arquivos (decimal com
/// sinal opcional, hex/octal/binário sem sinal). `i128` cobre com folga o
/// SteamID64 de 17 dígitos.
fn parse_big_integer(text: &str) -> Option<i128> {
    let trimmed = js_trim(text);
    if trimmed.is_empty() {
        // BigInt("") === 0n
        return Some(0);
    }
    let (negative, unsigned) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    if unsigned.is_empty() {
        return None;
    }
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        for candidate in [prefix.to_string(), prefix.to_ascii_uppercase()] {
            if let Some(digits) = unsigned.strip_prefix(candidate.as_str()) {
                // BigInt não aceita sinal em literal com prefixo.
                if negative || digits.is_empty() {
                    return None;
                }
                let mut value: i128 = 0;
                for character in digits.chars() {
                    let digit = character.to_digit(radix)? as i128;
                    value = value.checked_mul(radix as i128)?.checked_add(digit)?;
                }
                return Some(value);
            }
        }
    }
    if !unsigned.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = unsigned.parse::<i128>().ok()?;
    Some(if negative { -value } else { value })
}

// --- vínculo conta Steam × conta Arcadia -----------------------------------

/// Referência pública de uma conta (sem timestamps).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AccountRef {
    pub steamid: String,
    pub persona: String,
}

impl From<&SteamAccount> for AccountRef {
    fn from(account: &SteamAccount) -> Self {
        Self {
            steamid: account.steamid.clone(),
            persona: account.persona.clone(),
        }
    }
}

/// Conteúdo de `steam_account.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub steamid: String,
    pub persona: String,
    #[serde(default)]
    pub vinculado_em: i64,
}

impl Link {
    pub fn from_account(account: &SteamAccount, now_ms: i64) -> Self {
        Self {
            steamid: account.steamid.clone(),
            persona: account.persona.clone(),
            vinculado_em: now_ms,
        }
    }

    pub fn as_ref(&self) -> AccountRef {
        AccountRef {
            steamid: self.steamid.clone(),
            persona: self.persona.clone(),
        }
    }
}

/// `lerVinculo`: objeto JSON com `steamid` não vazio; qualquer outra coisa é
/// tratada como ausência de vínculo.
pub fn parse_link_json(text: &str) -> Option<Link> {
    let link: Link = serde_json::from_str(text).ok()?;
    if link.steamid.is_empty() {
        None
    } else {
        Some(link)
    }
}

/// JSON equivalente ao `JSON.stringify(dados, null, 2)` do cliente.
pub fn link_json(link: &Link) -> String {
    serde_json::to_string_pretty(link).unwrap_or_else(|_| "{}".to_string())
}

/// Entrada de `status()` em `steam-account.js`.
#[derive(Debug, Clone, Copy)]
pub struct CaptureContext<'a> {
    pub active: Option<&'a SteamAccount>,
    pub link: Option<&'a Link>,
    pub all_accounts: &'a [SteamAccount],
    pub auto: bool,
    pub force: bool,
    /// `naoGravar`: só olha o estado, não cria vínculo inicial.
    pub no_write: bool,
    pub now_ms: i64,
}

/// Resultado de `status()`. `vinculo_inicial` é o vínculo que o chamador deve
/// gravar no primeiro uso (o módulo não escreve nada).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureStatus {
    pub vinculo: Option<AccountRef>,
    pub conta_atual: Option<AccountRef>,
    pub permitido: bool,
    pub forcado: bool,
    pub vinculo_ok: bool,
    pub sem_steam: bool,
    pub auto: bool,
    pub contas_steam: Vec<AccountRef>,
    pub vinculo_inicial: Option<Link>,
}

pub fn capture_status(context: &CaptureContext<'_>) -> CaptureStatus {
    let mut effective_link = context.link.cloned();
    let mut initial_link = None;

    // Primeiro uso: amarra na conta Steam logada, como o cliente faz — mas
    // devolve o dado em vez de gravar.
    if effective_link.is_none() && context.active.is_some() && !context.no_write {
        if let Some(active) = context.active {
            let new_link = Link::from_account(active, context.now_ms);
            effective_link = Some(new_link.clone());
            initial_link = Some(new_link);
        }
    }

    let conta_atual = context.active.map(AccountRef::from);
    let vinculo = effective_link.as_ref().map(Link::as_ref);
    let vinculo_ok = match (&conta_atual, &vinculo) {
        (Some(current), Some(linked)) => current.steamid == linked.steamid,
        _ => false,
    };
    let sem_steam = conta_atual.is_none();
    let permitido = context.force || (context.auto && (sem_steam || vinculo_ok));

    CaptureStatus {
        vinculo,
        conta_atual,
        permitido,
        forcado: context.force,
        vinculo_ok,
        sem_steam,
        auto: context.auto,
        contas_steam: context.all_accounts.iter().map(AccountRef::from).collect(),
        vinculo_inicial: initial_link,
    }
}

/// Motivo legível do bloqueio, com as mesmas strings do cliente.
pub fn motivo_pausa(status: &CaptureStatus) -> &'static str {
    if status.forcado {
        return "liberado-pelo-botao";
    }
    if !status.auto {
        return "captura-automatica-desligada";
    }
    if status.conta_atual.is_some() && status.vinculo.is_none() {
        return "sem-vinculo-de-conta";
    }
    if status.conta_atual.is_some() && status.vinculo.is_some() && !status.vinculo_ok {
        return "conta-steam-trocada";
    }
    "regra-de-captura"
}

/// Janela em memória do botão "Capturar agora": uma passada por componente.
#[derive(Debug, Clone, Default)]
pub struct ForceWindow {
    pending: bool,
    expires_ms: i64,
    consumed: HashSet<String>,
}

impl ForceWindow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn force_next(&mut self, now_ms: i64) {
        self.pending = true;
        self.expires_ms = now_ms + JANELA_FORCA_MS;
        self.consumed.clear();
    }

    pub fn is_pending(&self, now_ms: i64) -> bool {
        self.pending && now_ms <= self.expires_ms
    }

    /// Consome a passada de um componente; a segunda chamada devolve `false`.
    pub fn consume(&mut self, component: &str, now_ms: i64) -> bool {
        if !self.pending || now_ms > self.expires_ms {
            self.pending = false;
            return false;
        }
        let component = if component.is_empty() {
            DEFAULT_COMPONENT
        } else {
            component
        };
        if !self.consumed.insert(component.to_string()) {
            return false;
        }
        true
    }
}

/// Primeiro vínculo com a conta Steam logada, sem gravar.
pub fn link_for_active(active: Option<&SteamAccount>, now_ms: i64) -> Option<Link> {
    active.map(|account| Link::from_account(account, now_ms))
}

/// Alvo de troca por SteamID64, nome de conta ou persona.
#[derive(Debug, Clone, Copy)]
pub enum AccountSelector<'a> {
    SteamId(&'a str),
    AccountName(&'a str),
    Persona(&'a str),
    MostRecent,
}

pub fn select_account<'a>(
    accounts: &'a [SteamAccount],
    selector: AccountSelector<'_>,
) -> Option<&'a SteamAccount> {
    match selector {
        AccountSelector::SteamId(steamid) => {
            accounts.iter().find(|account| account.steamid == steamid)
        }
        AccountSelector::AccountName(name) => {
            accounts.iter().find(|account| account.account_name == name)
        }
        AccountSelector::Persona(persona) => {
            accounts.iter().find(|account| account.persona == persona)
        }
        AccountSelector::MostRecent => accounts.iter().find(|account| account.most_recent),
    }
}

pub fn next_account<'a>(
    accounts: &'a [SteamAccount],
    current_steamid: &str,
) -> Option<&'a SteamAccount> {
    if accounts.is_empty() {
        return None;
    }
    let index = accounts
        .iter()
        .position(|account| account.steamid == current_steamid)?;
    Some(&accounts[(index + 1) % accounts.len()])
}

pub fn previous_account<'a>(
    accounts: &'a [SteamAccount],
    current_steamid: &str,
) -> Option<&'a SteamAccount> {
    if accounts.is_empty() {
        return None;
    }
    let index = accounts
        .iter()
        .position(|account| account.steamid == current_steamid)?;
    Some(&accounts[(index + accounts.len() - 1) % accounts.len()])
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTA_A: &str = "76561198000004541";
    const CONTA_B: &str = "76561198000010132";

    const LOGIN_USERS: &str = "\"users\"\n{\n\t\"76561198000010132\"\n\t{\n\t\t\"AccountName\"\t\t\"conta1\"\n\t\t\"PersonaName\"\t\t\"NEGO EJACULADO\"\n\t\t\"RememberPassword\"\t\t\"1\"\n\t\t\"MostRecent\"\t\t\"0\"\n\t\t\"Timestamp\"\t\t\"1700000000\"\n\t}\n\t\"76561198000004541\"\n\t{\n\t\t\"AccountName\"\t\t\"conta2\"\n\t\t\"PersonaName\"\t\t\"Kk\"\n\t\t\"RememberPassword\"\t\t\"1\"\n\t\t\"MostRecent\"\t\t\"1\"\n\t\t\"Timestamp\"\t\t\"1800000000\"\n\t}\n}\n";

    #[test]
    fn parses_loginusers_accounts_persona_and_active() {
        let accounts = parse_login_users(LOGIN_USERS).unwrap();
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].steamid, CONTA_B);
        assert_eq!(accounts[0].persona, "NEGO EJACULADO");
        assert!(!accounts[0].most_recent);
        assert_eq!(accounts[0].account_name, "conta1");
        assert!(accounts[0].remember_password);
        assert_eq!(accounts[0].timestamp, Some(1_700_000_000));

        let active = active_login_user(LOGIN_USERS).unwrap().unwrap();
        assert_eq!(active.steamid, CONTA_A);
        assert_eq!(active.persona, "Kk");
        assert!(active.most_recent);
        assert_eq!(active.timestamp, Some(1_800_000_000));
    }

    #[test]
    fn skips_invalid_steamids_and_non_object_blocks() {
        let text = "\"users\"\n{\n\t\"123\" { \"PersonaName\" \"curto\" }\n\t\"76561198000004541\" \"texto\"\n\t\"76561198000004542\" { \"PersonaName\" \"ok\" }\n}\n";
        let accounts = parse_login_users(text).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].persona, "ok");
    }

    #[test]
    fn most_recent_is_only_the_string_one() {
        let text = "\"users\" { \"76561198000004541\" { \"MostRecent\" \"true\" } \"76561198000004542\" { \"MostRecent\" \"01\" } }";
        let accounts = parse_login_users(text).unwrap();
        assert!(!accounts[0].most_recent);
        assert!(!accounts[1].most_recent);
        assert!(active_login_user(text).unwrap().is_none());
    }

    #[test]
    fn duplicate_persona_becomes_a_comma_joined_string_like_js() {
        let text =
            "\"users\" { \"76561198000004541\" { \"PersonaName\" \"A\" \"PersonaName\" \"B\" } }";
        let accounts = parse_login_users(text).unwrap();
        assert_eq!(accounts[0].persona, "A,B");
    }

    #[test]
    fn falls_back_to_the_first_account_without_most_recent() {
        let text = "\"users\" { \"76561198000004542\" { \"PersonaName\" \"Primeira\" } \"76561198000004541\" { \"PersonaName\" \"Segunda\" } }";
        let selected = select_active_login_user(text).unwrap().unwrap();
        assert_eq!(selected.account.persona, "Primeira");
        assert!(selected.sem_most_recent);
    }

    #[test]
    fn empty_or_missing_users_is_an_empty_list() {
        assert!(parse_login_users("").unwrap().is_empty());
        assert!(parse_login_users("\"outra\" { \"x\" \"1\" }")
            .unwrap()
            .is_empty());
        assert!(select_active_login_user("\"users\" { }").unwrap().is_none());
    }

    #[test]
    fn steamid_conversion_matches_the_client() {
        assert_eq!(steamid_to_account_id("76561198000004541"), "39738813");
        assert_eq!(account_id_to_steamid("39738813"), "76561198000004541");
        assert_eq!(steamid_to_account_id("lixo"), "");
        assert_eq!(account_id_to_steamid("lixo"), "");
        // BigInt("") === 0n no JS.
        assert_eq!(steamid_to_account_id(""), "-76561197960265728");
        assert_eq!(account_id_to_steamid(""), "76561197960265728");
    }

    #[test]
    fn path_helpers_build_the_real_steam_layouts() {
        let root = Path::new("C:\\Steam");
        assert_eq!(
            login_users_path(root),
            Path::new("C:\\Steam\\config\\loginusers.vdf")
        );
        assert_eq!(
            config_vdf_path(root),
            Path::new("C:\\Steam\\config\\config.vdf")
        );
        assert_eq!(
            local_config_path(root, CONTA_A).unwrap(),
            Path::new("C:\\Steam\\userdata\\39738813\\config\\localconfig.vdf")
        );
        assert!(local_config_path(root, "lixo").is_none());
    }

    #[test]
    fn config_vdf_extracts_accounts_and_auto_login() {
        let text = "\"InstallConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"AutoLoginUser\"\t\t\"conta2\"\n\t\t\t\t\"RememberPassword\"\t\t\"1\"\n\t\t\t\t\"Accounts\"\n\t\t\t\t{\n\t\t\t\t\t\"conta1\" { \"SteamID\" \"76561198000010132\" }\n\t\t\t\t\t\"conta2\" { \"SteamID\" \"76561198000004541\" \"PersonaName\" \"Kk\" }\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";
        let config = parse_config_accounts(text).unwrap();
        assert_eq!(config.auto_login_user.as_deref(), Some("conta2"));
        assert!(config.remember_password);
        assert_eq!(config.accounts.len(), 2);
        assert_eq!(config.accounts[0].account_name, "conta1");
        assert_eq!(config.accounts[0].steamid, CONTA_B);
        assert_eq!(config.accounts[1].persona, "Kk");
    }

    #[test]
    fn config_vdf_without_the_steam_section_is_empty() {
        assert_eq!(
            parse_config_accounts("\"outra\" { \"x\" \"1\" }").unwrap(),
            ConfigAccounts::default()
        );
    }

    #[test]
    fn link_json_round_trips_and_ignores_missing_steamid() {
        let link = Link {
            steamid: CONTA_A.to_string(),
            persona: "Kk".to_string(),
            vinculado_em: 1_700_000_000_000,
        };
        let json = link_json(&link);
        assert!(json.contains("\"vinculadoEm\""));
        assert_eq!(parse_link_json(&json), Some(link));
        assert_eq!(parse_link_json("{}"), None);
        assert_eq!(parse_link_json("{\"steamid\":\"\"}"), None);
        assert_eq!(parse_link_json("lixo"), None);
    }

    fn accounts() -> Vec<SteamAccount> {
        parse_login_users(LOGIN_USERS).unwrap()
    }

    fn context<'a>(
        active: Option<&'a SteamAccount>,
        link: Option<&'a Link>,
        all: &'a [SteamAccount],
    ) -> CaptureContext<'a> {
        CaptureContext {
            active,
            link,
            all_accounts: all,
            auto: true,
            force: false,
            no_write: false,
            now_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn first_use_suggests_linking_to_the_logged_account() {
        let all = accounts();
        let active = all.iter().find(|account| account.most_recent).unwrap();
        let status = capture_status(&context(Some(active), None, &all));
        assert_eq!(status.conta_atual.as_ref().unwrap().persona, "Kk");
        assert_eq!(status.vinculo.as_ref().unwrap().persona, "Kk");
        assert!(status.permitido);
        assert_eq!(
            status.vinculo_inicial.as_ref().unwrap().vinculado_em,
            1_700_000_000_000
        );
    }

    #[test]
    fn no_write_does_not_create_the_initial_link() {
        let all = accounts();
        let active = all.iter().find(|account| account.most_recent).unwrap();
        let mut options = context(Some(active), None, &all);
        options.no_write = true;
        let status = capture_status(&options);
        assert!(status.vinculo.is_none());
        assert!(status.vinculo_inicial.is_none());
        assert!(!status.permitido);
        assert_eq!(motivo_pausa(&status), "sem-vinculo-de-conta");
    }

    #[test]
    fn switched_account_pauses_capture_even_with_auto_on() {
        let all = accounts();
        let active = all.iter().find(|account| account.most_recent).unwrap();
        let old_link = Link {
            steamid: CONTA_B.to_string(),
            persona: "NEGO EJACULADO".to_string(),
            vinculado_em: 1,
        };
        let status = capture_status(&context(Some(active), Some(&old_link), &all));
        assert!(!status.permitido);
        assert!(!status.vinculo_ok);
        assert_eq!(motivo_pausa(&status), "conta-steam-trocada");
    }

    #[test]
    fn auto_off_pauses_but_force_releases() {
        let all = accounts();
        let active = all.iter().find(|account| account.most_recent).unwrap();
        let link = Link::from_account(active, 1);
        let mut options = context(Some(active), Some(&link), &all);
        options.auto = false;
        let status = capture_status(&options);
        assert!(!status.permitido);
        assert_eq!(motivo_pausa(&status), "captura-automatica-desligada");

        options.force = true;
        let forced = capture_status(&options);
        assert!(forced.permitido);
        assert_eq!(motivo_pausa(&forced), "liberado-pelo-botao");
    }

    #[test]
    fn missing_steam_is_absence_not_a_switch() {
        let all = accounts();
        let link = Link {
            steamid: CONTA_B.to_string(),
            persona: "NEGO EJACULADO".to_string(),
            vinculado_em: 1,
        };
        let status = capture_status(&context(None, Some(&link), &all));
        assert!(status.sem_steam);
        assert!(status.permitido);
        assert!(!status.vinculo_ok);
    }

    #[test]
    fn force_window_last_one_pass_per_component_and_expires() {
        let mut window = ForceWindow::new();
        assert!(!window.consume("schemas", 100));
        window.force_next(1_000);
        assert!(window.consume("schemas", 1_001));
        assert!(!window.consume("schemas", 1_002));
        assert!(window.consume("vigia-crack", 1_003));
        assert!(window.consume("", 1_004));
        assert!(!window.consume("", 1_005));
        window.force_next(2_000);
        assert!(!window.consume("schemas", 2_000 + JANELA_FORCA_MS + 1));
    }

    #[test]
    fn switching_helpers_find_wrap_and_link() {
        let all = accounts();
        assert_eq!(
            select_account(&all, AccountSelector::SteamId(CONTA_A))
                .unwrap()
                .persona,
            "Kk"
        );
        assert_eq!(
            select_account(&all, AccountSelector::AccountName("conta1"))
                .unwrap()
                .steamid,
            CONTA_B
        );
        assert_eq!(
            select_account(&all, AccountSelector::MostRecent)
                .unwrap()
                .steamid,
            CONTA_A
        );
        assert_eq!(next_account(&all, CONTA_A).unwrap().steamid, CONTA_B);
        assert_eq!(next_account(&all, CONTA_B).unwrap().steamid, CONTA_A);
        assert_eq!(previous_account(&all, CONTA_A).unwrap().steamid, CONTA_B);
        assert!(next_account(&all, "desconhecido").is_none());
        assert_eq!(
            link_for_active(Some(&all[0]), 42).unwrap(),
            Link::from_account(&all[0], 42)
        );
    }
}
