//! Estado do vigia e fila de sincronização de conquistas.
//!
//! O cliente Electron espalha essas decisões entre `manager.js`
//! (`hasSteamBinFor`), `sync.js` (fila com dedupe/earliest-wins) e o callback de
//! revogação do `cracked_watcher.js`. Aqui elas ficam num único estado puro:
//!
//! `Pending` → detecção local ainda não confirmada no `achievements.json`;
//! `Reconciled` → loader/store já contém o item;
//! `Revoked` → runtime UPC disse que esta máquina não desbloqueou.
//!
//! A fila deduplica por `(appid, apiname)`, mantém `achieved` monotônico e usa
//! o menor timestamp positivo (earliest-wins).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::js::{js_number_value, js_string_value, js_truthy_value};
use crate::loader::{ItemIndex, SyncRecord};
use crate::steam_bin::json_block_bit_key;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AchievementState {
    Pending,
    Reconciled,
    Revoked,
}

impl AchievementState {
    /// Transições aceitas: pendente confirma ou é revogado; reconciliado pode
    /// ser revogado; revogado volta a pendente se houver nova evidência local.
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::Reconciled)
                | (Self::Pending, Self::Revoked)
                | (Self::Reconciled, Self::Revoked)
                | (Self::Revoked, Self::Pending)
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AchievementRecord {
    pub appid: String,
    pub apiname: String,
    pub title: String,
    pub desc: String,
    pub icon: String,
    pub percent: f64,
    pub achieved: bool,
    pub unlocked_at: i64,
    pub local_unlock: bool,
    pub provider: Option<String>,
    pub state: AchievementState,
    pub attempts: u32,
}

impl AchievementRecord {
    pub fn new(appid: impl Into<String>, apiname: impl Into<String>) -> Self {
        Self {
            appid: appid.into(),
            apiname: apiname.into(),
            title: String::new(),
            desc: String::new(),
            icon: String::new(),
            percent: 0.0,
            achieved: false,
            unlocked_at: 0,
            local_unlock: false,
            provider: None,
            state: AchievementState::Pending,
            attempts: 0,
        }
    }

    /// Constrói a partir do payload do watcher (`UnlockPayload`/toast).
    pub fn from_unlock_payload(payload: &crate::cracked_watcher::UnlockPayload) -> Self {
        Self {
            appid: payload.appid.clone(),
            apiname: payload.apiname.clone(),
            title: payload.title.clone(),
            desc: payload.desc.clone(),
            icon: payload.icon.clone(),
            percent: payload.percent,
            achieved: payload.unlock != 0,
            unlocked_at: payload.unlock.max(0),
            local_unlock: true,
            provider: Some(payload.provider.clone()),
            state: AchievementState::Pending,
            attempts: 0,
        }
    }

    pub fn key(&self) -> String {
        format!("{}|{}", self.appid, self.apiname)
    }

    pub fn is_pending(&self) -> bool {
        self.state == AchievementState::Pending
    }

