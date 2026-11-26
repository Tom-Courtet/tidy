//! Modèle du bureau : boîtes, onglets, éléments, positions, widgets.
//!
//! Les fichiers ne bougent jamais pour être rangés : une boîte référence des éléments
//! de `~/Desktop` par leur nom. Supprimer une boîte rend ses éléments au bureau, sans
//! toucher aux fichiers.
//!
//! Persisté dans `desktop.json` ([`DesktopState`]). Tout nouveau champ prend
//! `#[serde(default)]`, et un changement de format incompatible passe par une
//! migration (voir [`crate::store::Versioned`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::geometry::{Rect, Size};
use crate::grid::Cell;
use crate::rules::Rule;
use crate::screen::{Screen, ScreenId, ScreenSetKey};
use crate::store::Versioned;

/// Identifiant d'une boîte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BoxId(pub Uuid);

impl BoxId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for BoxId {
    fn default() -> Self {
        Self::new()
    }
}

/// Identifiant d'un onglet de boîte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TabId(pub Uuid);

impl TabId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TabId {
    fn default() -> Self {
        Self::new()
    }
}

/// Couleur sRGB avec opacité.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b, 255)
    }
}

/// Un élément du bureau, désigné par son nom dans `~/Desktop` (pas de chemin absolu :
/// le dossier du bureau peut changer de place, par exemple avec iCloud).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ItemRef(pub String);

/// Nature d'une boîte.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BoxKind {
    /// Boîte ordinaire : des éléments de `~/Desktop`.
    #[default]
    Normal,
    /// Portail : montre en direct le contenu d'un dossier (hors du bureau).
    Portal { path: std::path::PathBuf },
    /// La boîte privée (une seule), adossée au coffre chiffré.
    Private,
}

/// Critère de tri des éléments d'une boîte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    /// Ordre choisi à la main (glisser-déposer).
    #[default]
    Manual,
    Name,
    Kind,
    Modified,
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SortOrder {
    pub key: SortKey,
    pub descending: bool,
    /// Regrouper par date (aujourd'hui, hier, cette semaine…).
    pub group_by_date: bool,
}

/// Présentation des éléments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    #[default]
    Grid,
    List,
}

/// Quand afficher le titre d'une boîte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleVisibility {
    #[default]
    Always,
    OnHover,
    Never,
}

/// Apparence d'une boîte. Les boîtes sans apparence propre prennent celle par défaut
/// des réglages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub background: Rgba,
    pub title_background: Rgba,
    pub title_color: Rgba,
    pub label_color: Rgba,
    pub title_size: f32,
    /// Opacité du fond, de 0 (invisible) à 1.
    pub opacity: f32,
    /// Opacité quand la souris est ailleurs.
    pub opacity_away: f32,
    pub corner_radius: f32,
    pub border: bool,
    /// Fond « verre dépoli » (flou de ce qui est derrière).
    pub glass: bool,
    pub title_visibility: TitleVisibility,
    pub label_shadow: bool,
    /// Taille des icônes, en points.
    pub icon_size: f32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            background: Rgba(0x10, 0x10, 0x12, 255),
            title_background: Rgba(0x1b, 0x1b, 0x1f, 255),
            title_color: Rgba(0xf4, 0xf4, 0xf5, 255),
            label_color: Rgba(0xf4, 0xf4, 0xf5, 255),
            title_size: 13.0,
            opacity: 0.85,
            opacity_away: 0.85,
            corner_radius: 16.0,
            border: true,
            glass: false,
            title_visibility: TitleVisibility::Always,
            label_shadow: true,
            icon_size: 48.0,
        }
    }
}

/// Position d'un élément (boîte ou widget) mémorisée pour un écran d'une configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenPlacement {
    pub screen_set: ScreenSetKey,
    pub screen: ScreenId,
    /// Rectangle relatif à l'origine du cadre utile de l'écran.
    pub rect: Rect,
    /// Taille du cadre utile au moment de l'enregistrement, pour adapter la position si
    /// la résolution a changé.
    pub screen_size: Size,
}

/// Toutes les positions connues d'un élément, une par configuration d'écrans.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Placement {
    pub by_screen_set: Vec<ScreenPlacement>,
}

impl Placement {
    /// Enregistre la position `rect` (coordonnées globales) sur `screen`, pour la
    /// configuration `screens`.
    pub fn remember(&mut self, screens: &[Screen], screen: &Screen, rect: Rect) {
        let key = ScreenSetKey::of(screens);
        let entry = ScreenPlacement {
            screen_set: key.clone(),
            screen: screen.id.clone(),
            rect: rect.relative_to(&screen.visible_frame),
            screen_size: screen.visible_frame.size(),
        };
        self.by_screen_set.retain(|p| p.screen_set != key);
        self.by_screen_set.push(entry);
    }

