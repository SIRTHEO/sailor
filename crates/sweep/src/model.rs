use serde::{Deserialize, Serialize};

pub const UNKNOWN_GRACE_SECS: u64 = 24 * 60 * 60;

pub const MARKER_FAMILIES: &[&str] = &[
    "consegna-fatta",
    "consegna-fatta-ripartenze",
    "consegna-blocchi",
    "consegna-stop",
    "consegna-avvisata",
    "consegna-misura",
    "consegna-ripartenze",
    "consegna-volontaria",
    "consegna-stop-riferimento",
    "consegna-riferimento-lockout",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SweepConfig {
    pub state_dir: String,
    pub deleting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Liveness {
    Alive,
    Gone,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardMarker {
    pub name: String,
    pub session: String,
    pub liveness: Liveness,
    pub age_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyMarker {
    pub name: String,
    pub hex: String,
    pub path: String,
    pub path_known: bool,
    pub age_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scan {
    pub config: SweepConfig,
    pub standard: Vec<StandardMarker>,
    pub legacy: Vec<LegacyMarker>,
}

/// Un elenco parziale non è un elenco: chi manca perché la sua voce era
/// illeggibile è indistinguibile da chi non è vivo, e un marcatore senza padrone
/// viene condannato senza grazia. Il conteggio delle voci saltate viaggia quindi
/// col dato, e chi sta per cancellare deve guardarlo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveSessions {
    pub ids: Vec<String>,
    pub unreadable: u64,
    pub observed: u64,
}

impl LiveSessions {
    pub fn complete(&self) -> bool {
        self.observed > 0 && self.unreadable == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifiedMarker {
    pub name: String,
    pub verdict: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardClassification {
    pub config: SweepConfig,
    pub looked: Vec<String>,
    pub classified: Vec<ClassifiedMarker>,
    pub condemned: Vec<RemovalTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyClassification {
    pub config: SweepConfig,
    pub live_ids: Vec<String>,
    pub looked: Vec<String>,
    pub classified: Vec<ClassifiedMarker>,
    pub condemned: Vec<RemovalTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovalTarget {
    pub name: String,
    pub kind: String,
    pub session: String,
    pub liveness: Liveness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovalPlan {
    pub state_dir: String,
    pub deleting: bool,
    pub looked: Vec<String>,
    pub orphan: Vec<String>,
    pub targets: Vec<RemovalTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovalTrace {
    pub looked: Vec<String>,
    pub orphan: Vec<String>,
    pub removed: Vec<String>,
    pub spared: Vec<String>,
    pub vanished: Vec<String>,
    pub remove_failed: Vec<String>,
    pub recovered: bool,
}
