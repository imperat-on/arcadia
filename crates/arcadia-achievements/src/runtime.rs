//! Runtime vivo das conquistas: uma passada determinística que o host agenda.
//!
//! Porte 1:1 do laço do `startSteamBinWatcher` (`steam_bin.js`) somado ao
//! `iniciarVigia` (`cracked_watcher.js`) e ao `loadAllSchemas` (`loader.js`):
//!
//! - o vigia de bin da Steam faz um snapshot inicial SILENCIOSO e só emite
//!   chaves que aparecerem depois (dedupe entre polls e entre arquivos de
//!   contas diferentes);
//! - o loader reconstrói os itens a partir de `UserGameStatsSchema_*.bin`,
//!   aplica o progresso real do bin e enfileira os registros de sync;
//! - o vigia de crack/emulador usa os mesmos candidatos/caminhos/parsers da
//!   crate (`CrackedWatcher`), inclusive a reconciliação de revogação UPC.
//!
//! Nada aqui agenda timers, abre threads, dorme ou faz rede: [`AchievementsRuntime::tick`]
//! devolve os eventos e o host decide quando chamar (15 s e/ou evento de fs com
//! debounce, como o Electron). Filesystem e relógio são injetados
//! ([`WatcherFs`]/[`Clock`]); a escrita do `achievements.json` usa o
//! `save_atomic` já existente do loader.
//!
//! # Gate de captura
//!
//! O oráculo só ingere/revoga quando a conta Steam logada é a vinculada e a
//! captura automática está ligada. [`CaptureGate`] representa essa decisão. Um
//! gate bloqueado faz `tick` devolver `skipped = true` sem tocar em snapshot,
//! store ou fila. [`AchievementsRuntime::force_next_capture`] libera UMA
//! passada (equivalente ao `forcarProximaCaptura` do botão "Capturar agora").
//! `catch_up` é mais fiel ao `loadAllSchemas`: com o gate bloqueado ele ainda
//! reconstrói o schema (sem ler o progresso), exatamente como o JS.
//!
//! # Fila
//!
//! Desbloqueios viram [`QueueRecord`] na [`SyncQueue`] do
//! [`AchievementManager`] (dedupe por `appid|apiname`, earliest-wins). O host
//! drena com [`AchievementsRuntime::take_queue`] e envia ao servidor.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::cracked_watcher::{
    extrair_appid, library_games, resolve_prefix, settings_for_entry, CrackEnvironment, CrackGroup,
    CrackedWatcher, Revocation, ScanGameOptions, UnlockPayload,
};
use crate::events::{AchievementEvent, RevokeEvent, UnlockEvent, TOAST_WINDOW_SECONDS};
use crate::fsx::{Clock, Platform, SystemClock, SystemFs, WatcherFs};
use crate::js::{js_number_value, js_string_value, js_truthy_value};
use crate::loader::{
    configured_appids, load_all_schemas_with_fs, AchievementsStore, LoadOptions, RebuildReport,
    SteamIcons,
};
use crate::manager::{AchievementManager, AchievementRecord, QueueRecord, SyncQueue};
use crate::schema::{parse_progress_filename, schema_from_bytes, schema_path, AchievementSchema};
use crate::steam_bin::{json_block_bit_key, progress_map_bytes};

/// Decisão de captura (equivalente ao `capturaPermitida` do `steam-account.js`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureGate {
    /// Captura automática ligada E conta Steam vinculada (ou sem Steam).
    pub allowed: bool,
    /// Botão "Capturar agora": ignora a troca de conta/interruptor.
    pub forced: bool,
}

impl CaptureGate {
    /// Captura liberada pela regra normal.
    pub const fn open() -> Self {
        Self {
            allowed: true,
            forced: false,
        }
    }

    /// Captura pausada (conta trocada ou captura automática desligada).
    pub const fn blocked() -> Self {
        Self {
            allowed: false,
            forced: false,
        }
    }

    /// Liberação explícita (persistente enquanto o gate durar).
    pub const fn forced() -> Self {
        Self {
            allowed: true,
            forced: true,
        }
    }

