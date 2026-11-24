//! Raccourcis de Tidy : raccourcis globaux des réglages et raccourci de chaque boîte.
//! Un même raccourci ne peut servir qu'une fois (détection des conflits).

use tidy_core::{BoxId, DeskBox, DesktopState, Settings};
use tidy_ui::hotkey::{self, FieldChange};
use tidy_ui::{icons, t, widgets};

use super::Manager;

/// Emplacement d'un raccourci.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Search,
    ToggleDesktop,
    QuickAccess,
    MainWindow,
    Box(BoxId),
}

impl Slot {
    /// Nom de l'action, pour les libellés et les messages de conflit.
    pub fn label(self, desktop: &DesktopState) -> String {
        match self {
            Slot::Search => t!("options.hotkey_search").to_owned(),
            Slot::ToggleDesktop => t!("options.hotkey_toggle_desktop").to_owned(),
            Slot::QuickAccess => t!("options.hotkey_quick_access").to_owned(),
            Slot::MainWindow => t!("options.hotkey_main_window").to_owned(),
            Slot::Box(id) => {
                let name = desktop
                    .find_box(id)
                    .map(|b| super::boxes::display_name(&b.name))
                    .unwrap_or_default();
                t!("hotkey.box_owner", name = name)
            }
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Slot::Search => icons::SEARCH,
            Slot::ToggleDesktop => icons::EYE_OFF,
            Slot::QuickAccess => icons::BOXES,
            Slot::MainWindow => icons::APP,
            Slot::Box(_) => icons::BOX,
        }
    }

    fn hint(self) -> Option<&'static str> {
        match self {
            Slot::Search => Some(t!("options.hotkey_search_hint")),
            Slot::ToggleDesktop => Some(t!("options.hotkey_toggle_desktop_hint")),
            Slot::QuickAccess => Some(t!("options.hotkey_quick_access_hint")),
            Slot::MainWindow => None,
            Slot::Box(_) => Some(t!("boxes_page.hotkey_hint")),
        }
    }
}

/// Raccourci actuel d'un emplacement.
pub fn get(settings: &Settings, desktop: &DesktopState, slot: Slot) -> Option<String> {
    let h = &settings.hotkeys;
    match slot {
        Slot::Search => h.search.clone(),
        Slot::ToggleDesktop => h.toggle_desktop.clone(),
        Slot::QuickAccess => h.quick_access.clone(),
        Slot::MainWindow => h.main_window.clone(),
        Slot::Box(id) => desktop.find_box(id).and_then(|b| b.hotkey.clone()),
    }
}

/// Tous les raccourcis de Tidy, avec leur emplacement.
pub fn all(settings: &Settings, desktop: &DesktopState) -> Vec<(Slot, Option<String>)> {
    let h = &settings.hotkeys;
    let mut out = vec![
        (Slot::Search, h.search.clone()),
        (Slot::ToggleDesktop, h.toggle_desktop.clone()),
        (Slot::QuickAccess, h.quick_access.clone()),
        (Slot::MainWindow, h.main_window.clone()),
    ];
    out.extend(
        desktop
            .boxes
            .iter()
            .map(|b| (Slot::Box(b.id), b.hotkey.clone())),
    );
    out
}

/// Autre emplacement qui utilise déjà `text`, s'il y en a un.
pub fn conflict(settings: &Settings, desktop: &DesktopState, me: Slot, text: &str) -> Option<Slot> {
    let all = all(settings, desktop);
    hotkey::conflict(
        text,
        all.iter()
            .filter(|(slot, _)| *slot != me)
            .map(|(slot, v)| (*slot, v.as_deref())),
    )
}

/// Ligne de réglage d'un raccourci : libellé à gauche, champ de saisie à droite.
pub fn row(m: &mut Manager, ui: &mut egui::Ui, slot: Slot) {
    let label = slot.label(&m.desktop);
    let title = match slot {
        Slot::Box(_) => t!("boxes_page.hotkey").to_owned(),
        _ => label,
    };
    let change = widgets::setting_row(ui, slot.icon(), &title, slot.hint(), |ui| {
        field(m, ui, slot)
    });
    if let Some(change) = change {
        apply(m, slot, change);
    }
}

/// Champ de saisie seul, avec la détection des conflits.
pub fn field(m: &Manager, ui: &mut egui::Ui, slot: Slot) -> Option<FieldChange> {
    let value = get(&m.settings, &m.desktop, slot);
    let (settings, desktop) = (&m.settings, &m.desktop);
    hotkey::field(
        ui,
        slot_salt(slot),
        value.as_deref(),
        |text| match conflict(settings, desktop, slot, text) {
            Some(owner) => Err(t!("hotkey.conflict", owner = owner.label(desktop))),
            None => Ok(()),
        },
    )
}

fn slot_salt(slot: Slot) -> String {
    match slot {
        Slot::Box(id) => format!("boîte-{}", id.0),
        other => format!("{other:?}"),
    }
}

/// Enregistre un raccourci changé (réglages ou boîte).
pub fn apply(m: &mut Manager, slot: Slot, change: FieldChange) {
    let value = match change {
        FieldChange::Set(text) => Some(text),
        FieldChange::Cleared => None,
    };
    let h = &mut m.settings.hotkeys;
    match slot {
        Slot::Search => h.search = value,
        Slot::ToggleDesktop => h.toggle_desktop = value,
        Slot::QuickAccess => h.quick_access = value,
        Slot::MainWindow => h.main_window = value,
        Slot::Box(id) => {
            m.edit_box(id, |b: &mut DeskBox| b.hotkey = value);
            return;
        }
    }
    m.save();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hotkey_cannot_serve_twice() {
        let settings = Settings::default();
        let mut desktop = DesktopState::default();
        let mut b = DeskBox::new("Documents");
        b.hotkey = Some("control+alt+KeyD".into());
        let id = b.id;
        desktop.boxes.push(b);

        // Le raccourci par défaut du masquage, écrit autrement.
        assert_eq!(
            conflict(&settings, &desktop, Slot::Search, "alt+control+KeyH"),
            Some(Slot::ToggleDesktop)
        );
        // Le sien n'est pas un conflit.
        assert_eq!(
            conflict(&settings, &desktop, Slot::ToggleDesktop, "control+alt+KeyH"),
            None
        );
        // Le raccourci d'une boîte compte aussi.
        assert_eq!(
            conflict(&settings, &desktop, Slot::MainWindow, "control+alt+KeyD"),
            Some(Slot::Box(id))
        );
        assert_eq!(
            conflict(&settings, &desktop, Slot::Box(id), "control+alt+KeyD"),
            None
        );
        assert_eq!(
            conflict(&settings, &desktop, Slot::MainWindow, "control+alt+KeyM"),
            None
        );
        assert!(Slot::Box(id).label(&desktop).contains("Documents"));
    }

    #[test]
    fn every_slot_reads_its_value() {
        let settings = Settings::default();
        let desktop = DesktopState::default();
        for (slot, value) in all(&settings, &desktop) {
            assert_eq!(get(&settings, &desktop, slot), value);
        }
    }
}
