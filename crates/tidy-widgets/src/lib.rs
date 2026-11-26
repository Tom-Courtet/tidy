//! Widgets du bureau : trait commun [`Widget`], registre des types ([`kinds`]) et les
//! widgets eux-mêmes (horloge, notes, pomodoro, outils rapides, météo, calendrier,
//! chat IA, Search Mini).
//!
//! Le processus du bureau (`app/src/desktop/widgets.rs`) héberge chaque instance dans
//! sa propre fenêtre posée sur le bureau, avec un cadre commun (déplacer, geler,
//! masquer, réglages, supprimer) : un widget ne dessine que son contenu. Ses réglages
//! sont un JSON libre, gardé dans [`tidy_core::model::WidgetInstance::settings`].
//!
//! Un widget ne touche ni au réseau ni au disque dans `ui` : les accès longs passent
//! par un fil à lui (voir les widgets de `tidy-integrations`).

pub mod search_mini;

use std::path::PathBuf;
use std::time::Duration;

/// Ce qu'un widget demande à son hôte.
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetAction {
    /// Ouvrir un fichier, un dossier ou une adresse web.
    Open(String),
    /// Notification native (fin d'un pomodoro, rappel).
    Notify { title: String, body: String },
    /// Demande à un module, envoyée à l'agent (`Request::Module`).
    Module {
        module: String,
        action: String,
        data: serde_json::Value,
    },
    /// Garder la fenêtre au premier plan (note épinglée) ou la rendre au bureau.
    PinOnTop(bool),
    /// Ouvrir la fenêtre principale sur une page (`"widgets"`…).
    OpenManager(String),
}

/// Contexte passé à chaque image.
pub struct WidgetCx {
    /// Dossier des données de Tidy (fichiers propres à un widget : notes…).
    pub data_dir: PathBuf,
    /// À mettre à `true` quand les réglages ont changé : l'hôte enregistre [`Widget::save`].
    pub changed: bool,
    pub actions: Vec<WidgetAction>,
}

impl WidgetCx {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            changed: false,
            actions: Vec::new(),
        }
    }
}

/// Un widget posé sur le bureau.
pub trait Widget: Send {
    /// Dessine le contenu (le cadre commun est dessiné par l'hôte).
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx);
    /// Réglages propres au widget (panneau de réglages de l'hôte). `true` si changés.
    fn settings_ui(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx) -> bool {
        let _ = (ui, cx);
        false
    }
    /// Réglages à enregistrer dans l'instance.
    fn save(&self) -> serde_json::Value;
    /// Prochain rafraîchissement voulu sans action de l'utilisateur (horloge : 1 s).
    fn repaint_after(&self) -> Option<Duration> {
        None
    }
}

/// Un type de widget du registre.
pub struct WidgetKind {
    /// Clé stable (`"clock"`, `"notes"`…), gardée dans `WidgetInstance::kind`.
    pub id: &'static str,
    /// Clé de traduction du nom (`tidy_ui::i18n::tr`).
    pub name_key: &'static str,
    /// Icône (`tidy_ui::icons`).
    pub icon: &'static str,
    /// Taille par défaut, en points.
    pub default_size: [f32; 2],
    /// Crée le widget à partir de ses réglages enregistrés (JSON vide : défauts).
    pub create: fn(&serde_json::Value) -> Box<dyn Widget>,
}

/// Tous les types de widgets, dans l'ordre du catalogue. Chaque lot ajoute ici les
/// siens (une ligne par widget).
pub fn kinds() -> Vec<WidgetKind> {
    vec![search_mini::kind()]
}

/// Type de widget d'après sa clé.
pub fn kind(id: &str) -> Option<WidgetKind> {
    kinds().into_iter().find(|k| k.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_widgets_round_trip() {
        let all = kinds();
        for (i, k) in all.iter().enumerate() {
            assert!(
                all[i + 1..].iter().all(|o| o.id != k.id),
                "{} en double",
                k.id
            );
            let w = (k.create)(&serde_json::Value::Null);
            let again = (k.create)(&w.save());
            assert_eq!(again.save(), w.save(), "{} : réglages perdus", k.id);
        }
    }
}
