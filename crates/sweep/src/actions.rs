use crate::model::*;
use flow::{Action, ActionError, EffectStatus, SharedState, StepRecord};
use guards::successor::{armed_fingerprint, recalculate_fingerprint_owner, FingerprintOwner};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

const ARMED_PREFIX: &str = "successore-armato-";

fn decode<T: DeserializeOwned>(input: &Value) -> Result<T, ActionError> {
    serde_json::from_value(input.clone())
        .map_err(|error| ActionError::new("invalid_input", error.to_string()))
}

fn encode<T: Serialize>(output: T) -> Result<Value, ActionError> {
    serde_json::to_value(output)
        .map_err(|error| ActionError::new("invalid_output", error.to_string()))
}

fn age(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    SystemTime::now()
        .duration_since(modified)
        .ok()
        .map(|value| value.as_secs())
}

fn split_marker(name: &str) -> Option<(&'static str, &str)> {
    MARKER_FAMILIES
        .iter()
        .filter_map(|family| {
            let rest = name.strip_prefix(family)?.strip_prefix('-')?;
            (!rest.is_empty()).then_some((*family, rest))
        })
        .max_by_key(|(family, _)| family.len())
}

enum ArmedContent {
    New(String),
    Legacy(String),
    Unreadable,
}

fn armed_content(raw: &str) -> ArmedContent {
    let lines: Vec<_> = raw.lines().filter(|line| !line.is_empty()).collect();
    match lines.as_slice() {
        [] => ArmedContent::Unreadable,
        [path] => ArmedContent::Legacy((*path).to_owned()),
        [first, second, ..] => {
            let timestamp = first.len() >= 5
                && first.as_bytes()[..4].iter().all(u8::is_ascii_digit)
                && first.as_bytes()[4] == b'-';
            if timestamp {
                ArmedContent::Legacy((*second).to_owned())
            } else {
                ArmedContent::New((*second).to_owned())
            }
        }
    }
}

fn state_record(state: &Path, session: &str) -> Option<Value> {
    let raw =
        fs::read_to_string(state.join("sessioni-vive").join(format!("{session}.json"))).ok()?;
    serde_json::from_str(&raw).ok()
}

