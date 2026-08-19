//! Le competenze partono da sole, o solo quando qualcuno le chiama per nome?
//!
//! Conta le chiamate allo strumento `Skill` nei trascritti e le divide in tre.
//! Fra «ordinata» e «scelta» c'è l'invocazione **indotta**, dove a nominare la
//! skill è stato un gancio, una notifica o un promemoria di sistema: contarla
//! come scelta direbbe che il modello decide mentre sta obbedendo, ed è la
//! differenza che il §7 del mandato chiede di misurare.
//!
//! Il conteggio dei turni sui trascritti è dichiarato inquinato; questo no:
//! conta chiamate a uno strumento con un nome esatto, e il denominatore è
//! esplicito. La misura del 19/08/2026 sta in
//! `docs/2026-08-19-le-skill-partono-da-sole.md`.
//!
//! Uso: `cargo run --release -p claude-hooks --example skill_source -- [giorni]`

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Le forme con cui un messaggio arriva da una macchina invece che da Theo.
const AUTOMATIC: &[&str] = &[
    "<task-notification>",
    "system-reminder",
    "hookspecificoutput",
    "this session is being continued",
    "[cross-session delivery notice]",
    "caveat: the messages below",
    "<command-name>",
];

#[derive(Default, Clone, Copy)]
struct Tally {
    ordered: usize,
    prompted: usize,
    chosen: usize,
}

fn text_of(message: &serde_json::Value) -> Option<String> {
    match message.get("content")? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(blocks) => {
            let joined: Vec<&str> = blocks
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect();
            (!joined.is_empty()).then(|| joined.join(" "))
        }
        _ => None,
    }
}

fn read_transcript(path: &Path, per_skill: &mut BTreeMap<String, Tally>, with_skill: &mut usize) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let mut last_human = String::new();
    let mut seen_here = false;
    for line in text.lines() {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(message) = event.get("message") else { continue };
        match event.get("type").and_then(|t| t.as_str()) {
            Some("user") => {
                if let Some(t) = text_of(message) {
                    // Il taglio è lo stesso della versione che ha prodotto la
                    // misura: un messaggio lunghissimo non deve far comparire
                    // il nome di una skill nominata mille righe prima.
                    last_human = t.chars().take(4000).collect::<String>().to_lowercase();
                }
            }
            Some("assistant") => {
                let Some(blocks) = message.get("content").and_then(|c| c.as_array()) else {
                    continue;
                };
                for block in blocks {
                    if block.get("type").and_then(|t| t.as_str()) != Some("tool_use")
                        || block.get("name").and_then(|n| n.as_str()) != Some("Skill")
                    {
                        continue;
                    }
                    let name = block
                        .get("input")
                        .and_then(|i| i.get("skill"))
                        .and_then(|s| s.as_str())
                        .unwrap_or("?")
                        .to_string();
                    seen_here = true;
                    let short = name.rsplit(':').next().unwrap_or(&name).to_lowercase();
                    let automatic = AUTOMATIC.iter().any(|m| last_human.contains(m));
                    let named = last_human.contains(&short) || last_human.contains(&name.to_lowercase());
                    let tally = per_skill.entry(name).or_default();
                    match (named, automatic) {
                        (true, false) => tally.ordered += 1,
                        (_, true) => tally.prompted += 1,
                        (false, false) => tally.chosen += 1,
                    }
                }
            }
            _ => {}
        }
    }
    if seen_here {
        *with_skill += 1;
    }
}

fn walk(dir: &Path, cutoff: SystemTime, per_skill: &mut BTreeMap<String, Tally>, files: &mut usize, with_skill: &mut usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            walk(&path, cutoff, per_skill, files, with_skill);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let recent = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|m| m >= cutoff)
            .unwrap_or(false);
        if !recent {
            continue;
        }
        *files += 1;
        read_transcript(&path, per_skill, with_skill);
    }
}

fn main() {
    let days: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(7);
    let cutoff = SystemTime::now() - Duration::from_secs(days * 86_400);
    let root = format!("{}/.claude/projects", std::env::var("HOME").unwrap_or_default());

    let mut per_skill: BTreeMap<String, Tally> = BTreeMap::new();
    let (mut files, mut with_skill) = (0usize, 0usize);
    walk(Path::new(&root), cutoff, &mut per_skill, &mut files, &mut with_skill);

    let total = |f: fn(&Tally) -> usize| per_skill.values().map(|t| f(t)).sum::<usize>();
    let (ordered, prompted, chosen) = (total(|t| t.ordered), total(|t| t.prompted), total(|t| t.chosen));
    let all = ordered + prompted + chosen;

    println!("{files} transcripts touched in the last {days} days, {with_skill} of them used a skill");
    println!("{all} Skill calls — {ordered} ordered, {prompted} prompted by a hook or a notice, {chosen} chosen by the model");
    if all > 0 {
        println!("actually chosen: {:.1}%", 100.0 * chosen as f64 / all as f64);
    }
    println!("\nname                                          ordered / prompted / chosen");
    let mut rows: Vec<(&String, &Tally)> = per_skill.iter().collect();
    rows.sort_by_key(|(_, t)| std::cmp::Reverse(t.ordered + t.prompted + t.chosen));
    for (name, t) in rows {
        println!("  {name:44} {:4} / {:4} / {:4}", t.ordered, t.prompted, t.chosen);
    }
}