    /// Position (coordonnées globales) pour la configuration `screens` : celle
    /// enregistrée pour cette configuration, sinon la plus récente dont l'écran est
    /// branché, sinon `None` (à placer par défaut). Le résultat reste dans l'écran.
    pub fn resolve(&self, screens: &[Screen]) -> Option<(ScreenId, Rect)> {
        let key = ScreenSetKey::of(screens);
        let exact = self.by_screen_set.iter().find(|p| p.screen_set == key);
        let fallback = || {
            self.by_screen_set
                .iter()
                .rev()
                .find(|p| screens.iter().any(|s| s.id == p.screen))
        };
        let placement = exact.or_else(fallback)?;
        let screen = screens.iter().find(|s| s.id == placement.screen)?;
        let rect = placement
            .rect
            .absolute_in(&screen.visible_frame)
            .clamped_to(&screen.visible_frame);
        Some((screen.id.clone(), rect))
    }
}

/// Un onglet de boîte. Une boîte a toujours au moins un onglet ; avec un seul, les
/// onglets ne s'affichent pas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub id: TabId,
    pub name: String,
    /// Éléments, dans l'ordre manuel.
    pub items: Vec<ItemRef>,
    /// Grille libre (ordre manuel, vue en grille) : case choisie pour un élément. Les
    /// éléments sans case prennent les premières cases libres ([`crate::grid`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cells: BTreeMap<ItemRef, Cell>,
}

impl Tab {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: TabId::new(),
            name: name.into(),
            items: Vec::new(),
            cells: BTreeMap::new(),
        }
    }

    /// Oublie les cases des éléments qui ne sont plus dans l'onglet.
    pub fn prune_cells(&mut self) {
        let items = &self.items;
        self.cells.retain(|item, _| items.contains(item));
    }
}

/// Une boîte posée sur le bureau.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeskBox {
    pub id: BoxId,
    pub name: String,
    #[serde(default)]
    pub kind: BoxKind,
    pub tabs: Vec<Tab>,
    #[serde(default)]
    pub active_tab: usize,
    #[serde(default)]
    pub placement: Placement,
    /// `None` : apparence par défaut des réglages.
    #[serde(default)]
    pub appearance: Option<Appearance>,
    #[serde(default)]
    pub sort: SortOrder,
    #[serde(default)]
    pub view: ViewMode,
    /// Enroulée : seule la barre de titre est visible.
    #[serde(default)]
    pub rolled_up: bool,
    /// Enroulement automatique quand la souris s'éloigne (« Smarter Roll »).
    #[serde(default)]
    pub auto_roll: bool,
    /// Reste visible quand on masque le bureau par double-clic.
    #[serde(default)]
    pub exclude_from_quick_hide: bool,
    /// N'apparaît pas avec l'accès rapide aux boîtes.
    #[serde(default)]
    pub exclude_from_quick_access: bool,
    /// Raccourci global qui fait apparaître la boîte (format de `global-hotkey`).
    #[serde(default)]
    pub hotkey: Option<String>,
}

impl DeskBox {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: BoxId::new(),
            name: name.into(),
            kind: BoxKind::Normal,
            tabs: vec![Tab::new(String::new())],
            active_tab: 0,
            placement: Placement::default(),
            appearance: None,
            sort: SortOrder::default(),
            view: ViewMode::default(),
            rolled_up: false,
            auto_roll: false,
            exclude_from_quick_hide: false,
            exclude_from_quick_access: false,
            hotkey: None,
        }
    }

    /// Tous les éléments de la boîte, tous onglets confondus.
    pub fn items(&self) -> impl Iterator<Item = &ItemRef> {
        self.tabs.iter().flat_map(|t| t.items.iter())
    }

    pub fn contains(&self, item: &ItemRef) -> bool {
        self.items().any(|i| i == item)
    }
}

/// Instance d'un widget posée sur le bureau. Le widget lui-même (code, rendu) vit dans
/// `tidy-widgets` ; le cœur ne garde que ses réglages sérialisés et sa position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetInstance {
    pub id: Uuid,
    /// Type du widget (`"clock"`, `"notes"`…), clé du registre de `tidy-widgets`.
    pub kind: String,
    #[serde(default)]
    pub settings: serde_json::Value,
    #[serde(default)]
    pub placement: Placement,
    #[serde(default)]
    pub frozen: bool,
    #[serde(default)]
    pub exclude_from_quick_hide: bool,
}

