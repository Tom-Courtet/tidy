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
//!
//! Lot H : horloge ([`clock`]), notes ([`notes`]), pomodoro ([`pomodoro`]) et outils
//! rapides ([`quick_tools`]).
//!
//! Clé réservée : l'hôte garde l'état « épinglé au premier plan » d'une instance dans
//! ses réglages, sous [`PINNED_KEY`] (ajoutée à l'objet rendu par [`Widget::save`]).
//! Un widget ne doit pas s'en servir pour autre chose.

pub mod clock;
pub mod notes;
pub mod pomodoro;
pub mod quick_tools;

use std::path::PathBuf;
use std::time::Duration;

/// Clé des réglages d'une instance où l'hôte garde l'épinglage au premier plan.
pub const PINNED_KEY: &str = "pinned_on_top";

// Widgets en ligne (lot I).
mod ai_chat;
mod calendar;
mod online;
mod weather;

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
    // --- Lot H ---
    /// Notification native, avec ou sans son (iPomodoro : son réglable).
    Alert {
        title: String,
        body: String,
        sound: bool,
    },
    /// Lancer un programme avec ses arguments (outils rapides : verrouiller la
    /// session…). Pour un fichier, une appli ou une adresse, préférer [`Self::Open`].
    Launch { program: String, args: Vec<String> },
    /// Masquer (`true`) ou rendre (`false`) le bureau : mode concentration.
    HideDesktop(bool),
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
    // --- Lot H ---
    /// Taille voulue du contenu, en points (sans le cadre de l'hôte) : l'hôte
    /// redimensionne la fenêtre quand elle change (horloge : taille, fuseaux). `None` :
    /// taille libre, choisie par l'utilisateur.
    fn preferred_size(&self) -> Option<[f32; 2]> {
        None
    }
    /// Rester visible même quand le bureau est masqué (pomodoro en mode concentration).
    fn keep_visible(&self) -> bool {
        false
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
    vec![
        clock::kind(),
        notes::kind(),
        pomodoro::kind(),
        quick_tools::kind(),
        WidgetKind {
            id: "weather",
            name_key: "widgets2.weather_name",
            icon: tidy_ui::icons::WEATHER,
            default_size: [300.0, 380.0],
            create: weather::create,
        },
        WidgetKind {
            id: "calendar",
            name_key: "widgets2.calendar_name",
            icon: tidy_ui::icons::CALENDAR,
            default_size: [320.0, 440.0],
            create: calendar::create,
        },
        WidgetKind {
            id: "ai_chat",
            name_key: "widgets2.ai_name",
            icon: tidy_ui::icons::AI,
            default_size: [360.0, 480.0],
            create: ai_chat::create,
        },
    ]
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

    /// Le nom de chaque widget est traduit (la clé ne passe pas par `t!`).
    #[test]
    fn every_kind_has_a_translated_name() {
        for k in kinds() {
            assert_ne!(tidy_ui::i18n::tr(k.name_key), k.name_key, "{}", k.id);
            assert!(k.default_size[0] >= 100.0 && k.default_size[1] >= 60.0);
        }
    }

    /// Chaque widget se dessine, avec ses réglages, sans écran.
    #[test]
    fn every_kind_draws() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        tidy_ui::theme::apply(&ctx);
        for k in kinds() {
            let mut w = (k.create)(&serde_json::Value::Null);
            let mut cx = WidgetCx::new(dir.path().to_path_buf());
            for _ in 0..2 {
                let out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    w.ui(ui, &mut cx);
                    w.settings_ui(ui, &mut cx);
                });
                out.drop_without_applying_deltas();
            }
        }
    }
}
