//! Lot M : ce qui manquait aux boîtes par rapport à l'original (menu ☰ d'iTop) :
//! recherche dans la barre de titre, gel, exclusions du masquage et de l'accès
//! rapides, « Actualiser les icônes », sous-menu « Règles », « Réglages… », fond
//! « verre dépoli ».
//!
//! Toutes les options de boîte sont des champs de [`DeskBox`], enregistrés dans
//! `desktop.json` par le bureau (seul écrivain) : elles survivent au redémarrage. La
//! boîte cible d'une catégorie (« Ranger ici ») est un réglage (`config.json`).

use tidy_core::Settings;
use tidy_core::category::Category;
use tidy_core::model::{Appearance, BoxId};
use tidy_ui::t;

use super::action::Action;
use super::{BoxWindow, Desktop, open_manager, targets};
use crate::ipc::{self, Request};

/// Fond « verre dépoli » d'une fenêtre de boîte.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GlassState {
    /// Dernière demande faite au système : (verre voulu, rayon des coins).
    applied: Option<(bool, u8)>,
    /// Le système a accepté : le fond est dessiné plus transparent.
    pub active: bool,
}

/// Demande au système le fond « verre dépoli » quand l'apparence le veut (ou le
/// retire), une fois la fenêtre posée et visible. Si l'OS refuse, la boîte garde son
/// fond ordinaire.
pub fn sync_glass(win: &mut BoxWindow, look: &Appearance, visible: bool) {
    if !win.placed || !visible {
        return;
    }
    let want = (
        look.glass,
        look.corner_radius.clamp(0.0, 40.0).round() as u8,
    );
    if win.glass.applied == Some(want) {
        return;
    }
    // Rien à retirer si le verre n'a jamais été posé.
    let active = if want.0 || win.glass.applied.is_some_and(|(on, _)| on) {
        tidy_platform::current().set_window_glass(&win.title, want.0, f32::from(want.1))
    } else {
        false
    };
    if want.0 && !active {
        log::info!(
            "verre dépoli refusé pour « {} » : fond ordinaire",
            win.title
        );
    }
    win.glass = GlassState {
        applied: Some(want),
        active: want.0 && active,
    };
}

/// Argument de `--manager` pour la page Boîtes, cette boîte dépliée.
pub fn boxes_page_arg(b: BoxId) -> String {
    format!("boxes:{}", b.0)
}

impl Desktop {
    /// Applique une action du lot M. Rend `false` si l'action n'en est pas une.
    pub(super) fn apply_extra(&mut self, action: &Action) -> bool {
        match *action {
            Action::ToggleSearchInTitle { b } => {
                self.edit_box(b, |x| x.search_in_title = !x.search_in_title);
                // Une recherche en cours disparaît avec l'option.
                if let Some(win) = self.windows.get_mut(&b) {
                    win.ui.search = None;
                }
            }
            Action::ToggleFreeze { b } => self.edit_box(b, |x| x.frozen = !x.frozen),
            Action::ToggleExcludeQuickHide { b } => {
                self.edit_box(b, |x| {
                    x.exclude_from_quick_hide = !x.exclude_from_quick_hide
                });
            }
            Action::ToggleExcludeQuickAccess { b } => {
                self.edit_box(b, |x| {
                    x.exclude_from_quick_access = !x.exclude_from_quick_access;
                });
                // Accès rapide en cours : la boîte suit tout de suite.
                if self.quick_access {
                    let excluded = self
                        .state
                        .find_box(b)
                        .is_some_and(|x| x.exclude_from_quick_access);
                    let keep = self.raised_until.contains_key(&b);
                    self.set_raised(b, !excluded || keep);
                }
            }
            Action::RefreshIcons { b } => self.refresh_icons(b),
            Action::ToggleRuleTarget { b, category } => self.toggle_rule_target(b, category),
            Action::OpenRules => open_manager("organization"),
            Action::BoxSettings { b } => open_manager(&boxes_page_arg(b)),
            _ => return false,
        }
        true
    }

    /// « Actualiser les icônes » : oublie les icônes des éléments affichés, puis relit
    /// le dossier (bureau ou dossier du portail). Les icônes sont redemandées au
    /// système au passage suivant.
    fn refresh_icons(&mut self, b: BoxId) {
        let Some(dir) = self.dir_of(b) else { return };
        if let Some(win) = self.windows.get(&b) {
            for item in win.sections.iter().flat_map(|s| s.items.iter()) {
                self.icons.forget(&dir.join(&item.name));
            }
        }
        self.rescan_dir_of(b);
        self.generation += 1;
        self.notice(b, t!("box_extra.icons_refreshed"));
    }

    /// « Ranger ici » : change la boîte cible d'une catégorie dans `config.json`, puis
    /// prévient l'agent, qui republie les réglages à tous (fenêtre principale
    /// comprise).
    fn toggle_rule_target(&mut self, b: BoxId, category: Option<Category>) {
        // Relire le fichier : la fenêtre principale a pu le changer entre-temps.
        let mut settings: Settings = match self.store.load_checked() {
            Ok(settings) => settings,
            Err(e) => {
                log::error!("réglages non enregistrés : {e:#}");
                self.notice(b, t!("box_extra.rules_save_failed"));
                return;
            }
        };
        targets::toggle(&mut settings.organization, b, category);
        if let Err(e) = self.store.save(&settings) {
            log::error!("réglages non enregistrés : {e:#}");
            self.notice(b, t!("box_extra.rules_save_failed"));
            return;
        }
        self.settings = settings;
        let store = self.store.clone();
        std::thread::spawn(move || {
            if let Err(e) = ipc::request(&store, &Request::ReloadSettings) {
                log::warn!("réglages : agent injoignable ({e:#})");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidy_core::model::DeskBox;

    #[test]
    fn options_survive_a_restart() {
        // Les options du lot M sont des champs de la boîte, relus tels quels.
        let mut b = DeskBox::new("A");
        b.search_in_title = true;
        b.frozen = true;
        b.exclude_from_quick_hide = true;
        b.exclude_from_quick_access = true;
        b.appearance = Some(Appearance {
            glass: true,
            ..Appearance::default()
        });
        let json = serde_json::to_string(&b).unwrap();
        let back: DeskBox = serde_json::from_str(&json).unwrap();
        assert_eq!(back, b);
        // Ancien fichier sans ces champs : valeurs par défaut.
        let mut old: serde_json::Value = serde_json::to_value(DeskBox::new("B")).unwrap();
        for key in [
            "search_in_title",
            "frozen",
            "exclude_from_quick_hide",
            "exclude_from_quick_access",
        ] {
            old.as_object_mut().unwrap().remove(key);
        }
        let read: DeskBox = serde_json::from_value(old).unwrap();
        assert!(!read.search_in_title && !read.frozen);
    }

    #[test]
    fn settings_page_names_the_box() {
        let b = BoxId::new();
        let arg = boxes_page_arg(b);
        let id = arg.strip_prefix("boxes:").unwrap();
        let back: BoxId = serde_json::from_value(serde_json::Value::String(id.into())).unwrap();
        assert_eq!(back, b);
    }
}