    pub fn is_revoked(&self) -> bool {
        self.state == AchievementState::Revoked
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Observation {
    pub inserted: bool,
    pub changed: bool,
}

/// Payload do vigia de bin da Steam (sem `provider`/`apiname`, como o JS).
#[derive(Debug, Clone, PartialEq)]
pub struct SteamUnlockPayload {
    pub appid: String,
    pub key: String,
    pub title: String,
    pub desc: String,
    pub icon: String,
    pub percent: f64,
    pub unlock: i64,
}

impl SteamUnlockPayload {
    pub fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert("appid".into(), Value::String(self.appid.clone()));
        object.insert("key".into(), Value::String(self.key.clone()));
        object.insert("title".into(), Value::String(self.title.clone()));
        object.insert("desc".into(), Value::String(self.desc.clone()));
        object.insert("icon".into(), Value::String(self.icon.clone()));
        object.insert(
            "percent".into(),
            serde_json::Number::from_f64(self.percent)
                .map(Value::Number)
                .unwrap_or_else(|| Value::from(0)),
        );
        object.insert("unlock".into(), Value::from(self.unlock));
        Value::Object(object)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SteamBinDecision {
    pub unlocks: Vec<SteamUnlockPayload>,
    /// Progresso novo sem item no índice (achievements.json ausente/incompleto).
    pub unindexed: Vec<String>,
}

/// Snapshot `appid -> Set("block|bit")` do vigia de bin da Steam.
///
/// A primeira leitura é o snapshot inicial e NÃO dispara toasts: só chaves que
/// aparecerem depois de `SteamBinWatcher::new` viram evento. Reobservar o mesmo
/// progresso é no-op.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SteamBinWatcher {
    pub appid: String,
    snapshot: BTreeSet<String>,
}

impl SteamBinWatcher {
    pub fn new(appid: impl Into<String>, progress: &BTreeMap<String, i64>) -> Self {
        Self {
            appid: appid.into(),
            snapshot: progress.keys().cloned().collect(),
        }
    }

    pub fn snapshot(&self) -> &BTreeSet<String> {
        &self.snapshot
    }

    /// Compara o progresso atual com o snapshot e devolve só as chaves novas.
    pub fn observe(
        &mut self,
        progress: &BTreeMap<String, i64>,
        index: &ItemIndex,
    ) -> SteamBinDecision {
        let mut decision = SteamBinDecision::default();
        for (key, unlock) in progress {
            if self.snapshot.contains(key) {
                continue;
            }
            self.snapshot.insert(key.clone());
            if *unlock <= 0 {
                continue;
            }
            match index.get(&self.appid, key) {
                Some(item) => {
                    let text = |field: &str| {
                        item.get(field)
                            .filter(|value| js_truthy_value(value))
                            .map(js_string_value)
                            .unwrap_or_default()
                    };
                    decision.unlocks.push(SteamUnlockPayload {
                        appid: self.appid.clone(),
                        key: key.clone(),
                        title: text("title"),
                        desc: text("desc"),
                        icon: text("icon"),
                        percent: item
                            .get("percent")
                            .and_then(js_number_value)
                            .filter(|value| value.is_finite())
                            .unwrap_or(0.0),
                        unlock: *unlock,
                    });
                }
                None => decision.unindexed.push(key.clone()),
            }
        }
        decision
    }

    /// Índice usado para o payload no formato do bin: `block|bit`.
    pub fn key_for(item: &Value) -> Option<String> {
        let object = item.as_object()?;
        let block = object.get("block").filter(|value| !value.is_null())?;
        let bit = object.get("bit").filter(|value| !value.is_null())?;
        json_block_bit_key(block, bit)
    }
}

/// Registro da fila de push (formato de `sync.js`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QueueRecord {
    pub appid: String,
    pub apiname: String,
    pub unlocked_at: i64,
    pub achieved: bool,
    pub title: Option<String>,
    pub icon: Option<String>,
    pub percent: Option<f64>,
}

impl QueueRecord {
    pub fn new(appid: impl Into<String>, apiname: impl Into<String>) -> Self {
        Self {
            appid: appid.into(),
            apiname: apiname.into(),
            ..Self::default()
        }
    }

    pub fn key(&self) -> String {
        format!("{}|{}", self.appid, self.apiname)
    }

    pub fn to_value(&self) -> Value {
        let mut object = Map::new();
        object.insert("appid".into(), Value::String(self.appid.clone()));
        object.insert("apiname".into(), Value::String(self.apiname.clone()));
        object.insert("unlocked_at".into(), Value::from(self.unlocked_at));
        object.insert("achieved".into(), Value::Bool(self.achieved));
        if let Some(title) = &self.title {
            object.insert("title".into(), Value::String(title.clone()));
        }
        if let Some(icon) = &self.icon {
            object.insert("icon".into(), Value::String(icon.clone()));
        }
        if let Some(percent) = self.percent {
            if let Some(number) = serde_json::Number::from_f64(percent) {
                object.insert("percent".into(), Value::Number(number));
            }
        }
        Value::Object(object)
    }

