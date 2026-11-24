//! Rapport de diagnostic à copier (bouton du Diagnostic) : version, système, écrans,
//! état de l'agent, réglages sans secrets, résumé du bureau (sans noms de fichiers) et
//! dernières lignes des journaux.

use std::fmt::Write as _;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::Manager;

/// Lignes de journal gardées par rôle (agent, bureau, fenêtre).
const LOG_LINES: usize = 40;
/// On ne lit que la fin des journaux.
const LOG_TAIL_BYTES: u64 = 32 * 1024;

pub fn build(m: &Manager) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Tidy {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "Système : {} {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::consts::FAMILY
    );
    match &m.status {
        Some(s) => {
            let _ = writeln!(
                out,
                "Agent : actif (v{}) · bureau en marche : {} · icônes du bureau masquées : {} · bureau masqué : {}",
                s.version, s.desktop_running, s.finder_hidden, s.desktop_hidden
            );
        }
        None => {
            let _ = writeln!(out, "Agent : ne répond pas");
        }
    }

    let _ = writeln!(out, "\nÉcrans :");
    for s in tidy_platform::current().screens() {
        let _ = writeln!(
            out,
            "- {} : {}×{} pt à ({}, {}), ×{}{} [{}]",
            s.name,
            s.frame.width,
            s.frame.height,
            s.frame.x,
            s.frame.y,
            s.scale,
            if s.primary { ", principal" } else { "" },
            s.id.0
        );
    }

    let d = &m.desktop;
    let _ = writeln!(
        out,
        "\nBureau : {} boîtes ({} portails), {} règles, {} éléments rangés, {} non affectés, {} widgets",
        d.boxes.len(),
        d.boxes
            .iter()
            .filter(|b| matches!(b.kind, tidy_core::model::BoxKind::Portal { .. }))
            .count(),
        d.rules.len(),
        d.boxes.iter().map(|b| b.items().count()).sum::<usize>(),
        d.unassigned.len(),
        d.widgets.len()
    );

    let _ = writeln!(out, "\nRéglages :");
    let mut settings = serde_json::to_value(&m.settings).unwrap_or(Value::Null);
    redact(&mut settings);
    let _ = writeln!(
        out,
        "{}",
        serde_json::to_string_pretty(&settings).unwrap_or_default()
    );

    let _ = writeln!(out, "\nJournaux :");
    for path in latest_logs(&m.store.logs_dir()) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let _ = writeln!(out, "--- {name} ---");
        match tail(&path, LOG_LINES) {
            Ok(lines) => {
                let _ = writeln!(out, "{lines}");
            }
            Err(e) => {
                let _ = writeln!(out, "(illisible : {e})");
            }
        }
    }
    out
}

/// Remplace la valeur des clés qui pourraient contenir un secret.
pub fn redact(value: &mut Value) {
    const SECRET: [&str; 6] = [
        "password",
        "secret",
        "token",
        "api_key",
        "apikey",
        "passphrase",
    ];
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                let lower = key.to_ascii_lowercase();
                if SECRET.iter().any(|s| lower.contains(s)) {
                    *v = Value::String("***".into());
                } else {
                    redact(v);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact),
        _ => {}
    }
}

/// Dernier fichier de journal de chaque rôle (`agent.log.AAAA-MM-JJ`…), par nom.
pub fn latest_logs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut latest: std::collections::BTreeMap<String, PathBuf> = Default::default();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some((role, _)) = name.split_once(".log") else {
            continue;
        };
        let newer = latest
            .get(role)
            .is_none_or(|p| p.file_name().and_then(|n| n.to_str()) < Some(name));
        if newer {
            latest.insert(role.to_owned(), path.clone());
        }
    }
    latest.into_values().collect()
}

/// Dernières `lines` lignes d'un fichier (seule la fin est lue).
pub fn tail(path: &Path, lines: usize) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(LOG_TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let all: Vec<&str> = text.lines().collect();
    // Début coupé au milieu d'une ligne : on l'écarte.
    let skip = usize::from(start > 0);
    let from = all.len().saturating_sub(lines).max(skip.min(all.len()));
    Ok(all[from..].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_redacted_but_hotkeys_stay() {
        let mut v = serde_json::json!({
            "hotkeys": { "search": "alt+Space" },
            "vault": { "password": "hunter2", "recovery": [{ "api_key": "abc" }] },
            "weather_token": "xyz"
        });
        redact(&mut v);
        assert_eq!(v["hotkeys"]["search"], "alt+Space");
        assert_eq!(v["vault"]["password"], "***");
        assert_eq!(v["vault"]["recovery"][0]["api_key"], "***");
        assert_eq!(v["weather_token"], "***");
    }

    #[test]
    fn the_latest_log_of_each_role_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "agent.log.2026-11-23",
            "agent.log.2026-11-24",
            "manager.log.2026-11-22",
            "notes.txt",
        ] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        let names: Vec<String> = latest_logs(dir.path())
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["agent.log.2026-11-24", "manager.log.2026-11-22"]);
    }

    #[test]
    fn tail_keeps_the_last_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent.log");
        let text: String = (1..=100).map(|i| format!("ligne {i}\n")).collect();
        std::fs::write(&path, text).unwrap();
        let out = tail(&path, 3).unwrap();
        assert_eq!(out, "ligne 98\nligne 99\nligne 100");
        assert_eq!(tail(&path, 1000).unwrap().lines().count(), 100);
    }
}
