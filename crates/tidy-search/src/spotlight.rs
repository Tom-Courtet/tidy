//! « Plus de résultats » : recherche de Spotlight (`mdfind -name`) sur tout le disque,
//! hors de l'index. macOS seulement ; ailleurs, aucun résultat.

use std::path::PathBuf;

/// Spotlight est-il disponible ici ?
pub fn available() -> bool {
    cfg!(target_os = "macos")
}

/// Chemins dont le nom contient `text`, au plus `limit`. Bloquant : à appeler hors du
/// fil de l'interface.
pub fn more_results(text: &str, limit: usize) -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};

        let text = text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        let child = Command::new("/usr/bin/mdfind")
            .arg("-name")
            .arg(text)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                log::warn!("mdfind : {e}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                out.push(PathBuf::from(line));
                if out.len() >= limit {
                    break;
                }
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        out
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (text, limit);
        Vec::new()
    }
}