    fn merge(&self, other: &Self) -> Self {
        let left_ts = positive(self.unlocked_at);
        let right_ts = positive(other.unlocked_at);
        let unlocked_at = match (left_ts, right_ts) {
            (Some(left), Some(right)) => left.min(right),
            (Some(value), None) | (None, Some(value)) => value,
            (None, None) => 0,
        };
        let achieved = self.achieved || other.achieved || unlocked_at > 0;
        // Metadados do registro com timestamp mais antigo são a base; o outro
        // preenche o que faltar.
        let base = match (left_ts, right_ts) {
            (Some(left), Some(right)) if right < left => other,
            (None, Some(_)) => other,
            _ => self,
        };
        let fallback = if std::ptr::eq(base, self) {
            other
        } else {
            self
        };
        let mut merged = base.clone();
        merged.unlocked_at = unlocked_at;
        merged.achieved = achieved;
        if merged.title.is_none() {
            merged.title = fallback.title.clone();
        }
        if merged.icon.is_none() {
            merged.icon = fallback.icon.clone();
        }
        if merged.percent.is_none() {
            merged.percent = fallback.percent;
        }
        merged
    }
}

impl From<&SyncRecord> for QueueRecord {
    fn from(record: &SyncRecord) -> Self {
        Self {
            appid: record.appid.clone(),
            apiname: record.apiname.clone(),
            unlocked_at: record.unlocked_at,
            achieved: record.achieved,
            title: Some(record.title.clone()),
            icon: record.icon.clone(),
            percent: record
                .percent
                .as_f64()
                .or_else(|| record.percent.as_i64().map(|value| value as f64)),
        }
    }
}

fn positive(value: i64) -> Option<i64> {
    (value > 0).then_some(value)
}

impl From<&AchievementRecord> for QueueRecord {
    fn from(record: &AchievementRecord) -> Self {
        Self {
            appid: record.appid.clone(),
            apiname: record.apiname.clone(),
            unlocked_at: record.unlocked_at.max(0),
            achieved: record.achieved,
            title: (!record.title.is_empty()).then(|| record.title.clone()),
            icon: (!record.icon.is_empty()).then(|| record.icon.clone()),
            percent: (record.percent != 0.0).then_some(record.percent),
        }
    }
}

/// Fila ordenada por `appid|apiname`, com dedupe/earliest-wins.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SyncQueue {
    records: BTreeMap<String, QueueRecord>,
}

impl SyncQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn get(&self, appid: &str, apiname: &str) -> Option<&QueueRecord> {
        self.records.get(&format!("{appid}|{apiname}"))
    }

    pub fn remove(&mut self, appid: &str, apiname: &str) -> Option<QueueRecord> {
        self.records.remove(&format!("{appid}|{apiname}"))
    }

    pub fn clear(&mut self) {
        self.records.clear();
    }

    /// Enfileira com dedupe por `(appid, apiname)` e merge earliest-wins.
    pub fn enqueue<I>(&mut self, incoming: I)
    where
        I: IntoIterator<Item = QueueRecord>,
    {
        for record in incoming {
            if record.appid.is_empty() || record.apiname.is_empty() {
                continue;
            }
            let key = record.key();
            match self.records.get(&key) {
                Some(existing) => {
                    let merged = existing.merge(&record);
                    self.records.insert(key, merged);
                }
                None => {
                    let mut record = record;
                    record.unlocked_at = record.unlocked_at.max(0);
                    record.achieved = record.achieved || record.unlocked_at > 0;
                    self.records.insert(key, record);
                }
            }
        }
    }

    /// Registros na ordem canônica `appid|apiname` (como o sort do JS).
    pub fn records(&self) -> Vec<&QueueRecord> {
        self.records.values().collect()
    }

    pub fn to_values(&self) -> Vec<Value> {
        self.records.values().map(QueueRecord::to_value).collect()
    }
}

/// Máquina de estados + fila de pendências.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AchievementManager {
    records: BTreeMap<String, AchievementRecord>,
    queue: SyncQueue,
}