/// Position d'une icône hors boîte, par configuration d'écrans.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct LooseIcon {
    pub placement: Placement,
}

/// État du bureau (`desktop.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesktopState {
    pub schema_version: u32,
    #[serde(default)]
    pub boxes: Vec<DeskBox>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// Icônes hors boîte, par nom.
    #[serde(default)]
    pub loose: BTreeMap<ItemRef, LooseIcon>,
    /// Éléments arrivés sans règle correspondante (bandeau « non affectées »).
    #[serde(default)]
    pub unassigned: Vec<ItemRef>,
    #[serde(default)]
    pub widgets: Vec<WidgetInstance>,
}

impl Default for DesktopState {
    fn default() -> Self {
        Self {
            schema_version: Self::CURRENT_VERSION,
            boxes: Vec::new(),
            rules: Vec::new(),
            loose: BTreeMap::new(),
            unassigned: Vec::new(),
            widgets: Vec::new(),
        }
    }
}

impl Versioned for DesktopState {
    const CURRENT_VERSION: u32 = 1;
    const FILE_NAME: &'static str = "desktop.json";
}

/// Modification de l'état du bureau demandée par un autre processus.
///
/// **Un seul écrivain** : le processus du bureau possède `desktop.json`. L'agent (règles,
/// nouveaux fichiers) et la fenêtre principale (pages Boîtes, Rangement) lui envoient des
/// commandes, relayées par l'agent (`Request::Desktop` → `Event::Desktop`). Si le bureau
/// ne tourne pas, l'agent applique lui-même la commande au fichier, avec la même
/// fonction [`DesktopState::apply`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DesktopCommand {
    /// Ranger des éléments : `Some(boîte)` les place dans le premier onglet de la boîte
    /// (en les retirant de toute autre), `None` les rend au bureau.
    Assign {
        items: Vec<(ItemRef, Option<BoxId>)>,
    },
    /// Éléments arrivés sans règle correspondante (bandeau « non affectées »).
    SetUnassigned { items: Vec<ItemRef> },
    /// Ajoute une boîte (ou la remplace si l'identifiant existe déjà).
    UpsertBox { b: DeskBox },
    /// Supprime une boîte ; ses éléments reviennent au bureau.
    RemoveBox { id: BoxId },
    /// Remplace les règles.
    SetRules { rules: Vec<Rule> },
    /// Remplace tout l'état (annuler un rangement, restaurer un instantané).
    Replace { state: Box<DesktopState> },
}

impl DesktopState {
    /// Applique une commande. Ne touche jamais aux fichiers.
    pub fn apply(&mut self, command: DesktopCommand) {
        match command {
            DesktopCommand::Assign { items } => {
                for (item, target) in items {
                    for b in &mut self.boxes {
                        for tab in &mut b.tabs {
                            tab.items.retain(|i| i != &item);
                        }
                    }
                    self.unassigned.retain(|i| i != &item);
                    match target.and_then(|id| self.find_box_mut(id)) {
                        Some(b) => {
                            if b.tabs.is_empty() {
                                b.tabs.push(Tab::new(String::new()));
                            }
                            b.tabs[0].items.push(item.clone());
                            self.loose.remove(&item);
                        }
                        None => {
                            self.loose.entry(item).or_default();
                        }
                    }
                }
            }
            DesktopCommand::SetUnassigned { items } => self.unassigned = items,
            DesktopCommand::UpsertBox { b } => match self.find_box_mut(b.id) {
                Some(existing) => *existing = b,
                None => self.boxes.push(b),
            },
            DesktopCommand::RemoveBox { id } => {
                self.remove_box(id);
            }
            DesktopCommand::SetRules { rules } => self.rules = rules,
            DesktopCommand::Replace { state } => *self = *state,
        }
    }

    pub fn find_box(&self, id: BoxId) -> Option<&DeskBox> {
        self.boxes.iter().find(|b| b.id == id)
    }

    pub fn find_box_mut(&mut self, id: BoxId) -> Option<&mut DeskBox> {
        self.boxes.iter_mut().find(|b| b.id == id)
    }

    /// Boîte qui contient `item`, s'il y en a une.
    pub fn box_of(&self, item: &ItemRef) -> Option<BoxId> {
        self.boxes.iter().find(|b| b.contains(item)).map(|b| b.id)
    }

