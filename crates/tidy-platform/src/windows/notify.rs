//! Notifications (lot H) : toasts de Windows 10 et 11, par WinRT
//! (`ToastNotificationManager`).
//!
//! Une appli non empaquetée doit présenter un identifiant d'application (AUMID) connu
//! du système : Tidy enregistre le sien sous
//! `HKCU\Software\Classes\AppUserModelId\Tidy.Desktop` (nom affiché « Tidy »), comme le
//! fait le Windows Community Toolkit. Si le toast est refusé, il repart sous
//! l'identifiant de PowerShell, toujours présent.

use std::sync::OnceLock;

use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::core::HSTRING;

/// Identifiant de Tidy pour ses notifications.
const AUMID: &str = "Tidy.Desktop";
/// Identifiant de PowerShell (repli).
const POWERSHELL_AUMID: &str =
    r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";

/// Enregistre l'identifiant de Tidy (une fois par processus).
fn registered() -> bool {
    static DONE: OnceLock<bool> = OnceLock::new();
    *DONE.get_or_init(|| {
        super::registry::set_string(
            &format!(r"Software\Classes\AppUserModelId\{AUMID}"),
            "DisplayName",
            "Tidy",
        )
        .inspect_err(|e| log::warn!("identifiant des notifications : {e:#}"))
        .is_ok()
    })
}

pub fn notify(title: &str, body: &str, sound: bool) -> bool {
    let xml = toast_xml(title, body, sound);
    std::thread::Builder::new()
        .name("tidy-notification".into())
        .spawn(move || {
            let ids: &[&str] = if registered() {
                &[AUMID, POWERSHELL_AUMID]
            } else {
                &[POWERSHELL_AUMID]
            };
            for id in ids {
                match show(&xml, id) {
                    Ok(()) => return,
                    Err(e) => log::warn!("notification ({id}) refusée : {e}"),
                }
            }
        })
        .is_ok()
}

fn show(xml: &str, aumid: &str) -> windows::core::Result<()> {
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(aumid))?.Show(&toast)
}

/// Contenu du toast : titre, texte, son par défaut ou silence.
fn toast_xml(title: &str, body: &str, sound: bool) -> String {
    let audio = if sound {
        r#"<audio src="ms-winsoundevent:Notification.Default"/>"#
    } else {
        r#"<audio silent="true"/>"#
    };
    format!(
        r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual>{audio}</toast>"#,
        escape(title),
        escape(body)
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_text_is_escaped() {
        let xml = toast_xml("Pause <courte>", "A & B", false);
        assert!(xml.contains("Pause &lt;courte&gt;"));
        assert!(xml.contains("A &amp; B"));
        assert!(xml.contains(r#"silent="true""#));
        assert!(toast_xml("t", "b", true).contains("Notification.Default"));
    }
}
