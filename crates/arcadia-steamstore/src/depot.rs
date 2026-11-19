//! Install/depot info: `.lua` dos provedores, PICS do `api.steamcmd.net`,
//! enriquecimento de depots, regras de OS/idioma/DLC, installdir e tamanhos.
//!
//! Porte de `getManifest`, `lerLuas`, `fetchAppInfo`, `fetchAppName`,
//! `enriquecerMetadata` e `sanitizeInstallDir` de
//! `app/electron/steamstore.js`, mais as regras de agrupamento/seleção do
//! `DepotPicker.tsx`.
//!
//! O oráculo baixa zips de provedores e extrai `.lua`/`.manifest` via
//! subprocesso. Aqui o conteúdo extraído é **injetado** ([`ProviderManifest`]);
//! o crate não spawna processo nem baixa arquivo. O resultado tem a mesma forma
//! de `store:installInfo`, exceto `outDir`, que só existe depois da extração e
//! fica na borda.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use arcadia_catalog::http::{Fetcher, Request};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// `api.steamcmd.net` — PICS público.
pub const APP_INFO_BASE: &str = "https://api.steamcmd.net/v1/info";
/// Timeout de `fetchAppInfo`.
pub const APP_INFO_TIMEOUT: Duration = Duration::from_secs(15);
/// Timeout de `fetchAppName`.
pub const APP_NAME_TIMEOUT: Duration = Duration::from_secs(10);
/// Branch padrão dos manifestos.
pub const DEFAULT_BRANCH: &str = "public";

/// Snapshot de um branch de manifesto dentro do PICS.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestBranch {
    #[serde(default)]
    pub manifest_id: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub download: u64,
}

/// Depot no formato do `store:installInfo` (campos opcionais só aparecem
/// depois do enriquecimento, como no oráculo).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepotInfo {
    #[serde(rename = "depotId")]
    pub depot_id: String,
    #[serde(rename = "manifestId", default)]
    pub manifest_id: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(rename = "dlcAppid", default, skip_serializing_if = "Option::is_none")]
    pub dlc_appid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
    /// Manifestos por branch (preenchido a partir do PICS; não serializa, o
    /// contrato de `store:installInfo` é o campo `manifestId` plano).
    #[serde(skip)]
    pub manifests: BTreeMap<String, ManifestBranch>,
}

impl DepotInfo {
    pub fn from_lua(depot: &LuaDepot) -> Self {
        Self {
            depot_id: depot.depot_id.clone(),
            manifest_id: depot.manifest_id.clone(),
            key: depot.key.clone(),
            size: depot.size,
            ..Self::default()
        }
    }

    pub fn is_shared(&self) -> bool {
        self.shared.unwrap_or(false)
    }

    pub fn manifest_for(&self, branch: &str) -> Option<&ManifestBranch> {
        self.manifests.get(branch)
    }
}

/// Branch do app (buildid/descrição).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    #[serde(default)]
    pub buildid: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub time_updated: u64,
    #[serde(default)]
    pub pwd_required: bool,
}

/// PICS parser de um appid.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    pub appid: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub installdir: String,
    #[serde(default)]
    pub list_of_dlc: Vec<String>,
    #[serde(default)]
    pub depots: Vec<DepotInfo>,
    #[serde(default)]
    pub branches: Vec<BranchInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buildid: Option<String>,
}

impl AppInfo {
    pub fn branch(&self, name: &str) -> Option<&BranchInfo> {
        self.branches.iter().find(|branch| branch.name == name)
    }

    pub fn public_buildid(&self) -> Option<&str> {
        self.branch(DEFAULT_BRANCH)
            .map(|branch| branch.buildid.as_str())
            .filter(|buildid| !buildid.is_empty())
            .or(self
                .buildid
                .as_deref()
                .filter(|buildid| !buildid.is_empty()))
    }

    pub fn depot(&self, depot_id: &str) -> Option<&DepotInfo> {
        self.depots.iter().find(|depot| depot.depot_id == depot_id)
    }

    pub fn depot_mut(&mut self, depot_id: &str) -> Option<&mut DepotInfo> {
        self.depots
            .iter_mut()
            .find(|depot| depot.depot_id == depot_id)
    }
}