    /// Supprime une boîte. Ses éléments redeviennent des icônes du bureau ; aucun
    /// fichier n'est touché. Renvoie les éléments rendus.
    pub fn remove_box(&mut self, id: BoxId) -> Vec<ItemRef> {
        let Some(index) = self.boxes.iter().position(|b| b.id == id) else {
            return Vec::new();
        };
        let removed = self.boxes.remove(index);
        self.rules.retain(|r| r.target != id);
        let items: Vec<ItemRef> = removed.items().cloned().collect();
        for item in &items {
            self.loose.entry(item.clone()).or_default();
        }
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(id: &str, x: f32, w: f32, h: f32) -> Screen {
        let frame = Rect::new(x, 0.0, w, h);
        Screen {
            id: ScreenId(id.into()),
            name: id.into(),
            frame,
            visible_frame: frame,
            scale: 2.0,
            primary: x == 0.0,
        }
    }

    #[test]
    fn placement_is_remembered_per_screen_set() {
        let laptop = screen("intégré", 0.0, 1512.0, 982.0);
        let external = screen("dell", 1512.0, 1920.0, 1080.0);
        let mut p = Placement::default();
        // Seul : en haut à gauche de l'écran du portable.
        p.remember(
            std::slice::from_ref(&laptop),
            &laptop,
            Rect::new(10.0, 10.0, 300.0, 200.0),
        );
        // Avec l'écran externe : sur l'externe.
        let both = [laptop.clone(), external.clone()];
        p.remember(&both, &external, Rect::new(1600.0, 50.0, 300.0, 200.0));

        let (id, r) = p.resolve(&both).unwrap();
        assert_eq!(id, external.id);
        assert_eq!(r, Rect::new(1600.0, 50.0, 300.0, 200.0));

        let (id, r) = p.resolve(std::slice::from_ref(&laptop)).unwrap();
        assert_eq!(id, laptop.id);
        assert_eq!(r, Rect::new(10.0, 10.0, 300.0, 200.0));
    }

    #[test]
    fn unknown_screen_set_falls_back_to_a_connected_screen() {
        let laptop = screen("intégré", 0.0, 1512.0, 982.0);
        let tv = screen("tv", 1512.0, 3840.0, 2160.0);
        let mut p = Placement::default();
        p.remember(
            std::slice::from_ref(&laptop),
            &laptop,
            Rect::new(1400.0, 10.0, 300.0, 200.0),
        );
        let (id, r) = p.resolve(&[laptop.clone(), tv]).unwrap();
        assert_eq!(id, laptop.id);
        // Ramenée dans l'écran.
        assert_eq!(r.right(), 1512.0);
    }

    #[test]
    fn assigning_moves_items_between_boxes_and_back() {
        let mut state = DesktopState::default();
        let a = DeskBox::new("A");
        let b = DeskBox::new("B");
        let (ida, idb) = (a.id, b.id);
        state.boxes.extend([a, b]);
        let item = ItemRef("photo.jpg".into());
        state.apply(DesktopCommand::Assign {
            items: vec![(item.clone(), Some(ida))],
        });
        assert_eq!(state.box_of(&item), Some(ida));
        state.apply(DesktopCommand::Assign {
            items: vec![(item.clone(), Some(idb))],
        });
        assert_eq!(state.box_of(&item), Some(idb));
        assert!(!state.find_box(ida).unwrap().contains(&item));
        state.apply(DesktopCommand::Assign {
            items: vec![(item.clone(), None)],
        });
        assert_eq!(state.box_of(&item), None);
        assert!(state.loose.contains_key(&item));
    }

    #[test]
    fn commands_round_trip_in_json() {
        let cmd = DesktopCommand::RemoveBox { id: BoxId::new() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert_eq!(serde_json::from_str::<DesktopCommand>(&json).unwrap(), cmd);
    }

    #[test]
    fn removing_a_box_returns_items_to_the_desktop() {
        let mut state = DesktopState::default();
        let mut b = DeskBox::new("Documents");
        b.tabs[0].items.push(ItemRef("Rapport.pdf".into()));
        let id = b.id;
        state.boxes.push(b);
        state
            .rules
            .push(Rule::category(id, crate::category::Category::Document));
        let items = state.remove_box(id);
        assert_eq!(items, vec![ItemRef("Rapport.pdf".into())]);
        assert!(state.boxes.is_empty());
        assert!(state.rules.is_empty());
        assert!(state.loose.contains_key(&ItemRef("Rapport.pdf".into())));
    }
}