    pub const fn is_allowed(self) -> bool {
        self.allowed || self.forced
    }
}

impl Default for CaptureGate {
    fn default() -> Self {
        Self::open()
    }
}

impl From<bool> for CaptureGate {
    fn from(allowed: bool) -> Self {
        Self {
            allowed,
            forced: false,
        }
    }
}

/// Desbloqueio encontrado no bin sem entrada correspondente no
/// `achievements.json` (o JS emite um `console.warn`, não um toast).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnindexedUnlock {
    pub appid: String,
    pub key: String,
    pub unlock: i64,
    pub file: String,
}

impl UnindexedUnlock {
    pub fn to_value(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("appid".into(), Value::String(self.appid.clone()));
        object.insert("key".into(), Value::String(self.key.clone()));
        object.insert("unlock".into(), Value::from(self.unlock));
        object.insert("file".into(), Value::String(self.file.clone()));
        Value::Object(object)
    }
}

/// Resultado de uma passada de [`AchievementsRuntime::tick`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TickReport {
    /// `true` quando o gate de captura bloqueou a passada (nada foi lido).
    pub skipped: bool,
    /// `true` quando a passada foi liberada por força (gate ou one-shot).
    pub forced: bool,
    pub scanned_appids: usize,
    pub scanned_files: usize,
    /// `true` quando o store local mudou e foi persistido.
    pub changed: bool,
    pub events: Vec<AchievementEvent>,
    pub unindexed: Vec<UnindexedUnlock>,
    pub errors: Vec<String>,
}

impl TickReport {
    pub fn unlocks(&self) -> impl Iterator<Item = &UnlockEvent> {
        self.events.iter().filter_map(AchievementEvent::as_unlock)
    }