/// `addappid(...)`/`setManifestid(...)` de um `.lua`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LuaDepot {
    #[serde(rename = "depotId")]
    pub depot_id: String,
    #[serde(rename = "manifestId")]
    pub manifest_id: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub size: u64,
}

/// Conteúdo agregado dos `.lua` de um provedor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LuaManifest {
    #[serde(default)]
    pub depots: Vec<LuaDepot>,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub dlcs: Vec<String>,
}

/// Provedor já extraído. `manifests_present` contém os depotIds cujo
/// `<depot>_<manifest>.manifest` existir no diretório do provedor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderManifest {
    pub nome: String,
    pub depots: Vec<LuaDepot>,
    pub token: String,
    pub dlcs: Vec<String>,
    pub manifests_present: HashSet<String>,
}

/// Resultado do merge entre provedores (mesma estratégia do `getManifest`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergedManifest {
    #[serde(default)]
    pub depots: Vec<DepotInfo>,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub dlcs: Vec<String>,
    #[serde(default)]
    pub fonte: String,
    #[serde(default)]
    pub skipped: Vec<String>,
    #[serde(default)]
    pub contribuidores: Vec<String>,
}

/// Envelope de `store:installInfo`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstallInfoResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depots: Option<Vec<DepotInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dlcs: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fonte: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installdir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Nome histórico do contrato.
pub type InstallInfo = InstallInfoResponse;

impl InstallInfoResponse {
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            appid: None,
            depots: None,
            token: None,
            dlcs: None,
            fonte: None,
            installdir: None,
            error: Some(message.into()),
        }
    }
}

/// Cliente PICS (`api.steamcmd.net`).
#[derive(Clone)]
pub struct DepotClient {
    fetcher: Arc<dyn Fetcher>,
    app_info_base: String,
    app_info_timeout: Duration,
    app_name_timeout: Duration,
}

impl DepotClient {
    pub fn new(fetcher: Arc<dyn Fetcher>) -> Self {
        Self {
            fetcher,
            app_info_base: APP_INFO_BASE.to_string(),
            app_info_timeout: APP_INFO_TIMEOUT,
            app_name_timeout: APP_NAME_TIMEOUT,
        }
    }

    pub fn from_base_url(api_base: &str) -> Result<Self, arcadia_catalog::http::FetchError> {
        let fetcher = arcadia_catalog::http::HttpClientFetcher::from_base_url(api_base)?;
        Ok(Self::new(Arc::new(fetcher)))
    }

    pub fn with_app_info_base(mut self, base: impl Into<String>) -> Self {
        self.app_info_base = base.into();
        self
    }

    pub fn with_timeouts(mut self, app_info: Duration, app_name: Duration) -> Self {
        self.app_info_timeout = app_info;
        self.app_name_timeout = app_name;
        self
    }

    pub fn fetcher(&self) -> &Arc<dyn Fetcher> {
        &self.fetcher
    }

    fn info_url(&self, appid: &str) -> String {
        format!(
            "{}/{}",
            self.app_info_base.trim_end_matches('/'),
            appid.trim()
        )
    }

    async fn fetch_json(&self, url: &str, timeout: Duration) -> Option<Value> {
        let request = Request::get(url)
            .timeout(timeout)
            .header("User-Agent", "arcadia");
        let response = self.fetcher.fetch(request).await.ok()?;
        if !response.is_success() {
            return None;
        }
        response.json_value().ok()
    }

    /// `fetchAppInfo`: devolve o PICS pronto; falha de rede vira `None`.
    pub async fn app_info(&self, appid: &str) -> Option<AppInfo> {
        let appid = appid.trim();
        if !crate::store::valid_appid(appid) {
            return None;
        }
        let data = self
            .fetch_json(&self.info_url(appid), self.app_info_timeout)
            .await?;
        parse_app_info(&data, appid)
    }