impl AchievementManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, appid: &str, apiname: &str) -> Option<&AchievementRecord> {
        self.records.get(&format!("{appid}|{apiname}"))
    }

    pub fn records(&self) -> Vec<&AchievementRecord> {
        self.records.values().collect()
    }

    pub fn queue(&self) -> &SyncQueue {
        &self.queue
    }

    pub fn queue_mut(&mut self) -> &mut SyncQueue {
        &mut self.queue
    }

    /// Enfileira os registros do rebuild do loader com o mesmo dedupe.
    pub fn enqueue_sync_records(&mut self, records: &[SyncRecord]) {
        self.queue.enqueue(records.iter().map(QueueRecord::from));
    }

    /// Observação de detecção local (bin/Steam ou crack). Idempotente.
    pub fn observe(&mut self, mut record: AchievementRecord) -> Observation {
        if record.appid.is_empty() || record.apiname.is_empty() {
            return Observation::default();
        }
        let key = record.key();
        let mut observation = Observation::default();
        match self.records.get_mut(&key) {
            Some(existing) => {
                let was_achieved = existing.achieved;
                let was_unlock = existing.unlocked_at;
                let was_state = existing.state;
                existing.achieved |= record.achieved;
                existing.local_unlock |= record.local_unlock;
                existing.unlocked_at = earliest_positive(existing.unlocked_at, record.unlocked_at);
                if existing.title.is_empty() {
                    existing.title = record.title;
                }
                if existing.desc.is_empty() {
                    existing.desc = record.desc;
                }
                if existing.icon.is_empty() {
                    existing.icon = record.icon;
                }
                if existing.percent == 0.0 {
                    existing.percent = record.percent;
                }
                if existing.provider.is_none() {
                    existing.provider = record.provider;
                }
                if existing.state == AchievementState::Revoked
                    && record.achieved
                    && record.unlocked_at > 0
                {
                    existing.state = AchievementState::Pending;
                }
                let changed = was_achieved != existing.achieved
                    || was_unlock != existing.unlocked_at
                    || was_state != existing.state;
                observation.changed = changed;
                if existing.achieved && existing.state == AchievementState::Pending {
                    let queued = QueueRecord::from(&*existing);
                    self.queue.enqueue([queued]);
                }
                observation
            }
            None => {
                record.state = AchievementState::Pending;
                if record.unlocked_at < 0 {
                    record.unlocked_at = 0;
                }
                if record.achieved {
                    let queued = QueueRecord::from(&record);
                    self.queue.enqueue([queued]);
                }
                self.records.insert(key, record);
                Observation {
                    inserted: true,
                    changed: true,
                }
            }
        }
    }

    pub fn observe_unlock(
        &mut self,
        payload: &crate::cracked_watcher::UnlockPayload,
    ) -> Observation {
        self.observe(AchievementRecord::from_unlock_payload(payload))
    }

    /// O `achievements.json`/loader já contém a conquista; sai da fila.
    pub fn reconcile(
        &mut self,
        appid: &str,
        apiname: &str,
        achieved: bool,
        unlocked_at: i64,
    ) -> bool {
        let key = format!("{appid}|{apiname}");
        let queue_removed = self.queue.remove(appid, apiname).is_some();
        match self.records.get_mut(&key) {
            Some(record) => {
                let was = record.state;
                record.achieved |= achieved;
                record.unlocked_at = earliest_positive(record.unlocked_at, unlocked_at.max(0));
                if record.achieved && record.state != AchievementState::Revoked {
                    record.state = AchievementState::Reconciled;
                }
                was != record.state || queue_removed
            }
            None => {
                if !achieved {
                    return queue_removed;
                }
                let mut record = AchievementRecord::new(appid, apiname);
                record.achieved = true;
                record.unlocked_at = unlocked_at.max(0);
                record.state = AchievementState::Reconciled;
                self.records.insert(key, record);
                true
            }
        }
    }

    pub fn mark_reconciled(&mut self, appid: &str, apiname: &str) -> bool {
        self.reconcile(appid, apiname, true, 0)
    }

    /// Revogação do runtime local (UPC): desfaz e tira da fila.
    pub fn revoke(&mut self, appid: &str, apiname: &str) -> bool {
        let key = format!("{appid}|{apiname}");
        self.queue.remove(appid, apiname);
        match self.records.get_mut(&key) {
            Some(record) => {
                let changed = record.achieved
                    || record.unlocked_at != 0
                    || record.state != AchievementState::Revoked;
                record.achieved = false;
                record.unlocked_at = 0;
                record.state = AchievementState::Revoked;
                changed
            }
            None => {
                let mut record = AchievementRecord::new(appid, apiname);
                record.state = AchievementState::Revoked;
                self.records.insert(key, record);
                true
            }
        }
    }

    /// Conquistas desbloqueadas ainda pendentes, prontas para push.
    pub fn pending_records(&self) -> Vec<&AchievementRecord> {
        self.records
            .values()
            .filter(|record| record.achieved && record.state == AchievementState::Pending)
            .collect()
    }

    pub fn take_pending_queue(&mut self) -> Vec<QueueRecord> {
        let pending: Vec<QueueRecord> = self
            .records
            .values()
            .filter(|record| record.achieved && record.state == AchievementState::Pending)
            .map(QueueRecord::from)
            .collect();
        pending
    }

    pub fn has_steam_bin_for(&self, index: &ItemIndex, appid: &str) -> bool {
        index.has_steam_bin_for(appid)
    }

    pub fn should_start_steam_bin_watcher(&self, index: &ItemIndex, appid: &str) -> bool {
        index.has_steam_bin_for(appid)
    }
}