    pub fn revocations(&self) -> impl Iterator<Item = &RevokeEvent> {
        self.events.iter().filter_map(AchievementEvent::as_revoke)
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Resultado de [`AchievementsRuntime::catch_up`]/[`AchievementsRuntime::load_schemas`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CatchUpReport {
    /// O gate bloqueou a leitura do progresso (o schema foi reconstruído mesmo
    /// assim, como no `loadAllSchemas`).
    pub skipped_progress: bool,
    pub forced: bool,
    pub updated: usize,
    pub icons_copied: usize,
    pub total_appids: usize,
    pub total: usize,
    /// Registros efetivamente novos na fila (dedupe aplicado).
    pub sync_queued: usize,
    pub apps: Vec<RebuildReport>,
    pub errors: Vec<String>,
}

impl CatchUpReport {
    pub fn changed(&self) -> bool {
        self.updated > 0
    }
}

#[derive(Debug, Clone)]
struct ProgressFile {
    appid: String,
    path: PathBuf,
    progress: BTreeMap<String, i64>,
}

#[derive(Debug, Clone)]
struct PendingUnlock {
    appid: String,
    key: String,
    apiname: String,
    provider: String,
    title: String,
    desc: String,
    icon: String,
    icongray: String,
    hidden: bool,
    percent: f64,
    unlock: i64,
}

impl PendingUnlock {
    fn into_event(self, done: usize, total: usize) -> UnlockEvent {
        UnlockEvent {
            appid: self.appid,
            key: self.key,
            apiname: self.apiname,
            provider: self.provider,
            title: self.title,
            desc: self.desc,
            icon: self.icon,
            icongray: self.icongray,
            hidden: self.hidden,
            percent: self.percent,
            unlock: self.unlock,
            local_unlock: true,
            done,
            total,
        }
    }

    fn into_record(self) -> AchievementRecord {
        let mut record = AchievementRecord::new(self.appid, self.apiname);
        record.title = self.title;
        record.desc = self.desc;
        record.icon = self.icon;
        record.percent = self.percent;
        record.achieved = true;
        record.unlocked_at = self.unlock.max(0);
        record.local_unlock = true;
        record.provider = Some(self.provider);
        record
    }
}

/// Runtime das conquistas. Uma instância equivale ao par
/// `startSteamBinWatcher` + `iniciarVigia` de uma conta.
pub struct AchievementsRuntime {
    data_dir: PathBuf,
    account: String,
    steam_root: PathBuf,
    appids: Vec<String>,
    fs: Arc<dyn WatcherFs>,
    clock: Arc<dyn Clock>,
    platform: Platform,
    crack_env: CrackEnvironment,
    compatdata: PathBuf,
    library: Value,
    settings: Value,
    gate: CaptureGate,
    force_pending: bool,
    force_expires_ms: i64,
    force_consumed: BTreeSet<String>,
    baseline_pending: bool,
    snapshots: BTreeMap<String, BTreeSet<String>>,
    schemas: BTreeMap<String, AchievementSchema>,
    watcher: CrackedWatcher,
    manager: AchievementManager,
    allow_revoke: bool,
}

impl AchievementsRuntime {
    /// Constrói o runtime com filesystem/relógio injetados. `account` vazio
    /// significa guest (arquivos na raiz de `data_dir`, como o `conta.js`).
    pub fn new(
        data_dir: impl Into<PathBuf>,
        account: impl Into<String>,
        steam_root: impl Into<PathBuf>,
        appids: impl IntoIterator<Item = impl Into<String>>,
        fs: Arc<dyn WatcherFs>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let data_dir = data_dir.into();
        let account = account.into().trim().to_string();
        let steam_root = steam_root.into();
        let compatdata = steam_root.join("steamapps").join("compatdata");
        let mut ids = BTreeSet::new();
        for appid in appids {
            let appid = appid.into();
            let normalized = crate::r#match::normalize_appid(&appid).unwrap_or(appid);
            if !normalized.is_empty() {
                ids.insert(normalized);
            }
        }
        let mut runtime = Self {
            data_dir,
            account,
            steam_root,
            appids: ids.into_iter().collect(),
            fs,
            clock,
            platform: Platform::current(),
            crack_env: CrackEnvironment::from_current(),
            compatdata,
            library: Value::Object(serde_json::Map::new()),
            settings: Value::Object(serde_json::Map::new()),
            gate: CaptureGate::open(),
            force_pending: false,
            force_expires_ms: 0,
            force_consumed: BTreeSet::new(),
            baseline_pending: true,
            snapshots: BTreeMap::new(),
            schemas: BTreeMap::new(),
            watcher: CrackedWatcher::new("__guest__"),
            manager: AchievementManager::new(),
            allow_revoke: false,
        };
        runtime.watcher.account = runtime.account.clone();
        runtime.reload_config();
        runtime
    }

    /// Atalho para o host real (fs/relógio do sistema).
    pub fn system(
        data_dir: impl Into<PathBuf>,
        account: impl Into<String>,
        steam_root: impl Into<PathBuf>,
        appids: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self::new(
            data_dir,
            account,
            steam_root,
            appids,
            Arc::new(SystemFs),
            Arc::new(SystemClock),
        )
    }

    // ── Caminhos/configuração ──────────────────────────────────────────────

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn account(&self) -> &str {
        &self.account
    }

    /// `data_dir` para guest, `data_dir/contas/<account>` quando logado.
    pub fn account_dir(&self) -> PathBuf {
        if self.account.is_empty() {
            self.data_dir.clone()
        } else {
            self.data_dir.join("contas").join(&self.account)
        }
    }

    pub fn achievements_path(&self) -> PathBuf {
        self.account_dir().join("achievements.json")
    }

    pub fn steam_root(&self) -> &Path {
        &self.steam_root
    }

    /// `<steam>/appcache/stats`, a pasta dos `UserGameStats*.bin`.
    pub fn stats_dir(&self) -> PathBuf {
        self.steam_root.join("appcache").join("stats")
    }

    pub fn appids(&self) -> &[String] {
        &self.appids
    }

    pub fn set_appids(&mut self, appids: impl IntoIterator<Item = impl Into<String>>) {
        let mut ids = BTreeSet::new();
        for appid in appids {
            let appid = appid.into();
            let normalized = crate::r#match::normalize_appid(&appid).unwrap_or(appid);
            if !normalized.is_empty() {
                ids.insert(normalized);
            }
        }
        self.appids = ids.into_iter().collect();
    }

    pub fn add_appid(&mut self, appid: impl Into<String>) {
        let appid = appid.into();
        let normalized = crate::r#match::normalize_appid(&appid).unwrap_or(appid);
        if !normalized.is_empty() && !self.appids.contains(&normalized) {
            self.appids.push(normalized);
            self.appids.sort();
        }
    }

    pub fn library(&self) -> &Value {
        &self.library
    }

    pub fn settings(&self) -> &Value {
        &self.settings
    }

    pub fn set_library(&mut self, library: Value) {
        self.library = library;
    }

    pub fn set_settings(&mut self, settings: Value) {
        self.settings = settings;
    }

    /// Lê `library.json` (raiz de dados) e `game_settings.json` (conta) pelo
    /// filesystem injetado, como o `lerLibrary` do JS. Appids `steam:<id>` dos
    /// dois arquivos entram na lista monitorada.
    pub fn reload_config(&mut self) {
        self.library = self
            .fs
            .read_to_string(&self.data_dir.join("library.json"))
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        self.settings = self
            .fs
            .read_to_string(&self.account_dir().join("game_settings.json"))
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        let mut ids: BTreeSet<String> = self.appids.iter().cloned().collect();
        for appid in configured_appids(&self.library, &self.settings) {
            ids.insert(appid);
        }
        self.appids = ids.into_iter().collect();
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    pub fn set_platform(&mut self, platform: Platform) {
        self.platform = platform;
    }

    pub fn crack_environment(&self) -> &CrackEnvironment {
        &self.crack_env
    }

    pub fn set_crack_environment(&mut self, environment: CrackEnvironment) {
        self.crack_env = environment;
    }

    /// Pasta `compatdata` usada para montar o prefixo Wine no Linux.
    pub fn compatdata(&self) -> &Path {
        &self.compatdata
    }

    pub fn set_compatdata(&mut self, compatdata: impl Into<PathBuf>) {
        self.compatdata = compatdata.into();
    }

    // ── Gate de captura ────────────────────────────────────────────────────

    pub fn capture_gate(&self) -> CaptureGate {
        self.gate
    }

    pub fn set_capture_gate(&mut self, gate: impl Into<CaptureGate>) {
        self.gate = gate.into();
    }

    /// Equivalente ao `forcarProximaCaptura`: libera UMA passada de cada
    /// consumidor (`schemas` no `catch_up`, `tick` no vigia), como o
    /// `consumirForca(componente)` do JS. Expira sozinho em 120 s.
    pub fn force_next_capture(&mut self) {
        self.force_pending = true;
        self.force_expires_ms = self.clock.now_ms().saturating_add(120_000);
        self.force_consumed.clear();
    }

    pub fn allow_revoke(&self) -> bool {
        self.allow_revoke
    }

    /// O `main.js` só revoga depois do primeiro pull de sync. O host liga isto
    /// quando essa condição for verdadeira.
    pub fn set_allow_revoke(&mut self, allow: bool) {
        self.allow_revoke = allow;
    }

    // ── Store/fila ─────────────────────────────────────────────────────────

    /// Lê o `achievements.json` da conta pelo filesystem injetado.
    /// `Math.floor(Date.now()/1000)` pelo relógio injetado.
    pub fn now_seconds(&self) -> i64 {
        (self.clock.now_ms() as f64 / 1000.0).floor() as i64
    }

    /// Mesma decisão de toast do `main.js` (`JANELA_TOAST_S`).
    pub fn should_toast(&self, event: &UnlockEvent) -> bool {
        event.is_recent(self.now_seconds(), TOAST_WINDOW_SECONDS)
    }

    /// Lê o `achievements.json` da conta pelo filesystem injetado. Se a
    /// implementação virtual não expõe o arquivo, cai no caminho real — a
    /// escrita atômica (`save_store`) sempre usa o disco.
    pub fn load_store(&self) -> AchievementsStore {
        let path = self.achievements_path();
        match self.fs.read_to_string(&path) {
            Some(text) => AchievementsStore::from_json(&text),
            None => AchievementsStore::load(path),
        }
    }

    /// Grava atomicamente (tmp + rename) — mesmo contrato do `saveAchievements`.
    pub fn save_store(&self, store: &AchievementsStore) -> io::Result<()> {
        store.save_atomic(self.achievements_path())
    }

    pub fn manager(&self) -> &AchievementManager {
        &self.manager
    }

    pub fn manager_mut(&mut self) -> &mut AchievementManager {
        &mut self.manager
    }

    pub fn queue(&self) -> &SyncQueue {
        self.manager.queue()
    }

    /// Drena a fila na ordem canônica (`appid|apiname`).
    pub fn take_queue(&mut self) -> Vec<QueueRecord> {
        let records: Vec<QueueRecord> = self
            .manager
            .queue()
            .records()
            .into_iter()
            .cloned()
            .collect();
        self.manager.queue_mut().clear();
        records
    }

    // ── Passadas ───────────────────────────────────────────────────────────

    /// Passada do vigia (Steam + crack/emulador). Sem timers: o host agenda.
    pub fn tick(&mut self) -> TickReport {
        let forced = self.take_force("tick") || self.gate.forced;
        if !self.gate.allowed && !forced {
            return TickReport {
                skipped: true,
                ..TickReport::default()
            };
        }

        let mut report = TickReport {
            forced,
            scanned_appids: self.appids.len(),
            ..TickReport::default()
        };
        let mut store = self.load_store();
        let mut pending: Vec<PendingUnlock> = Vec::new();
        let mut revocations: Vec<RevokeEvent> = Vec::new();
        let mut store_changed = false;

        let files = self.progress_files();
        report.scanned_files = files.len();
        if self.baseline_pending {
            // Snapshot inicial silencioso (o `snap` do startSteamBinWatcher).
            self.baseline_pending = false;
            for file in &files {
                self.snapshots
                    .insert(file.appid.clone(), file.progress.keys().cloned().collect());
            }
        } else {
            for (appid, group) in group_by_appid(&files) {
                let mut previous = self.snapshots.get(&appid).cloned().unwrap_or_default();
                let mut new_keys: Vec<(String, i64, String)> = Vec::new();
                for file in group {
                    let path = file.path.to_string_lossy().into_owned();
                    for (key, unlock) in &file.progress {
                        if previous.contains(key) {
                            continue;
                        }
                        previous.insert(key.clone());
                        new_keys.push((key.clone(), *unlock, path.clone()));
                    }
                }
                self.snapshots.insert(appid.clone(), previous);
                if new_keys.is_empty() {
                    continue;
                }
                let mut items = store.items(&appid);
                if items.is_empty() {
                    for (key, unlock, file) in new_keys {
                        if unlock > 0 {
                            report.unindexed.push(UnindexedUnlock {
                                appid: appid.clone(),
                                key,
                                unlock,
                                file,
                            });
                        }
                    }
                    continue;
                }
                let mut changed_app = false;
                for (key, unlock, file) in new_keys {
                    if unlock <= 0 {
                        continue;
                    }
                    let Some(position) = items
                        .iter()
                        .position(|item| item_key(item).as_deref() == Some(key.as_str()))
                    else {
                        report.unindexed.push(UnindexedUnlock {
                            appid: appid.clone(),
                            key,
                            unlock,
                            file,
                        });
                        continue;
                    };
                    if item_achieved(&items[position]) {
                        continue;
                    }
                    let pending_unlock =
                        self.pending_from_item(&appid, &key, &items[position], unlock, "steam");
                    if let Some(object) = items[position].as_object_mut() {
                        object.insert("achieved".into(), Value::Bool(true));
                        object.insert("unlock".into(), Value::from(unlock));
                        object.insert("localUnlock".into(), Value::Bool(true));
                    }
                    changed_app = true;
                    pending.push(pending_unlock);
                }
                if changed_app {
                    store.set_items(&appid, items);
                    store_changed = true;
                }
            }
        }

        // Vigia de crack/emulador: mesmos candidatos/parsers do oráculo.
        let (cracked_pending, cracked_revocations, cracked_changed) = self.scan_cracked(&mut store);
        pending.extend(cracked_pending);
        revocations.extend(cracked_revocations);
        store_changed |= cracked_changed;

        // Dedupe dentro da passada (mesma conquista pelo bin e pelo crack).
        let mut seen = BTreeSet::new();
        pending.retain(|event| seen.insert((event.appid.clone(), event.key.clone())));
        let mut seen_revokes = BTreeSet::new();
        revocations
            .retain(|event| seen_revokes.insert((event.appid.clone(), event.apiname.clone())));

        if store_changed {
            if let Err(error) = self.save_store(&store) {
                report
                    .errors
                    .push(format!("save achievements.json: {error}"));
            }
        }

        for event in &pending {
            if !event.apiname.is_empty() {
                self.manager.observe(event.clone().into_record());
            }
        }
        for event in &revocations {
            self.manager.revoke(&event.appid, &event.apiname);
        }

        let mut events = Vec::with_capacity(pending.len() + revocations.len());
        for event in pending {
            let (done, total) = done_total(&store, &event.appid);
            events.push(AchievementEvent::Unlocked(event.into_event(done, total)));
        }
        for event in revocations {
            events.push(AchievementEvent::Revoked(event));
        }

        report.events = events;
        report.changed = store_changed;
        report
    }

    /// Alias de [`Self::tick`] (o nome usado pelo laço de polling do host).
    pub fn poll(&mut self) -> TickReport {
        self.tick()
    }

    /// Passada com o gate informado (aceita `CaptureGate` ou `bool`).
    pub fn tick_with_gate(&mut self, gate: impl Into<CaptureGate>) -> TickReport {
        self.set_capture_gate(gate);
        self.tick()
    }

    /// Rebuild dos schemas + progresso (`loadAllSchemas`), sem rede. Também
    /// prepara o snapshot do vigia para o próximo `tick` não repetir toasts.
    pub fn catch_up(&mut self) -> CatchUpReport {
        let forced = self.take_force("schemas") || self.gate.forced;
        let allowed = self.gate.allowed || forced;

        let mut store = self.load_store();
        let stats_dir = self.stats_dir();
        let icons = SteamIcons::new(
            stats_dir.join("achievement_images"),
            self.data_dir.join("achievement_icons"),
        );
        let extra_appids = self.appids.clone();
        let options = LoadOptions {
            stats_dir: &stats_dir,
            permission_allowed: allowed,
            icons: &icons,
            scrape: None,
            extra_appids: &extra_appids,
        };
        let load = load_all_schemas_with_fs(&mut store, &options, self.fs.as_ref());

        let mut errors = Vec::new();
        if load.updated > 0 {
            if let Err(error) = self.save_store(&store) {
                errors.push(format!("save achievements.json: {error}"));
            }
        }
        let queue_before = self.manager.queue().len();
        for app in &load.apps {
            self.manager.enqueue_sync_records(&app.sync_records);
        }
        let sync_queued = self.manager.queue().len().saturating_sub(queue_before);

        self.prime_snapshots();
        self.schemas.clear();

        CatchUpReport {
            skipped_progress: !allowed,
            forced,
            updated: load.updated,
            icons_copied: load.icons_copied,
            total_appids: load.total_appids,
            total: load.total,
            sync_queued,
            apps: load.apps,
            errors,
        }
    }

    /// Alias histórico do `loadAllSchemas`.
    pub fn load_schemas(&mut self) -> CatchUpReport {
        self.catch_up()
    }

    /// `catch_up` com o gate informado (aceita `CaptureGate` ou `bool`).
    pub fn catch_up_with_gate(&mut self, gate: impl Into<CaptureGate>) -> CatchUpReport {
        self.set_capture_gate(gate);
        self.catch_up()
    }

    // ── Internos ───────────────────────────────────────────────────────────

    /// Consome a força pendente para UM componente (janela de 120 s, como
    /// `consumirForca`).
    fn take_force(&mut self, component: &str) -> bool {
        if !self.force_pending {
            return false;
        }
        if self.clock.now_ms() > self.force_expires_ms {
            self.force_pending = false;
            self.force_consumed.clear();
            return false;
        }
        if !self.force_consumed.insert(component.to_string()) {
            return false;
        }
        true
    }

    /// Arquivos `UserGameStats_<steamid>_<appid>.bin` dos appids monitorados.
    /// Nomes são ordenados para a passada ser determinística.
    fn progress_files(&self) -> Vec<ProgressFile> {
        let stats_dir = self.stats_dir();
        let Some(mut names) = self.fs.read_dir(&stats_dir) else {
            return Vec::new();
        };
        names.sort();
        let wanted: BTreeSet<&str> = self.appids.iter().map(String::as_str).collect();
        let mut files = Vec::new();
        for name in names {
            let Some((_steamid, appid)) = parse_progress_filename(&name) else {
                continue;
            };
            if !wanted.contains(appid) {
                continue;
            }
            let path = stats_dir.join(&name);
            let progress = self
                .fs
                .read_bytes(&path)
                .map(|bytes| progress_map_bytes(&bytes))
                .unwrap_or_default();
            files.push(ProgressFile {
                appid: appid.to_string(),
                path,
                progress,
            });
        }
        files
    }

    /// Snapshot dos bins atuais, sem emitir eventos (usado pelo `catch_up`).
    fn prime_snapshots(&mut self) {
        self.baseline_pending = false;
        for file in self.progress_files() {
            self.snapshots
                .insert(file.appid, file.progress.keys().cloned().collect());
        }
    }

    fn schema_hidden(&mut self, appid: &str, key: &str) -> bool {
        if !self.schemas.contains_key(appid) {
            let schema = self
                .fs
                .read_bytes(&schema_path(self.stats_dir(), appid))
                .map(|bytes| schema_from_bytes(appid, &bytes))
                .unwrap_or_else(|| AchievementSchema::new(appid));
            self.schemas.insert(appid.to_string(), schema);
        }
        self.schemas
            .get(appid)
            .and_then(|schema| schema.get(key))
            .map(|achievement| achievement.hidden)
            .unwrap_or(false)
    }

    fn pending_from_item(
        &mut self,
        appid: &str,
        key: &str,
        item: &Value,
        unlock: i64,
        provider: &str,
    ) -> PendingUnlock {
        let hidden = item.get("hidden").map(js_truthy_value).unwrap_or(false)
            || self.schema_hidden(appid, key);
        PendingUnlock {
            appid: appid.to_string(),
            key: key.to_string(),
            apiname: item_text(item, "apiname"),
            provider: provider.to_string(),
            title: item_text(item, "title"),
            desc: item_text(item, "desc"),
            icon: item_text(item, "icon"),
            icongray: item_text(item, "icongray"),
            hidden,
            percent: item
                .get("percent")
                .and_then(js_number_value)
                .filter(|value| value.is_finite())
                .unwrap_or(0.0),
            unlock,
        }
    }

    fn pending_from_payload(&mut self, payload: &UnlockPayload, items: &[Value]) -> PendingUnlock {
        let item = items
            .iter()
            .find(|item| item_key(item).as_deref() == Some(payload.key.as_str()))
            .or_else(|| {
                items
                    .iter()
                    .find(|item| item_text(item, "apiname").eq_ignore_ascii_case(&payload.apiname))
            });
        let hidden = item
            .and_then(|item| item.get("hidden"))
            .map(js_truthy_value)
            .unwrap_or(false)
            || self.schema_hidden(&payload.appid, &payload.key);
        PendingUnlock {
            appid: payload.appid.clone(),
            key: payload.key.clone(),
            apiname: payload.apiname.clone(),
            provider: payload.provider.clone(),
            title: payload.title.clone(),
            desc: payload.desc.clone(),
            icon: payload.icon.clone(),
            icongray: item
                .map(|item| item_text(item, "icongray"))
                .unwrap_or_default(),
            hidden,
            percent: payload.percent,
            unlock: payload.unlock,
        }
    }

    /// Varre os jogos configurados com o `CrackedWatcher` (Goldberg, CODEX,
    /// FLT, Uplay/UPC...). Devolve eventos; a revogação UPC só sai quando
    /// [`Self::allow_revoke`] está ligado, como o `lastPullAt` do JS.
    fn scan_cracked(
        &mut self,
        store: &mut AchievementsStore,
    ) -> (Vec<PendingUnlock>, Vec<RevokeEvent>, bool) {
        let mut pending = Vec::new();
        let mut revocations = Vec::new();
        let mut changed = false;
        if self.appids.is_empty() {
            return (pending, revocations, changed);
        }
        let games = library_games(&self.library, &self.settings);
        for appid in self.appids.clone() {
            let Some(entry) = games
                .iter()
                .find(|game| extrair_appid(game).as_deref() == Some(appid.as_str()))
                .cloned()
            else {
                continue;
            };
            if !entry.get("installed").map(js_truthy_value).unwrap_or(false) {
                continue;
            }
            let settings = settings_for_entry(&self.settings, &entry, &appid);
            let prefix = resolve_prefix(&appid, &settings, &entry, &self.compatdata, None);
            let mut items = store.items(&appid);
            if items.is_empty() {
                continue; // sem schema não há o que detectar
            }
            let scan = {
                let fs = self.fs.as_ref();
                let clock = self.clock.as_ref();
                let environment = &self.crack_env;
                let options = ScanGameOptions {
                    fs,
                    clock,
                    appid: &appid,
                    entry: &entry,
                    settings: &settings,
                    platform: self.platform,
                    prefix: &prefix,
                    env: environment,
                    allow_revoke: self.allow_revoke,
                };
                self.watcher.scan_game(&options, &mut items)
            };
            for payload in &scan.unlocks {
                pending.push(self.pending_from_payload(payload, &items));
            }
            for revocation in &scan.revocations {
                revocations.push(revoke_event(&appid, revocation, &items));
            }
            if scan.changed {
                store.set_items(&appid, items);
                changed = true;
            }
        }
        (pending, revocations, changed)
    }
}

fn group_by_appid(files: &[ProgressFile]) -> BTreeMap<String, Vec<&ProgressFile>> {
    let mut grouped: BTreeMap<String, Vec<&ProgressFile>> = BTreeMap::new();
    for file in files {
        grouped.entry(file.appid.clone()).or_default().push(file);
    }
    grouped
}

fn revoke_event(appid: &str, revocation: &Revocation, items: &[Value]) -> RevokeEvent {
    let item = items
        .iter()
        .find(|item| item_text(item, "apiname").eq_ignore_ascii_case(&revocation.apiname));
    RevokeEvent {
        appid: appid.to_string(),
        apiname: revocation.apiname.clone(),
        key: item.and_then(item_key).unwrap_or_default(),
        title: item
            .map(|item| item_text(item, "title"))
            .unwrap_or_default(),
        provider: CrackGroup::Upc.provider_name().to_string(),
        reason: "upc-runtime".to_string(),
    }
}

fn done_total(store: &AchievementsStore, appid: &str) -> (usize, usize) {
    let items = store.items(appid);
    let total = items.len();
    let done = items.iter().filter(|item| item_achieved(item)).count();
    (done, total)
}

fn item_achieved(item: &Value) -> bool {
    item.get("achieved").map(js_truthy_value).unwrap_or(false)
}

fn item_key(item: &Value) -> Option<String> {
    let object = item.as_object()?;
    let block = object.get("block").filter(|value| !value.is_null())?;
    let bit = object.get("bit").filter(|value| !value.is_null())?;
    json_block_bit_key(block, bit)
}

fn item_text(item: &Value, field: &str) -> String {
    item.get(field)
        .filter(|value| js_truthy_value(value))
        .map(js_string_value)
        .unwrap_or_default()
}