fn process_name(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn process_exists(pid: u32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    let result = unsafe { kill(pid as i32, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(1)
}

fn boot_time() -> Option<u64> {
    let output = Command::new("sysctl")
        .args(["-n", "kern.boottime"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let after = text.split("sec = ").nth(1)?;
    after
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

fn liveness(state: &Path, session: &str) -> Liveness {
    let Some(record) = state_record(state, session) else {
        return Liveness::Unknown;
    };
    if record
        .get("updated_at")
        .and_then(Value::as_u64)
        .zip(boot_time())
        .is_some_and(|(updated, boot)| updated < boot)
    {
        return Liveness::Gone;
    }
    let Some(pid) = record
        .get("session_pid")
        .and_then(Value::as_u64)
        .map(|value| value as u32)
    else {
        return Liveness::Unknown;
    };
    match process_name(pid) {
        Some(name) if name.rsplit('/').next() == Some("claude") => Liveness::Alive,
        Some(_) => Liveness::Unknown,
        None if process_exists(pid) => Liveness::Unknown,
        None => Liveness::Gone,
    }
}

fn scan_dir(config: SweepConfig) -> Scan {
    let state = Path::new(&config.state_dir);
    let mut standard = Vec::new();
    let mut legacy = Vec::new();
    let mut known = HashMap::new();
    let Ok(entries) = fs::read_dir(state) else {
        return Scan {
            config,
            standard,
            legacy,
        };
    };
    for entry in entries.flatten().filter(|entry| entry.path().is_file()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(age_secs) = age(&entry.path()) else {
            continue;
        };
        if let Some(hex) = name.strip_prefix(ARMED_PREFIX).map(str::to_owned) {
            let raw = fs::read_to_string(entry.path()).unwrap_or_default();
            match armed_content(&raw) {
                ArmedContent::New(full) if !full.is_empty() => {
                    let session: String = full.chars().take(8).collect();
                    let live = *known
                        .entry(session.clone())
                        .or_insert_with(|| liveness(state, &session));
                    standard.push(StandardMarker {
                        name,
                        session,
                        liveness: live,
                        age_secs,
                    });
                }
                ArmedContent::Legacy(path) => legacy.push(LegacyMarker {
                    name,
                    hex,
                    path,
                    path_known: true,
                    age_secs,
                }),
                ArmedContent::Unreadable => legacy.push(LegacyMarker {
                    name,
                    hex,
                    path: String::new(),
                    path_known: false,
                    age_secs,
                }),
                ArmedContent::New(_) => {}
            }
            continue;
        }
        let Some((_, session)) = split_marker(&name) else {
            continue;
        };
        let session = session.to_owned();
        let live = *known
            .entry(session.clone())
            .or_insert_with(|| liveness(state, &session));
        standard.push(StandardMarker {
            name,
            session,
            liveness: live,
            age_secs,
        });
    }
    standard.sort_by(|left, right| left.name.cmp(&right.name));
    legacy.sort_by(|left, right| left.name.cmp(&right.name));
    Scan {
        config,
        standard,
        legacy,
    }
}

pub(crate) fn read_live(state: &Path) -> Option<LiveSessions> {
    let entries = fs::read_dir(state.join("sessioni-vive")).ok()?;
    let mut ids = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(short) = name.strip_suffix(".json") else {
            continue;
        };
        let Some(record) = state_record(state, short) else {
            continue;
        };
        if liveness(state, short) == Liveness::Alive {
            if let Some(id) = record
                .get("session_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            {
                ids.push(id.to_owned());
            }
        }
    }
    ids.sort();
    Some(LiveSessions { ids })
}

pub struct ScanAction;

impl Action for ScanAction {
    fn execute(&self, input: &Value, _shared: &mut SharedState) -> Result<Value, ActionError> {
        encode(scan_dir(decode(input)?))
    }
}

pub struct StandardAction;

impl Action for StandardAction {
    fn execute(&self, input: &Value, _shared: &mut SharedState) -> Result<Value, ActionError> {
        let scan: Scan = decode(input)?;
        let mut classified = Vec::new();
        let mut condemned = Vec::new();
        for marker in &scan.standard {
            let stale = match marker.liveness {
                Liveness::Alive => false,
                Liveness::Gone => true,
                Liveness::Unknown => marker.age_secs >= UNKNOWN_GRACE_SECS,
            };
            let verdict = if stale {
                "orphan"
            } else if marker.liveness == Liveness::Alive {
                "alive"
            } else {
                "fresh"
            };
            classified.push(ClassifiedMarker {
                name: marker.name.clone(),
                verdict: verdict.to_owned(),
            });
            if stale {
                condemned.push(RemovalTarget {
                    name: marker.name.clone(),
                    kind: "standard".to_owned(),
                    session: marker.session.clone(),
                    liveness: marker.liveness,
                });
            }
        }
        encode(StandardClassification {
            config: scan.config,
            looked: scan
                .standard
                .iter()
                .map(|marker| marker.name.clone())
                .collect(),
            classified,
            condemned,
        })
    }
}

#[derive(serde::Deserialize)]
struct LegacyInput {
    scan_markers: Scan,
    read_live_sessions: LiveSessions,
}

pub struct LegacyAction;

impl Action for LegacyAction {
    fn execute(&self, input: &Value, _shared: &mut SharedState) -> Result<Value, ActionError> {
        let input: LegacyInput = decode(input)?;
        let mut classified = Vec::new();
        let mut condemned = Vec::new();
        for marker in &input.scan_markers.legacy {
            let verdict = if !marker.path_known || armed_fingerprint(&marker.path, "") == marker.hex
            {
                if marker.age_secs >= UNKNOWN_GRACE_SECS {
                    "unmatched_stale"
                } else {
                    "unmatched_fresh"
                }
            } else {
                match recalculate_fingerprint_owner(
                    &marker.hex,
                    &marker.path,
                    Some(&input.read_live_sessions.ids),
                ) {
                    FingerprintOwner::Alive => "alive",
                    FingerprintOwner::Orphan => "orphan",
                    FingerprintOwner::Unknown => "unknown",
                }
            };
            classified.push(ClassifiedMarker {
                name: marker.name.clone(),
                verdict: verdict.to_owned(),
            });
            if matches!(verdict, "orphan" | "unmatched_stale") {
                condemned.push(RemovalTarget {
                    name: marker.name.clone(),
                    kind: "legacy".to_owned(),
                    session: String::new(),
                    liveness: Liveness::Unknown,
                });
            }
        }
        encode(LegacyClassification {
            config: input.scan_markers.config,
            live_ids: input.read_live_sessions.ids,
            looked: input
                .scan_markers
                .legacy
                .iter()
                .map(|marker| marker.name.clone())
                .collect(),
            classified,
            condemned,
        })
    }
}

#[derive(serde::Deserialize)]
struct PlanInput {
    classify_standard: StandardClassification,
    classify_legacy: LegacyClassification,
}

pub struct PlanAction;

impl Action for PlanAction {
    fn execute(&self, input: &Value, _shared: &mut SharedState) -> Result<Value, ActionError> {
        let input: PlanInput = decode(input)?;
        let mut looked = input.classify_standard.looked;
        looked.extend(input.classify_legacy.looked);
        let mut targets = input.classify_standard.condemned;
        targets.extend(input.classify_legacy.condemned);
        let orphan = targets.iter().map(|target| target.name.clone()).collect();
        encode(RemovalPlan {
            state_dir: input.classify_standard.config.state_dir,
            deleting: input.classify_standard.config.deleting,
            looked,
            orphan,
            targets,
        })
    }
}

pub struct RemoveAction;

impl RemoveAction {
    fn recovered(plan: &RemovalPlan) -> RemovalTrace {
        let state = Path::new(&plan.state_dir);
        let mut removed = Vec::new();
        let mut spared = Vec::new();
        for target in &plan.targets {
            if state.join(&target.name).exists() {
                spared.push(target.name.clone());
            } else {
                removed.push(target.name.clone());
            }
        }
        RemovalTrace {
            looked: plan.looked.clone(),
            orphan: plan.orphan.clone(),
            removed,
            spared,
            vanished: Vec::new(),
            remove_failed: Vec::new(),
            recovered: true,
        }
    }

    fn remove(plan: &RemovalPlan) -> RemovalTrace {
        let state = Path::new(&plan.state_dir);
        let live_now = read_live(state);
        let mut trace = RemovalTrace {
            looked: plan.looked.clone(),
            orphan: plan.orphan.clone(),
            removed: Vec::new(),
            spared: Vec::new(),
            vanished: Vec::new(),
            remove_failed: Vec::new(),
            recovered: false,
        };
        for (index, target) in plan.targets.iter().enumerate() {
            let path = state.join(&target.name);
            let still = if target.kind == "standard" {
                age(&path).is_some_and(|value| match target.liveness {
                    Liveness::Alive => false,
                    Liveness::Gone => true,
                    Liveness::Unknown => value >= UNKNOWN_GRACE_SECS,
                })
            } else {
                live_now
                    .as_ref()
                    .is_some_and(|live| legacy_still_condemned(&path, live))
            };
            if !still {
                if path.exists() {
                    trace.spared.push(target.name.clone())
                } else {
                    trace.vanished.push(target.name.clone())
                }
                continue;
            }
            match fs::remove_file(&path) {
                Ok(()) => trace.removed.push(target.name.clone()),
                Err(_) if path.exists() => trace.remove_failed.push(target.name.clone()),
                Err(_) => trace.vanished.push(target.name.clone()),
            }
            test_pause_after_first(index);
        }
        trace
    }
}

fn legacy_still_condemned(path: &Path, live: &LiveSessions) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(hex) = name.strip_prefix(ARMED_PREFIX) else {
        return false;
    };
    let raw = fs::read_to_string(path).unwrap_or_default();
    match armed_content(&raw) {
        ArmedContent::New(_) => false,
        ArmedContent::Unreadable => age(path).is_some_and(|value| value >= UNKNOWN_GRACE_SECS),
        ArmedContent::Legacy(marker_path) if armed_fingerprint(&marker_path, "") == hex => {
            age(path).is_some_and(|value| value >= UNKNOWN_GRACE_SECS)
        }
        ArmedContent::Legacy(marker_path) => matches!(
            recalculate_fingerprint_owner(hex, &marker_path, Some(&live.ids)),
            FingerprintOwner::Orphan
        ),
    }
}

impl Action for RemoveAction {
    fn execute(&self, input: &Value, _shared: &mut SharedState) -> Result<Value, ActionError> {
        encode(Self::remove(&decode(input)?))
    }

    fn inspect_effect(
        &self,
        record: &StepRecord,
        _shared: &SharedState,
    ) -> Result<EffectStatus, ActionError> {
        let plan: RemovalPlan = decode(&record.input)?;
        Ok(EffectStatus::Applied(encode(Self::recovered(&plan))?))
    }
}

fn test_pause_after_first(index: usize) {
    if index != 0 {
        return;
    }
    let Some(signal) = std::env::var_os("SWEEP_TEST_PAUSE_AFTER_FIRST") else {
        return;
    };
    let _ = fs::write(signal, b"first-removed");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

pub(crate) fn actions() -> flow::ActionRegistry {
    let mut actions = flow::ActionRegistry::default();
    actions.register("scan_markers", ScanAction);
    actions.register("classify_standard", StandardAction);
    actions.register("classify_legacy", LegacyAction);
    actions.register("plan_removals", PlanAction);
    actions.register("remove_markers", RemoveAction);
    actions
}

pub(crate) fn config_path(config: &SweepConfig) -> PathBuf {
    PathBuf::from(&config.state_dir)
}