/// Equivalente a `hasSteamBinFor(appid)` do `manager.js`.
pub fn has_steam_bin_for(index: &ItemIndex, appid: &str) -> bool {
    index.has_steam_bin_for(appid)
}

fn earliest_positive(left: i64, right: i64) -> i64 {
    match (positive(left), positive(right)) {
        (Some(left), Some(right)) => left.min(right),
        (Some(value), None) | (None, Some(value)) => value,
        (None, None) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::load_item_index;
    use crate::loader::AchievementsStore;
    use serde_json::json;

    fn record(appid: &str, apiname: &str, achieved: bool, unlocked_at: i64) -> AchievementRecord {
        AchievementRecord {
            achieved,
            unlocked_at,
            title: format!("Título {apiname}"),
            ..AchievementRecord::new(appid, apiname)
        }
    }

    #[test]
    fn state_transitions_follow_the_machine() {
        assert!(AchievementState::Pending.can_transition_to(AchievementState::Reconciled));
        assert!(AchievementState::Pending.can_transition_to(AchievementState::Revoked));
        assert!(AchievementState::Reconciled.can_transition_to(AchievementState::Revoked));
        assert!(AchievementState::Revoked.can_transition_to(AchievementState::Pending));
        assert!(!AchievementState::Reconciled.can_transition_to(AchievementState::Pending));
    }

    #[test]
    fn queue_dedupes_earliest_unlock_and_sorts_by_key() {
        let mut queue = SyncQueue::new();
        queue.enqueue([
            QueueRecord {
                appid: "2".into(),
                apiname: "B".into(),
                unlocked_at: 200,
                achieved: true,
                ..QueueRecord::default()
            },
            QueueRecord {
                appid: "1".into(),
                apiname: "A".into(),
                unlocked_at: 100,
                achieved: true,
                ..QueueRecord::default()
            },
            QueueRecord {
                appid: "1".into(),
                apiname: "A".into(),
                unlocked_at: 50,
                achieved: false,
                title: Some("A".into()),
                ..QueueRecord::default()
            },
            QueueRecord::new("", "sem-appid"),
            QueueRecord::new("1", ""),
        ]);
        assert_eq!(queue.len(), 2);
        let records: Vec<_> = queue.records();
        assert_eq!(records[0].key(), "1|A");
        assert_eq!(records[0].unlocked_at, 50);
        assert!(records[0].achieved, "achieved é OR");
        assert_eq!(records[0].title.as_deref(), Some("A"));
        assert_eq!(records[1].key(), "2|B");
        assert!(queue.get("1", "A").is_some());
        assert!(queue.remove("1", "A").is_some());
        assert_eq!(queue.len(), 1);
        assert!(!queue.to_values().is_empty());
    }

    #[test]
    fn steam_bin_watcher_initial_snapshot_is_silent_then_reports_new_keys() {
        let store = AchievementsStore::from_json(
            r#"{"42":{"items":[
                {"apiname":"A","title":"Primeiros passos","desc":"d","icon":"i","block":1,"bit":0,"percent":12.5}
            ]}}"#,
        );
        let index = load_item_index(&store);
        let first = BTreeMap::from([("1|0".to_string(), 100)]);
        let mut watcher = SteamBinWatcher::new("42", &first);
        assert!(watcher.observe(&first, &index).unlocks.is_empty());

        let second = BTreeMap::from([("1|0".to_string(), 100), ("1|1".to_string(), 200)]);
        let decision = watcher.observe(&second, &index);
        assert_eq!(decision.unlocks.len(), 0, "1|1 não existe no índice");
        assert_eq!(decision.unindexed, ["1|1"]);

        let store = AchievementsStore::from_json(
            r#"{"42":{"items":[
                {"apiname":"A","title":"Primeiros passos","desc":"d","icon":"i","block":1,"bit":0,"percent":12.5},
                {"apiname":"B","title":"Segunda","block":1,"bit":1,"percent":0}
            ]}}"#,
        );
        let index = load_item_index(&store);
        let third = BTreeMap::from([("1|0".to_string(), 100), ("1|1".to_string(), 200)]);
        let mut watcher = SteamBinWatcher::new("42", &first);
        let decision = watcher.observe(&third, &index);
        assert_eq!(decision.unlocks.len(), 1);
        let unlock = &decision.unlocks[0];
        assert_eq!(unlock.key, "1|1");
        assert_eq!(unlock.title, "Segunda");
        assert_eq!(unlock.unlock, 200);
        assert!(watcher.observe(&third, &index).unlocks.is_empty());
    }

    #[test]
    fn steam_bin_watcher_ignores_zero_and_negative_new_values() {
        let store = AchievementsStore::from_json(
            r#"{"42":{"items":[{"apiname":"A","title":"A","block":1,"bit":0}]}}"#,
        );
        let index = load_item_index(&store);
        let mut watcher = SteamBinWatcher::new("42", &BTreeMap::new());
        let progress = BTreeMap::from([("1|0".to_string(), 0), ("1|1".to_string(), -5)]);
        let decision = watcher.observe(&progress, &index);
        assert!(decision.unlocks.is_empty());
        assert!(
            decision.unindexed.is_empty(),
            "valor não positivo não alerta"
        );
    }

    #[test]
    fn queue_keeps_blocked_records_with_zero_timestamp() {
        let mut queue = SyncQueue::new();
        queue.enqueue([QueueRecord {
            appid: "42".into(),
            apiname: "BLOQUEADA".into(),
            unlocked_at: 0,
            achieved: false,
            title: Some("Ainda não".into()),
            ..QueueRecord::default()
        }]);
        let record = queue.get("42", "BLOQUEADA").unwrap();
        assert!(!record.achieved);
        assert_eq!(record.unlocked_at, 0);
        assert_eq!(record.title.as_deref(), Some("Ainda não"));
    }

    #[test]
    fn queue_ignores_negative_as_unlock_and_keeps_zero_timestamps_valid() {
        let mut queue = SyncQueue::new();
        queue.enqueue([QueueRecord {
            appid: "1".into(),
            apiname: "A".into(),
            unlocked_at: -5,
            achieved: true,
            ..QueueRecord::default()
        }]);
        let record = queue.get("1", "A").unwrap();
        assert_eq!(record.unlocked_at, 0);
        assert!(record.achieved);
    }

    #[test]
    fn manager_observe_inserts_pending_and_queues_once() {
        let mut manager = AchievementManager::new();
        let first = manager.observe(record("42", "ACH01", true, 1_700_000_000));
        assert!(first.inserted && first.changed);
        assert_eq!(manager.records().len(), 1);
        assert_eq!(manager.pending_records().len(), 1);
        assert_eq!(manager.queue().len(), 1);
        let second = manager.observe(record("42", "ACH01", true, 1_700_000_000));
        assert!(!second.inserted);
        assert!(!second.changed);
        assert_eq!(manager.queue().len(), 1);
    }

    #[test]
    fn manager_achieved_is_monotonic_and_earliest_wins() {
        let mut manager = AchievementManager::new();
        manager.observe(record("42", "A", true, 200));
        manager.observe(record("42", "A", false, 100));
        let stored = manager.record("42", "A").unwrap();
        assert!(stored.achieved);
        assert_eq!(stored.unlocked_at, 100);
        assert_eq!(manager.queue().get("42", "A").unwrap().unlocked_at, 100);
    }

    #[test]
    fn reconcile_moves_to_reconciled_and_drops_from_queue() {
        let mut manager = AchievementManager::new();
        manager.observe(record("42", "A", true, 100));
        assert!(manager.reconcile("42", "A", true, 90));
        let stored = manager.record("42", "A").unwrap();
        assert_eq!(stored.state, AchievementState::Reconciled);
        assert_eq!(stored.unlocked_at, 90);
        assert!(manager.queue().is_empty());
        assert!(manager.pending_records().is_empty());
    }

    #[test]
    fn revoke_clears_state_and_queue_and_can_return_to_pending() {
        let mut manager = AchievementManager::new();
        manager.observe(record("42", "A", true, 100));
        assert!(manager.revoke("42", "A"));
        let stored = manager.record("42", "A").unwrap();
        assert!(!stored.achieved);
        assert_eq!(stored.unlocked_at, 0);
        assert_eq!(stored.state, AchievementState::Revoked);
        assert!(manager.queue().is_empty());
        assert!(!manager.revoke("42", "A"), "segunda revogação é no-op");
        manager.observe(record("42", "A", true, 300));
        let stored = manager.record("42", "A").unwrap();
        assert_eq!(stored.state, AchievementState::Pending);
        assert_eq!(manager.queue().len(), 1);
    }

    #[test]
    fn revoke_of_unknown_creates_revoked_and_reconcile_keeps_it() {
        let mut manager = AchievementManager::new();
        assert!(manager.revoke("42", "X"));
        assert!(manager.record("42", "X").unwrap().is_revoked());
        assert!(!manager.reconcile("42", "X", true, 10));
        assert!(manager.record("42", "X").unwrap().is_revoked());
    }

    #[test]
    fn observe_unlock_payload_marks_local_pending() {
        let payload = crate::cracked_watcher::UnlockPayload {
            appid: "42".into(),
            key: "1|0".into(),
            apiname: "ACH01".into(),
            provider: "goldberg".into(),
            title: "Primeiros passos".into(),
            desc: "d".into(),
            icon: "i".into(),
            percent: 12.5,
            unlock: 1_700_000_000,
        };
        let mut manager = AchievementManager::new();
        manager.observe_unlock(&payload);
        let record = manager.record("42", "ACH01").unwrap();
        assert!(record.local_unlock);
        assert_eq!(record.provider.as_deref(), Some("goldberg"));
        assert_eq!(record.state, AchievementState::Pending);
        assert_eq!(
            manager.queue().get("42", "ACH01").unwrap().percent,
            Some(12.5)
        );
        assert_eq!(manager.take_pending_queue().len(), 1);
    }

    #[test]
    fn manager_has_steam_bin_decision_uses_loader_index() {
        let store = AchievementsStore::from_json(
            r#"{"42":{"items":[{"apiname":"A","block":1,"bit":0}]},"7":{"items":[{"apiname":"B"}]}}"#,
        );
        let index = load_item_index(&store);
        let manager = AchievementManager::new();
        assert!(manager.has_steam_bin_for(&index, "42"));
        assert!(!manager.should_start_steam_bin_watcher(&index, "7"));
    }

    #[test]
    fn queue_record_from_sync_record_preserves_metadata() {
        let sync = SyncRecord {
            appid: "42".into(),
            apiname: "A".into(),
            unlocked_at: 10,
            achieved: true,
            title: "T".into(),
            icon: Some("i".into()),
            percent: json!(7),
        };
        let queue = QueueRecord::from(&sync);
        assert_eq!(queue.percent, Some(7.0));
        assert_eq!(queue.icon.as_deref(), Some("i"));
    }
}