    /// `fetchAppName`.
    pub async fn app_name(&self, appid: &str) -> Option<String> {
        let appid = appid.trim();
        if !crate::store::valid_appid(appid) {
            return None;
        }
        let data = self
            .fetch_json(&self.info_url(appid), self.app_name_timeout)
            .await?;
        let name = data
            .pointer(&format!("/data/{appid}/common/name"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        }
    }

    /// Alias com o nome do oráculo (`fetchAppName`).
    pub async fn fetch_app_name(&self, appid: &str) -> Option<String> {
        self.app_name(appid).await
    }

    /// Alias com o nome do oráculo (`getManifest`).
    pub async fn get_manifest(
        &self,
        appid: &str,
        providers: &[ProviderManifest],
    ) -> InstallInfoResponse {
        self.install_info(appid, providers).await
    }

    /// `store:installInfo` sem baixar zips: usa os provedores já extraídos.
    /// Mantém o `manifestId` do provedor, como o oráculo; para trocar pelo
    /// manifesto do PICS de um branch use [`Self::install_info_for_branch`].
    pub async fn install_info(
        &self,
        appid: &str,
        providers: &[ProviderManifest],
    ) -> InstallInfoResponse {
        let merged = merge_provider_manifests(providers);
        let app = self.app_info(appid).await;
        let dlcs = match app.as_ref() {
            Some(app) => self.load_dlc_infos(app).await,
            None => Vec::new(),
        };
        install_info_from_merged(appid, merged, app.as_ref(), &dlcs)
    }

    /// Variante com branch explícito (`public`, `beta`…).
    pub async fn install_info_for_branch(
        &self,
        appid: &str,
        providers: &[ProviderManifest],
        branch: &str,
    ) -> InstallInfoResponse {
        let merged = merge_provider_manifests(providers);
        let app = self.app_info(appid).await;
        let dlcs = match app.as_ref() {
            Some(app) => self.load_dlc_infos(app).await,
            None => Vec::new(),
        };
        let mut response = install_info_from_merged(appid, merged, app.as_ref(), &dlcs);
        if let (true, Some(app), Some(depots)) =
            (response.ok, app.as_ref(), response.depots.as_mut())
        {
            apply_branch_manifests(depots, app, branch);
        }
        response
    }

    /// PICS dos DLCs listados e dos `dlcappid` diretos, como o
    /// `enriquecerMetadata` (cada falha é ignorada).
    async fn load_dlc_infos(&self, app: &AppInfo) -> Vec<AppInfo> {
        let mut ids = app.list_of_dlc.clone();
        for depot in &app.depots {
            if let Some(dlc) = depot.dlc_appid.as_deref().filter(|dlc| !dlc.is_empty()) {
                if !ids.iter().any(|id| id == dlc) {
                    ids.push(dlc.to_string());
                }
            }
        }
        let mut infos = Vec::new();
        for id in ids {
            if let Some(info) = self.app_info(&id).await {
                infos.push(info);
            }
        }
        infos
    }

    /// Variante pura: metadados do app e DLCs já carregados.
    pub fn install_info_with_metadata(
        &self,
        appid: &str,
        providers: &[ProviderManifest],
        app: Option<&AppInfo>,
        dlc_infos: &[AppInfo],
    ) -> InstallInfoResponse {
        install_info_from_merged(appid, merge_provider_manifests(providers), app, dlc_infos)
    }

    /// Buildid do branch (default `public`) — usado pela checagem de update.
    pub async fn latest_buildid(&self, appid: &str, branch: &str) -> Option<String> {
        self.app_info(appid)
            .await
            .and_then(|info| latest_build_from_app_info(&info, branch))
    }
}

/// `sanitizeInstallDir`: remove traversal e caracteres inválidos; vazio vira
/// `"jogo"`. `.`, `_`, espaço e `-` são permitidos (igual ao oráculo).
pub fn sanitize_install_dir(installdir: &str) -> String {
    let filtered = installdir
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ' ' | '-')
        })
        .collect::<String>();
    let without_dots = filtered.replace("..", "");
    let trimmed = without_dots.trim();
    if trimmed.is_empty() {
        "jogo".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Nome de pasta sugerido pela UI (`title.replace(/[^A-Za-z0-9]/g, "")`).
pub fn install_dir_default(title: &str) -> String {
    title.chars().filter(char::is_ascii_alphanumeric).collect()
}

/// `lerLuas(outDir, appid)` sobre um arquivo.
pub fn parse_lua_manifest(text: &str, appid: &str) -> LuaManifest {
    static KEY_RE: OnceLock<Regex> = OnceLock::new();
    static MAN_RE: OnceLock<Regex> = OnceLock::new();
    static TOKEN_RE: OnceLock<Regex> = OnceLock::new();
    static DLC_RE: OnceLock<Regex> = OnceLock::new();
    let key_re = KEY_RE.get_or_init(|| {
        Regex::new(r#"addappid\(\s*(\d+)\s*,\s*\d+\s*,\s*"([0-9a-f]+)"\s*\)"#)
            .expect("regex addappid com chave")
    });
    let man_re = MAN_RE.get_or_init(|| {
        Regex::new(r#"setManifestid\(\s*(\d+)\s*,\s*"(\d+)"\s*(?:,\s*(\d+))?"#)
            .expect("regex setManifestid")
    });
    let token_re = TOKEN_RE
        .get_or_init(|| Regex::new(r#"addtoken\(\s*"([^"]+)"\s*\)"#).expect("regex addtoken"));
    let dlc_re =
        DLC_RE.get_or_init(|| Regex::new(r"addappid\(\s*(\d+)\s*\)").expect("regex addappid"));

    let mut keys = std::collections::HashMap::new();
    for capture in key_re.captures_iter(text) {
        keys.insert(capture[1].to_string(), capture[2].to_string());
    }
    let mut depots = Vec::new();
    for capture in man_re.captures_iter(text) {
        let depot_id = capture[1].to_string();
        depots.push(LuaDepot {
            key: keys.get(&depot_id).cloned().unwrap_or_default(),
            depot_id: depot_id.clone(),
            manifest_id: capture[2].to_string(),
            size: capture
                .get(3)
                .and_then(|value| value.as_str().parse::<u64>().ok())
                .unwrap_or(0),
        });
    }
    let token = token_re
        .captures(text)
        .map(|capture| capture[1].to_string())
        .unwrap_or_default();
    let mut dlcs = Vec::new();
    for capture in dlc_re.captures_iter(text) {
        let id = capture[1].to_string();
        if id != appid && !dlcs.contains(&id) {
            dlcs.push(id);
        }
    }
    LuaManifest {
        depots,
        token,
        dlcs,
    }
}

/// `lerLuas` sobre vários arquivos `(nome, conteúdo)`, na ordem de leitura.
pub fn parse_lua_files(files: &[(String, String)], appid: &str) -> LuaManifest {
    let mut result = LuaManifest::default();
    for (_, contents) in files {
        let parsed = parse_lua_manifest(contents, appid);
        result.depots.extend(parsed.depots);
        if !parsed.token.is_empty() {
            result.token = parsed.token;
        }
        for dlc in parsed.dlcs {
            if !result.dlcs.contains(&dlc) {
                result.dlcs.push(dlc);
            }
        }
    }
    result
}

/// `fetchAppInfo`: parser do JSON do PICS.
pub fn parse_app_info(data: &Value, appid: &str) -> Option<AppInfo> {
    let app = data
        .pointer(&format!("/data/{appid}"))
        .or_else(|| data.get(appid))
        .or_else(|| {
            data.get("data")
                .and_then(|value| value.as_object())
                .and_then(|object| object.values().next())
        })
        .or_else(|| {
            if data.get("common").is_some() || data.get("depots").is_some() {
                Some(data)
            } else {
                None
            }
        })?;
    if !app.is_object() {
        return None;
    }
    let name = app
        .pointer("/common/name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let installdir = app
        .pointer("/config/installdir")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let list_of_dlc = app
        .pointer("/extended/listofdlc")
        .and_then(Value::as_str)
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    let raw_depots = app.get("depots");
    let mut depots = Vec::new();
    if let Some(object) = raw_depots.and_then(Value::as_object) {
        for (id, value) in object {
            if id == "branches" || !value.is_object() {
                continue;
            }
            let mut manifests = BTreeMap::new();
            if let Some(raw) = value.get("manifests").and_then(Value::as_object) {
                for (branch, manifest) in raw {
                    manifests.insert(
                        branch.clone(),
                        ManifestBranch {
                            manifest_id: string_truthy(manifest.get("gid")),
                            size: manifest.get("size").and_then(Value::as_u64).unwrap_or(0),
                            download: manifest
                                .get("download")
                                .and_then(Value::as_u64)
                                .unwrap_or(0),
                        },
                    );
                }
            }
            let public = manifests.get(DEFAULT_BRANCH);
            let config = value.get("config");
            let os = config
                .and_then(|config| config.get("oslist"))
                .map(json_string)
                .unwrap_or_default();
            let language = config
                .and_then(|config| config.get("language"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            depots.push(DepotInfo {
                depot_id: id.clone(),
                manifest_id: public
                    .map(|manifest| manifest.manifest_id.clone())
                    .unwrap_or_default(),
                key: String::new(),
                size: public
                    .map(|manifest| manifest.size)
                    .or_else(|| value.get("maxsize").and_then(Value::as_u64))
                    .unwrap_or(0),
                name: Some(
                    value
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                ),
                os: Some(os),
                language: Some(language),
                dlc_appid: Some(string_truthy(value.get("dlcappid"))),
                shared: Some(truthy(value.get("sharedinstall"))),
                optional: Some(truthy(value.get("optional"))),
                manifests,
            });
        }
    }

    let mut branches = Vec::new();
    if let Some(object) = raw_depots
        .and_then(|depots| depots.get("branches"))
        .and_then(Value::as_object)
    {
        for (name, value) in object {
            branches.push(BranchInfo {
                name: name.clone(),
                buildid: string_truthy(value.get("buildid")),
                description: value
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                time_updated: value
                    .get("timeupdated")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                pwd_required: truthy(value.get("pwdrequired")),
            });
        }
    }
    let buildid = branches
        .iter()
        .find(|branch| branch.name == DEFAULT_BRANCH)
        .map(|branch| branch.buildid.clone())
        .filter(|buildid| !buildid.is_empty());

    Some(AppInfo {
        appid: appid.to_string(),
        name,
        installdir,
        list_of_dlc,
        depots,
        branches,
        buildid,
    })
}

/// `enriquecerMetadata`: cruza os depots do `.lua` com o PICS do jogo e dos
/// DLCs (os/apps/idioma/nome/dlcAppid/shared/optional).
pub fn enrich_depots(depots: &mut [DepotInfo], game: &AppInfo, dlcs: &[AppInfo]) {
    let by_id = game
        .depots
        .iter()
        .map(|depot| (depot.depot_id.as_str(), depot))
        .collect::<std::collections::HashMap<_, _>>();
    for depot in depots.iter_mut() {
        let Some(meta) = by_id.get(depot.depot_id.as_str()) else {
            continue;
        };
        depot.os = meta.os.clone();
        depot.language = meta.language.clone();
        depot.dlc_appid = meta.dlc_appid.clone();
        depot.shared = meta.shared;
        depot.optional = meta.optional;
        depot.name = Some(if meta.name.as_deref().unwrap_or("").is_empty() {
            if game.name.is_empty() {
                depot.depot_id.clone()
            } else {
                game.name.clone()
            }
        } else {
            meta.name.clone().unwrap_or_default()
        });
    }

    // DLCs listados em extended.listofdlc: seus depots aparecem no manifesto do
    // jogo base sem dlcappid; o nome vem do PICS do DLC.
    let dlc_by_id = dlcs
        .iter()
        .map(|dlc| (dlc.appid.as_str(), dlc))
        .collect::<std::collections::HashMap<_, _>>();
    for dlc_id in &game.list_of_dlc {
        let Some(dlc) = dlc_by_id.get(dlc_id.as_str()) else {
            continue;
        };
        let dlc_name = if dlc.name.is_empty() {
            format!("DLC {dlc_id}")
        } else {
            dlc.name.clone()
        };
        for depot in depots.iter_mut() {
            let Some(meta) = dlc.depot(&depot.depot_id) else {
                continue;
            };
            depot.dlc_appid = Some(dlc_id.clone());
            depot.name = Some(dlc_name.clone());
            if depot.os.as_deref().unwrap_or("").is_empty() {
                depot.os = meta.os.clone();
            }
            if depot.language.as_deref().unwrap_or("").is_empty() {
                depot.language = meta.language.clone();
            }
        }
    }

    // Nomes de DLCs marcados diretamente pelo PICS do jogo base.
    let direct = depots
        .iter()
        .filter_map(|depot| depot.dlc_appid.clone())
        .filter(|dlc| !dlc.is_empty() && !game.list_of_dlc.contains(dlc))
        .collect::<BTreeSet<_>>();
    for dlc_id in direct {
        let Some(dlc) = dlc_by_id.get(dlc_id.as_str()) else {
            continue;
        };
        if dlc.name.is_empty() {
            continue;
        }
        for depot in depots.iter_mut() {
            if depot.dlc_appid.as_deref() == Some(dlc_id.as_str()) {
                depot.name = Some(dlc.name.clone());
            }
        }
    }
}

/// Merge da cascata de provedores: líder define a ordem e a versão preferida;
/// os demais completam manifestos ausentes. Depots sem `.manifest` local vão
/// para `skipped`.
pub fn merge_provider_manifests(providers: &[ProviderManifest]) -> MergedManifest {
    if providers.is_empty() {
        return MergedManifest::default();
    }
    let leader = &providers[0];
    let leader_complete = leader
        .depots
        .iter()
        .all(|depot| leader.manifests_present.contains(&depot.depot_id));
    if leader_complete && !leader.depots.is_empty() {
        // Early-exit do oráculo: o primeiro provedor cobre todos os seus
        // depots com `.manifest` binário — ele ganha sozinho.
        return MergedManifest {
            depots: leader.depots.iter().map(DepotInfo::from_lua).collect(),
            token: leader.token.clone(),
            dlcs: leader.dlcs.clone(),
            fonte: leader.nome.clone(),
            skipped: Vec::new(),
            contribuidores: vec![leader.nome.clone()],
        };
    }
    let mut result = MergedManifest {
        skipped: Vec::new(),
        contribuidores: Vec::new(),
        ..MergedManifest::default()
    };
    let mut seen = HashSet::new();
    for provider in providers {
        for depot in &provider.depots {
            if !seen.insert(depot.depot_id.clone()) {
                continue;
            }
            let mut choice: Option<(LuaDepot, &str)> = leader
                .depots
                .iter()
                .find(|candidate| candidate.depot_id == depot.depot_id)
                .filter(|candidate| leader.manifests_present.contains(&candidate.depot_id))
                .map(|candidate| (candidate.clone(), leader.nome.as_str()));
            if choice.is_none() {
                for other in providers {
                    let Some(candidate) = other
                        .depots
                        .iter()
                        .find(|candidate| candidate.depot_id == depot.depot_id)
                    else {
                        continue;
                    };
                    if !other.manifests_present.contains(&candidate.depot_id) {
                        continue;
                    }
                    let key = if candidate.key.is_empty() {
                        providers
                            .iter()
                            .filter_map(|provider| {
                                provider
                                    .depots
                                    .iter()
                                    .find(|candidate| candidate.depot_id == depot.depot_id)
                                    .map(|candidate| candidate.key.clone())
                                    .filter(|key| !key.is_empty())
                            })
                            .next()
                            .unwrap_or_default()
                    } else {
                        candidate.key.clone()
                    };
                    choice = Some((
                        LuaDepot {
                            depot_id: candidate.depot_id.clone(),
                            manifest_id: candidate.manifest_id.clone(),
                            key,
                            size: candidate.size,
                        },
                        other.nome.as_str(),
                    ));
                    break;
                }
            }
            let Some((candidate, source)) = choice else {
                result.skipped.push(depot.depot_id.clone());
                continue;
            };
            if !result.contribuidores.iter().any(|name| name == source) {
                result.contribuidores.push(source.to_string());
            }
            result.depots.push(DepotInfo::from_lua(&candidate));
        }
    }
    result.token = if !leader.token.is_empty() {
        leader.token.clone()
    } else {
        providers
            .iter()
            .map(|provider| provider.token.clone())
            .find(|token| !token.is_empty())
            .unwrap_or_default()
    };
    result.dlcs = if !leader.dlcs.is_empty() {
        leader.dlcs.clone()
    } else {
        providers
            .iter()
            .map(|provider| provider.dlcs.clone())
            .find(|dlcs| !dlcs.is_empty())
            .unwrap_or_default()
    };
    let extras = result
        .contribuidores
        .iter()
        .filter(|name| name.as_str() != leader.nome)
        .cloned()
        .collect::<Vec<_>>();
    result.fonte = if extras.is_empty() {
        leader.nome.clone()
    } else {
        format!("{} (+manifests de {})", leader.nome, extras.join(","))
    };
    result
}

/// `installInfo` para um branch específico: aplica os manifestos do PICS
/// (`manifests.<branch>.gid/size`) sobre os depots do merge.
pub fn apply_branch_manifests(depots: &mut [DepotInfo], app: &AppInfo, branch: &str) -> usize {
    let mut updated = 0;
    for depot in depots.iter_mut() {
        let Some(meta) = app.depot(&depot.depot_id) else {
            continue;
        };
        let Some(manifest) = meta.manifest_for(branch) else {
            continue;
        };
        if manifest.manifest_id.is_empty() {
            continue;
        }
        depot.manifest_id = manifest.manifest_id.clone();
        depot.size = manifest.size;
        updated += 1;
    }
    updated
}

/// Monta o envelope final de `store:installInfo`.
pub fn install_info_from_merged(
    appid: &str,
    merged: MergedManifest,
    app: Option<&AppInfo>,
    dlcs: &[AppInfo],
) -> InstallInfoResponse {
    if merged.depots.is_empty() {
        return InstallInfoResponse::failure("nenhum provedor disponível");
    }
    let mut depots = merged.depots;
    if let Some(app) = app {
        enrich_depots(&mut depots, app, dlcs);
    }
    InstallInfoResponse {
        ok: true,
        appid: Some(appid.to_string()),
        depots: Some(depots),
        token: Some(merged.token),
        dlcs: Some(merged.dlcs),
        fonte: Some(merged.fonte),
        installdir: app
            .map(|app| app.installdir.clone())
            .filter(|dir| !dir.is_empty()),
        error: None,
    }
}

// ── Depot picker ──────────────────────────────────────────────────────────

/// Tipo do grupo na tela (`DepotPicker.tsx`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepotGroupKind {
    Base,
    Idioma,
    Dlc,
    SemMeta,
}

impl DepotGroupKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Idioma => "idioma",
            Self::Dlc => "dlc",
            Self::SemMeta => "sem_meta",
        }
    }
}

/// Grupo exibido pelo seletor de depots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepotGroup {
    pub id: String,
    pub tipo: DepotGroupKind,
    pub titulo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    pub depots: Vec<DepotInfo>,
}

fn depot_has_meta(depot: &DepotInfo) -> bool {
    depot.os.as_deref().is_some_and(|os| !os.is_empty())
        || depot
            .language
            .as_deref()
            .is_some_and(|language| !language.is_empty())
        || depot
            .dlc_appid
            .as_deref()
            .is_some_and(|dlc| !dlc.is_empty())
}

fn language_title(code: &str) -> String {
    let mut chars = code.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `agrupar(depots, t)`.
pub fn depot_groups(depots: &[DepotInfo]) -> Vec<DepotGroup> {
    if depots.iter().all(|depot| !depot_has_meta(depot)) {
        return vec![DepotGroup {
            id: "sem_meta".to_string(),
            tipo: DepotGroupKind::SemMeta,
            titulo: "sem_meta".to_string(),
            os: None,
            depots: depots.to_vec(),
        }];
    }
    let mut groups: Vec<DepotGroup> = Vec::new();
    for depot in depots {
        if depot.is_shared() || depot.os.as_deref() == Some("macos") || !depot_has_meta(depot) {
            continue;
        }
        let (kind, title) =
            if let Some(dlc) = depot.dlc_appid.as_deref().filter(|dlc| !dlc.is_empty()) {
                (
                    DepotGroupKind::Dlc,
                    depot
                        .name
                        .clone()
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| format!("DLC {dlc}")),
                )
            } else if let Some(language) = depot
                .language
                .as_deref()
                .filter(|language| !language.is_empty())
            {
                (DepotGroupKind::Idioma, language_title(language))
            } else {
                (DepotGroupKind::Base, "Base".to_string())
            };
        let os = depot.os.clone().filter(|os| !os.is_empty());
        let id = format!(
            "{}|{}|{}",
            kind.as_str(),
            title,
            os.clone().unwrap_or_default()
        );
        if let Some(group) = groups.iter_mut().find(|group| group.id == id) {
            group.depots.push(depot.clone());
        } else {
            groups.push(DepotGroup {
                id,
                tipo: kind,
                titulo: title,
                os,
                depots: vec![depot.clone()],
            });
        }
    }
    groups.sort_by_key(|group| match group.tipo {
        DepotGroupKind::Base => 0,
        DepotGroupKind::Idioma => 1,
        DepotGroupKind::Dlc => 2,
        DepotGroupKind::SemMeta => 3,
    });
    groups
}

/// `idiomaPreferido`: pt-BR tenta brasileiro antes de português; fallback
/// inglês.
pub fn preferred_language(depots: &[DepotInfo], ui_language: &str) -> String {
    let candidates: &[&str] = match ui_language {
        "pt-BR" => &["brazilian", "portuguese"],
        "es-ES" => &["spanish"],
        "en-US" => &["english"],
        _ => &[],
    };
    for candidate in candidates {
        if depots.iter().any(|depot| {
            depot
                .language
                .as_deref()
                .is_some_and(|language| language == *candidate)
        }) {
            return (*candidate).to_string();
        }
    }
    "english".to_string()
}

/// `essencial`: base do OS atual (Windows via Proton), sem DLC/idioma.
pub fn essential_selection(depots: &[DepotInfo]) -> BTreeSet<String> {
    let mut selected = BTreeSet::new();
    for depot in depots {
        let has_dlc = depot
            .dlc_appid
            .as_deref()
            .is_some_and(|dlc| !dlc.is_empty());
        let has_language = depot
            .language
            .as_deref()
            .is_some_and(|language| !language.is_empty());
        if has_dlc || has_language {
            continue;
        }
        let os = depot.os.as_deref().unwrap_or("");
        if os.is_empty() || os == "windows" {
            selected.insert(depot.depot_id.clone());
        }
    }
    selected
}

/// `padrao`: essencial + idioma do usuário (inglês se o jogo não tiver).
pub fn default_selection(depots: &[DepotInfo], ui_language: &str) -> BTreeSet<String> {
    let language = preferred_language(depots, ui_language);
    let mut selected = essential_selection(depots);
    for depot in depots {
        if depot.language.as_deref() != Some(language.as_str()) {
            continue;
        }
        let os = depot.os.as_deref().unwrap_or("");
        if os.is_empty() || os == "windows" {
            selected.insert(depot.depot_id.clone());
        }
    }
    selected
}

/// Soma de `size` dos depots escolhidos.
pub fn selected_size(depots: &[DepotInfo], selection: &BTreeSet<String>) -> u64 {
    depots
        .iter()
        .filter(|depot| selection.contains(&depot.depot_id))
        .fold(0_u64, |total, depot| total.saturating_add(depot.size))
}

/// `sizeGiB`: bytes -> GiB (mesma conta da UI).
pub fn size_gib(bytes: u64) -> f64 {
    bytes as f64 / 1024_f64.powi(3)
}

/// Depots escolhidos, na ordem original.
pub fn selected_depots<'a>(
    depots: &'a [DepotInfo],
    selection: &BTreeSet<String>,
) -> Vec<&'a DepotInfo> {
    depots
        .iter()
        .filter(|depot| selection.contains(&depot.depot_id))
        .collect()
}

/// Conveniência para `latest_buildid` sem rede.
pub fn latest_build_from_app_info(info: &AppInfo, branch: &str) -> Option<String> {
    info.branch(branch)
        .map(|branch| branch.buildid.clone())
        .filter(|buildid| !buildid.is_empty())
        .or_else(|| {
            if branch == DEFAULT_BRANCH {
                info.buildid.clone().filter(|buildid| !buildid.is_empty())
            } else {
                None
            }
        })
}

/// Dados extras do seletor para a borda (tudo serializável).
pub fn depot_picker_json(depots: &[DepotInfo], ui_language: &str) -> Value {
    let groups = depot_groups(depots);
    let essential = essential_selection(depots);
    let selected = default_selection(depots, ui_language);
    json!({
        "grupos": groups,
        "essenciais": essential,
        "selecionados": selected,
        "total_bytes": selected_size(depots, &selected),
        "total_gib": size_gib(selected_size(depots, &selected)),
        "idioma": preferred_language(depots, ui_language),
    })
}

// ── auxiliares ────────────────────────────────────────────────────────────

fn string_truthy(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(true)) => "true".to_string(),
        Some(Value::Bool(false)) => String::new(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => String::new(),
    }
}

fn json_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(","),
        other => string_truthy(Some(other)),
    }
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(boolean)) => *boolean,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

#[cfg(test)]
#[path = "depot_tests.rs"]
mod tests;
