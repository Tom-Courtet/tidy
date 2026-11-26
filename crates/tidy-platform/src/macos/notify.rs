//! Notifications (lot H) : `display notification` d'AppleScript, par `osascript`.
//!
//! Les notifications de `UNUserNotificationCenter` demandent une app empaquetée et
//! signée ; `osascript` marche aussi pour l'exécutable nu (la notification porte alors
//! le nom de « Éditeur de script »).

use std::process::Command;

/// Son des notifications avec son.
const SOUND: &str = "Glass";

pub fn notify(title: &str, body: &str, sound: bool) -> bool {
    let mut script = format!(
        "display notification {} with title {}",
        applescript_string(body),
        applescript_string(title)
    );
    if sound {
        script.push_str(&format!(" sound name {}", applescript_string(SOUND)));
    }
    match Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .spawn()
    {
        Ok(mut child) => {
            // Attendre la fin sur un fil, pour ne pas laisser de processus zombie.
            let _ = std::thread::Builder::new()
                .name("tidy-notification".into())
                .spawn(move || child.wait());
            true
        }
        Err(e) => {
            log::warn!("notification impossible : {e}");
            false
        }
    }
}

/// Texte entre guillemets pour AppleScript (repris de Snappy).
fn applescript_string(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applescript_strings_are_escaped() {
        assert_eq!(applescript_string("Pause"), "\"Pause\"");
        assert_eq!(applescript_string(r#"x"y\z"#), r#""x\"y\\z""#);
    }
}
