//! Dispositions : instantanés de l'état du bureau et dispositions nommées.
//!
//! - **Instantané** : copie de [`DesktopState`] (boîtes, placements, onglets, cases de
//!   la grille libre, widgets, règles), avec sa date, son nom et son type
//!   ([`SnapshotKind`]). Les automatiques sont pris à chaque changement de disposition
//!   (par l'agent, regroupés, sans doublon) et élagués au-delà de
//!   [`crate::settings::SnapshotSettings::keep`] ; le premier et les manuels ne
//!   s'effacent qu'à la main.
//! - **Disposition nommée** : un « espace de travail » enregistré sous un nom, avec un
//!   raccourci global facultatif pour l'appliquer.
//!
//! Stockage dans le dossier de données : `snapshots/index.json` (la liste, versionnée)
//! et un fichier `snapshots/<id>.json` par instantané ou disposition, écrits de façon
//! atomique. **Un seul écrivain : l'agent** ; la fenêtre principale lit seulement
//! ([`peek_index`]). Restaurer ne réécrit pas `desktop.json` ici : l'agent envoie une
//! `DesktopCommand::Replace` au processus du bureau.
//!
//! Le reste est pur et testé : empreinte d'un état ([`fingerprint`]), élagage
//! ([`prune`]), état restauré ([`restored_state`]), positions selon le réglage du
//! changement de résolution ([`resolve_placement`]) et miniature ([`thumbnail`]).

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
pub use uuid::Uuid;

use crate::geometry::Rect;
use crate::model::{DesktopState, ItemRef, Placement, ScreenPlacement};
use crate::screen::{Screen, ScreenId, ScreenSetKey};
use crate::settings::ResolutionPolicy;
use crate::store::{Store, Versioned, migrate, write_atomic};

/// Dossier des instantanés, dans le dossier de données.
pub const SNAPSHOT_DIR: &str = "snapshots";

/// Type d'un instantané.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    /// Pris tout seul après un changement ; élagué au-delà du nombre gardé.
    #[default]
    Auto,
    /// Pris à la main ; ne s'efface qu'à la main.
    Manual,
    /// Le tout premier instantané ; ne s'efface qu'à la main.
    First,
}

impl SnapshotKind {
    /// Protégé de l'élagage automatique.
    pub fn protected(self) -> bool {
        !matches!(self, SnapshotKind::Auto)
    }
}

/// Pourquoi un instantané a été pris (sert au nom par défaut).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotReason {
    /// La disposition a changé.
    #[default]
    Changed,
    /// Copie de sécurité prise juste avant une restauration.
    BeforeRestore,
    /// Demandé par l'utilisateur.
    User,
}

/// Un instantané dans l'index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub id: Uuid,
    /// Nom donné par l'utilisateur ; vide : nom par défaut selon le type et la raison.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: SnapshotKind,
    #[serde(default)]
    pub reason: SnapshotReason,
    pub created: Timestamp,
    /// Empreinte de la disposition ([`fingerprint`]).
    #[serde(default)]
    pub hash: u64,
    /// Nombre de boîtes (affichage).
    #[serde(default)]
    pub boxes: usize,
}

/// Une disposition nommée.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedLayout {
    pub id: Uuid,
    pub name: String,
    /// Raccourci global qui applique la disposition (format de `global-hotkey`).
    #[serde(default)]
    pub hotkey: Option<String>,
    pub created: Timestamp,
    /// Dernière fois que son contenu a été enregistré.
    pub updated: Timestamp,
    #[serde(default)]
    pub hash: u64,
    #[serde(default)]
    pub boxes: usize,
}

/// Index des instantanés et des dispositions nommées (`snapshots/index.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutIndex {
    pub schema_version: u32,
    /// Du plus ancien au plus récent.
    #[serde(default)]
    pub snapshots: Vec<SnapshotMeta>,
    #[serde(default)]
    pub layouts: Vec<NamedLayout>,
    /// Le premier instantané a déjà été pris (même s'il a été effacé depuis).
    #[serde(default)]
    pub first_taken: bool,
}

impl Default for LayoutIndex {
    fn default() -> Self {
        Self {
            schema_version: Self::CURRENT_VERSION,
            snapshots: Vec::new(),
            layouts: Vec::new(),
            first_taken: false,
        }
    }
}

impl Versioned for LayoutIndex {
    const CURRENT_VERSION: u32 = 1;
    const FILE_NAME: &'static str = "snapshots/index.json";
}

impl LayoutIndex {
    pub fn snapshot(&self, id: Uuid) -> Option<&SnapshotMeta> {
        self.snapshots.iter().find(|s| s.id == id)
    }

    pub fn layout(&self, id: Uuid) -> Option<&NamedLayout> {
        self.layouts.iter().find(|l| l.id == id)
    }

    /// Instantané le plus récent dont la disposition diffère de `current` (« revenir à
    /// la disposition précédente »).
    pub fn previous(&self, current: u64) -> Option<&SnapshotMeta> {
        self.snapshots.iter().rev().find(|s| s.hash != current)
    }

    /// Raccourcis des dispositions nommées.
    pub fn hotkeys(&self) -> Vec<(Uuid, String)> {
        self.layouts
            .iter()
            .filter_map(|l| Some((l.id, l.hotkey.clone()?)))
            .collect()
    }
}

/// Lit l'index sans rien écrire (fenêtre principale) : absent ou illisible, un index
/// vide.
pub fn peek_index(store: &Store) -> LayoutIndex {
    fs::read_to_string(store.path_of::<LayoutIndex>())
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| migrate::<LayoutIndex>(value).ok())
        .unwrap_or_default()
}

/// Fichier d'un instantané ou d'une disposition.
pub fn state_path(store: &Store, id: Uuid) -> PathBuf {
    store
        .root()
        .join(SNAPSHOT_DIR)
        .join(format!("{}.json", id.simple()))
}

/// Lit l'état d'un instantané ou d'une disposition (migrations comprises).
pub fn load_state(store: &Store, id: Uuid) -> Result<DesktopState> {
    let path = state_path(store, id);
    let text =
        fs::read_to_string(&path).with_context(|| format!("lecture de {}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("{} illisible", path.display()))?;
    migrate::<DesktopState>(value)
}

fn save_state(store: &Store, id: Uuid, state: &DesktopState) -> Result<()> {
    let text = serde_json::to_string_pretty(state)?;
    write_atomic(&state_path(store, id), text.as_bytes())
}

fn remove_state(store: &Store, id: Uuid) {
    let path = state_path(store, id);
    if let Err(e) = fs::remove_file(&path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("{} non retiré : {e}", path.display());
    }
}

/// Empreinte de la **disposition** : boîtes (placements, onglets, cases, apparence…),
/// règles et widgets. Les icônes hors boîte et les éléments non affectés n'y entrent
/// pas : un fichier qui arrive sur le bureau ne change pas la disposition. FNV-1a sur
/// le JSON, stable d'une version de Rust à l'autre.
pub fn fingerprint(state: &DesktopState) -> u64 {
    let parts = (&state.boxes, &state.rules, &state.widgets);
    let bytes = serde_json::to_vec(&parts).unwrap_or_default();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Élague les instantanés automatiques au-delà de `keep` (les plus anciens d'abord).
/// Le premier et les manuels ne comptent pas et restent. Rend les identifiants retirés.
pub fn prune(snapshots: &mut Vec<SnapshotMeta>, keep: usize) -> Vec<Uuid> {
    let autos = snapshots.iter().filter(|s| !s.kind.protected()).count();
    let mut excess = autos.saturating_sub(keep);
    let mut removed = Vec::new();
    snapshots.retain(|s| {
        if excess > 0 && !s.kind.protected() {
            excess -= 1;
            removed.push(s.id);
            false
        } else {
            true
        }
    });
    removed
}

/// État à appliquer pour restaurer `snapshot` alors que le bureau est dans `current` :
/// boîtes, règles et widgets de l'instantané ; les éléments qui étaient rangés et ne le
/// sont plus reviennent au bureau (icônes hors boîte), ceux que l'instantané range
/// quittent le bureau et les non affectés. Aucun fichier n'est touché.
pub fn restored_state(current: &DesktopState, snapshot: &DesktopState) -> DesktopState {
    let mut state = snapshot.clone();
    state.schema_version = DesktopState::CURRENT_VERSION;
    let boxed: BTreeSet<&ItemRef> = state.boxes.iter().flat_map(|b| b.items()).collect();
    let mut loose = current.loose.clone();
    for item in current.boxes.iter().flat_map(|b| b.items()) {
        if !boxed.contains(item) {
            loose.entry(item.clone()).or_default();
        }
    }
    loose.retain(|item, _| !boxed.contains(item));
    let unassigned = current
        .unassigned
        .iter()
        .filter(|item| !boxed.contains(item))
        .cloned()
        .collect();
    state.loose = loose;
    state.unassigned = unassigned;
    state
}

/// Position (coordonnées globales) d'un élément pour la configuration `screens`, selon
/// le réglage du changement de résolution :
/// - mémoriser par configuration ou ne rien faire : la position enregistrée pour cette
///   configuration d'écrans (sinon la plus récente dont l'écran est branché), gardée
///   dans l'écran ([`Placement::resolve`]) ;
/// - redimensionner : la même, mise à l'échelle de la taille actuelle de l'écran
///   (proportions gardées par rapport à la taille au moment de l'enregistrement).
pub fn resolve_placement(
    placement: &Placement,
    screens: &[Screen],
    policy: ResolutionPolicy,
) -> Option<(ScreenId, Rect)> {
    match policy {
        ResolutionPolicy::RememberPerResolution | ResolutionPolicy::Nothing => {
            placement.resolve(screens)
        }
        ResolutionPolicy::Rescale => {
            let entry = chosen(placement, screens)?;
            let screen = screens.iter().find(|s| s.id == entry.screen)?;
            Some((screen.id.clone(), rescaled(entry, screen)))
        }
    }
}

/// Même choix d'entrée que [`Placement::resolve`].
fn chosen<'a>(placement: &'a Placement, screens: &[Screen]) -> Option<&'a ScreenPlacement> {
    let key = ScreenSetKey::of(screens);
    placement
        .by_screen_set
        .iter()
        .find(|p| p.screen_set == key)
        .or_else(|| {
            placement
                .by_screen_set
                .iter()
                .rev()
                .find(|p| screens.iter().any(|s| s.id == p.screen))
        })
}

/// Rectangle d'une entrée mis à l'échelle du cadre utile actuel de `screen`.
fn rescaled(entry: &ScreenPlacement, screen: &Screen) -> Rect {
    let frame = screen.visible_frame;
    let (old_w, old_h) = (entry.screen_size.width, entry.screen_size.height);
    let r = entry.rect;
    let scaled = if old_w > 1.0 && old_h > 1.0 {
        let (sx, sy) = (frame.width / old_w, frame.height / old_h);
        Rect::new(r.x * sx, r.y * sy, r.width * sx, r.height * sy)
    } else {
        r
    };
    scaled.absolute_in(&frame).clamped_to(&frame)
}

/// Oublie les positions des autres configurations d'écrans que `screens` (« réinitialiser
/// les autres résolutions »). Rend `true` si quelque chose a changé.
pub fn forget_other_screen_sets(placement: &mut Placement, screens: &[Screen]) -> bool {
    let key = ScreenSetKey::of(screens);
    let before = placement.by_screen_set.len();
    if !placement.by_screen_set.iter().any(|p| p.screen_set == key) {
        // Rien pour cette configuration : on garde la position qui sert aujourd'hui.
        let Some(keep) = chosen(placement, screens).cloned() else {
            return false;
        };
        placement.by_screen_set = vec![keep];
        return before != 1;
    }
    placement.by_screen_set.retain(|p| p.screen_set == key);
    placement.by_screen_set.len() != before
}

/// Cadre d'une boîte dans une miniature : rectangle de 0 à 1 dans l'écran principal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThumbBox {
    pub rect: Rect,
    pub rolled: bool,
}

/// Taille d'une boîte sans position enregistrée (celle du processus du bureau).
const DEFAULT_BOX: [f32; 2] = [320.0, 220.0];

/// Place par défaut de la `i`-ième boîte sans position, comme le processus du bureau :
/// en colonnes depuis le coin en haut à droite du cadre utile `v`, sans chevauchement.
fn default_rect(v: Rect, i: usize) -> Rect {
    let [w, h] = DEFAULT_BOX;
    let (margin, gap) = (40.0, 10.0);
    let rows = (((v.height - 2.0 * margin + gap) / (h + gap)).floor() as usize).max(1);
    let (col, row) = ((i / rows) as f32, (i % rows) as f32);
    Rect::new(
        v.right() - margin - w - col * (w + gap),
        v.y + margin + row * (h + gap),
        w,
        h,
    )
    .clamped_to(&v)
}

/// Miniature d'un état : cadres des boîtes posées sur l'écran principal, en fractions
/// de son cadre (0 à 1), et proportions de l'écran (largeur / hauteur). Une boîte jamais
/// déplacée (sans position enregistrée) est dessinée à sa place par défaut.
pub fn thumbnail(
    state: &DesktopState,
    screens: &[Screen],
    policy: ResolutionPolicy,
) -> (f32, Vec<ThumbBox>) {
    let Some(primary) = screens.iter().find(|s| s.primary).or(screens.first()) else {
        return (16.0 / 9.0, Vec::new());
    };
    let f = primary.frame;
    let ratio = if f.height > 0.0 {
        f.width / f.height
    } else {
        16.0 / 9.0
    };
    let boxes = state
        .boxes
        .iter()
        .enumerate()
        .filter_map(|(i, b)| {
            let (screen, r) = resolve_placement(&b.placement, screens, policy)
                .unwrap_or_else(|| (primary.id.clone(), default_rect(primary.visible_frame, i)));
            if screen != primary.id || f.width <= 0.0 || f.height <= 0.0 {
                return None;
            }
            Some(ThumbBox {
                rect: Rect::new(
                    (r.x - f.x) / f.width,
                    (r.y - f.y) / f.height,
                    r.width / f.width,
                    r.height / f.height,
                ),
                rolled: b.rolled_up,
            })
        })
        .collect();
    (ratio, boxes)
}

/// Résultat d'une prise d'instantané.
#[derive(Debug, Clone, PartialEq)]
pub enum Taken {
    New(SnapshotMeta),
    /// Identique au dernier instantané : rien de pris.
    Duplicate,
}

/// Instantanés et dispositions d'un dossier de données. **À n'utiliser que dans
/// l'agent** (seul écrivain) ; les appels doivent être faits un à la fois.
#[derive(Debug, Clone)]
pub struct Layouts {
    store: Store,
}

impl Layouts {
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Index (illisible : mis de côté en `.bad`, index vide).
    pub fn index(&self) -> LayoutIndex {
        self.store.load()
    }

    fn save_index(&self, index: &LayoutIndex) -> Result<()> {
        self.store.save(index)
    }

    /// Prend un instantané de `state`. Un automatique identique au dernier instantané
    /// n'est pas pris ; le tout premier devient [`SnapshotKind::First`]. Les
    /// automatiques au-delà de `keep` sont élagués.
    pub fn take(
        &self,
        state: &DesktopState,
        kind: SnapshotKind,
        reason: SnapshotReason,
        name: &str,
        keep: usize,
        now: Timestamp,
    ) -> Result<Taken> {
        let mut index = self.index();
        let hash = fingerprint(state);
        if kind == SnapshotKind::Auto && index.snapshots.last().is_some_and(|s| s.hash == hash) {
            return Ok(Taken::Duplicate);
        }
        let kind = if index.first_taken {
            kind
        } else {
            index.first_taken = true;
            match kind {
                SnapshotKind::Auto => SnapshotKind::First,
                other => other,
            }
        };
        let meta = SnapshotMeta {
            id: Uuid::new_v4(),
            name: name.trim().to_owned(),
            kind,
            reason,
            created: now,
            hash,
            boxes: state.boxes.len(),
        };
        save_state(&self.store, meta.id, state)?;
        index.snapshots.push(meta.clone());
        let removed = prune(&mut index.snapshots, keep);
        self.save_index(&index)?;
        for id in removed {
            remove_state(&self.store, id);
        }
        Ok(Taken::New(meta))
    }

    /// Applique le nombre gardé (réglage changé).
    pub fn prune_to(&self, keep: usize) -> Result<usize> {
        let mut index = self.index();
        let removed = prune(&mut index.snapshots, keep);
        if removed.is_empty() {
            return Ok(0);
        }
        self.save_index(&index)?;
        for id in &removed {
            remove_state(&self.store, *id);
        }
        Ok(removed.len())
    }

    pub fn rename_snapshot(&self, id: Uuid, name: &str) -> Result<()> {
        let mut index = self.index();
        let Some(s) = index.snapshots.iter_mut().find(|s| s.id == id) else {
            bail!("instantané introuvable");
        };
        s.name = name.trim().to_owned();
        self.save_index(&index)
    }

    /// Efface un instantané (de n'importe quel type : c'est l'utilisateur qui le
    /// demande).
    pub fn delete_snapshot(&self, id: Uuid) -> Result<()> {
        let mut index = self.index();
        let before = index.snapshots.len();
        index.snapshots.retain(|s| s.id != id);
        if index.snapshots.len() == before {
            bail!("instantané introuvable");
        }
        self.save_index(&index)?;
        remove_state(&self.store, id);
        Ok(())
    }

    /// Enregistre `state` comme nouvelle disposition nommée.
    pub fn save_layout(&self, name: &str, state: &DesktopState, now: Timestamp) -> Result<Uuid> {
        let name = name.trim();
        if name.is_empty() {
            bail!("nom vide");
        }
        let mut index = self.index();
        let layout = NamedLayout {
            id: Uuid::new_v4(),
            name: name.to_owned(),
            hotkey: None,
            created: now,
            updated: now,
            hash: fingerprint(state),
            boxes: state.boxes.len(),
        };
        save_state(&self.store, layout.id, state)?;
        let id = layout.id;
        index.layouts.push(layout);
        self.save_index(&index)?;
        Ok(id)
    }

    /// Remplace le contenu d'une disposition nommée par `state`.
    pub fn update_layout(&self, id: Uuid, state: &DesktopState, now: Timestamp) -> Result<()> {
        let mut index = self.index();
        let Some(l) = index.layouts.iter_mut().find(|l| l.id == id) else {
            bail!("disposition introuvable");
        };
        save_state(&self.store, id, state)?;
        l.updated = now;
        l.hash = fingerprint(state);
        l.boxes = state.boxes.len();
        self.save_index(&index)
    }

    pub fn rename_layout(&self, id: Uuid, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            bail!("nom vide");
        }
        let mut index = self.index();
        let Some(l) = index.layouts.iter_mut().find(|l| l.id == id) else {
            bail!("disposition introuvable");
        };
        l.name = name.to_owned();
        self.save_index(&index)
    }

    /// Change le raccourci d'une disposition. Le refus des doublons est fait par la
    /// fenêtre principale (elle connaît tous les raccourcis) ; ici, un raccourci déjà
    /// pris par une autre disposition est seulement retiré de celle-ci.
    pub fn set_layout_hotkey(&self, id: Uuid, hotkey: Option<String>) -> Result<()> {
        let mut index = self.index();
        if !index.layouts.iter().any(|l| l.id == id) {
            bail!("disposition introuvable");
        }
        for l in &mut index.layouts {
            if l.id == id {
                l.hotkey = hotkey.clone();
            } else if hotkey.is_some() && l.hotkey == hotkey {
                l.hotkey = None;
            }
        }
        self.save_index(&index)
    }

    pub fn delete_layout(&self, id: Uuid) -> Result<()> {
        let mut index = self.index();
        let before = index.layouts.len();
        index.layouts.retain(|l| l.id != id);
        if index.layouts.len() == before {
            bail!("disposition introuvable");
        }
        self.save_index(&index)?;
        remove_state(&self.store, id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Size;
    use crate::model::{DeskBox, LooseIcon};

    fn screen(id: &str, x: f32, w: f32, h: f32, primary: bool) -> Screen {
        let frame = Rect::new(x, 0.0, w, h);
        Screen {
            id: ScreenId(id.into()),
            name: id.into(),
            frame,
            visible_frame: frame,
            scale: 1.0,
            primary,
        }
    }

    fn state_with(names: &[&str]) -> DesktopState {
        let mut state = DesktopState::default();
        for name in names {
            state.boxes.push(DeskBox::new(*name));
        }
        state
    }

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + seconds).unwrap()
    }

    fn meta(kind: SnapshotKind) -> SnapshotMeta {
        SnapshotMeta {
            id: Uuid::new_v4(),
            name: String::new(),
            kind,
            reason: SnapshotReason::Changed,
            created: at(0),
            hash: 0,
            boxes: 0,
        }
    }

    #[test]
    fn pruning_keeps_the_newest_autos_and_every_protected_one() {
        let mut list = vec![meta(SnapshotKind::First)];
        for _ in 0..3 {
            list.push(meta(SnapshotKind::Auto));
        }
        list.push(meta(SnapshotKind::Manual));
        for _ in 0..3 {
            list.push(meta(SnapshotKind::Auto));
        }
        let autos: Vec<Uuid> = list
            .iter()
            .filter(|s| s.kind == SnapshotKind::Auto)
            .map(|s| s.id)
            .collect();
        let removed = prune(&mut list, 2);
        assert_eq!(removed, autos[..4].to_vec(), "les plus anciens d'abord");
        assert_eq!(list.len(), 4);
        assert_eq!(list[0].kind, SnapshotKind::First);
        assert_eq!(list[1].kind, SnapshotKind::Manual);
        assert_eq!(list[2].id, autos[4]);
        assert_eq!(list[3].id, autos[5]);
        // Rien à retirer : rien ne bouge.
        assert!(prune(&mut list, 50).is_empty());
        assert!(prune(&mut list, 0).len() == 2);
        assert_eq!(list.len(), 2, "le premier et le manuel restent même à 0");
    }

    #[test]
    fn snapshots_round_trip_dedupe_and_protect() {
        let dir = tempfile::tempdir().unwrap();
        let layouts = Layouts::new(Store::open(dir.path()).unwrap());
        let a = state_with(&["Documents"]);
        let b = state_with(&["Documents", "Images"]);

        let first = layouts
            .take(
                &a,
                SnapshotKind::Auto,
                SnapshotReason::Changed,
                "",
                2,
                at(0),
            )
            .unwrap();
        let Taken::New(first) = first else {
            panic!("premier instantané attendu")
        };
        assert_eq!(first.kind, SnapshotKind::First);
        assert_eq!(first.boxes, 1);
        // Même disposition : pas de doublon.
        assert_eq!(
            layouts
                .take(
                    &a,
                    SnapshotKind::Auto,
                    SnapshotReason::Changed,
                    "",
                    2,
                    at(5)
                )
                .unwrap(),
            Taken::Duplicate
        );
        // Un manuel identique est pris quand même (demandé).
        let manual = layouts
            .take(
                &a,
                SnapshotKind::Manual,
                SnapshotReason::User,
                " Travail ",
                2,
                at(6),
            )
            .unwrap();
        assert!(
            matches!(manual, Taken::New(ref m) if m.kind == SnapshotKind::Manual && m.name == "Travail")
        );
        for (i, s) in [&b, &a, &b].into_iter().enumerate() {
            layouts
                .take(
                    s,
                    SnapshotKind::Auto,
                    SnapshotReason::Changed,
                    "",
                    2,
                    at(10 + i as i64),
                )
                .unwrap();
        }
        let index = layouts.index();
        let kinds: Vec<SnapshotKind> = index.snapshots.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SnapshotKind::First,
                SnapshotKind::Manual,
                SnapshotKind::Auto,
                SnapshotKind::Auto
            ]
        );
        // Les fichiers élagués sont retirés, les autres se relisent à l'identique.
        let files = fs::read_dir(dir.path().join(SNAPSHOT_DIR)).unwrap().count();
        assert_eq!(files, 4 + 1, "quatre instantanés et l'index");
        assert_eq!(load_state(layouts.store(), first.id).unwrap(), a);
        assert_eq!(
            load_state(layouts.store(), index.snapshots[3].id).unwrap(),
            b
        );
        assert_eq!(peek_index(layouts.store()), index);

        // Renommer, effacer (même le premier, à la main).
        layouts.rename_snapshot(first.id, "Installation").unwrap();
        assert_eq!(layouts.index().snapshots[0].name, "Installation");
        layouts.delete_snapshot(first.id).unwrap();
        assert!(layouts.index().snapshot(first.id).is_none());
        assert!(!state_path(layouts.store(), first.id).exists());
        assert!(layouts.delete_snapshot(first.id).is_err());
        // Le premier ne revient pas : le suivant reste automatique.
        let next = layouts
            .take(
                &b,
                SnapshotKind::Auto,
                SnapshotReason::Changed,
                "",
                2,
                at(30),
            )
            .unwrap();
        assert_eq!(next, Taken::Duplicate);
        let next = layouts
            .take(
                &a,
                SnapshotKind::Auto,
                SnapshotReason::Changed,
                "",
                2,
                at(31),
            )
            .unwrap();
        assert!(matches!(next, Taken::New(m) if m.kind == SnapshotKind::Auto));
        assert_eq!(layouts.prune_to(1).unwrap(), 1);
    }

    #[test]
    fn previous_skips_the_current_layout() {
        let a = state_with(&["A"]);
        let b = state_with(&["B"]);
        let mut index = LayoutIndex::default();
        for (s, kind) in [(&a, SnapshotKind::First), (&b, SnapshotKind::Auto)] {
            let mut m = meta(kind);
            m.hash = fingerprint(s);
            index.snapshots.push(m);
        }
        assert_eq!(
            index.previous(fingerprint(&b)).map(|s| s.hash),
            Some(fingerprint(&a))
        );
        assert_eq!(
            index.previous(fingerprint(&a)).map(|s| s.hash),
            Some(fingerprint(&b))
        );
    }

    #[test]
    fn fingerprint_ignores_loose_icons() {
        let mut a = state_with(&["A"]);
        let before = fingerprint(&a);
        a.loose
            .insert(ItemRef("x.txt".into()), LooseIcon::default());
        a.unassigned.push(ItemRef("y.txt".into()));
        assert_eq!(fingerprint(&a), before);
        a.boxes[0].rolled_up = true;
        assert_ne!(fingerprint(&a), before);
    }

    #[test]
    fn named_layouts_round_trip_with_unique_hotkeys() {
        let dir = tempfile::tempdir().unwrap();
        let layouts = Layouts::new(Store::open(dir.path()).unwrap());
        let work = state_with(&["Travail"]);
        let game = state_with(&["Jeux", "Lanceurs"]);
        assert!(layouts.save_layout("  ", &work, at(0)).is_err());
        let w = layouts.save_layout("Travail", &work, at(0)).unwrap();
        let g = layouts.save_layout("Jeux", &game, at(1)).unwrap();
        layouts
            .set_layout_hotkey(w, Some("control+alt+Digit1".into()))
            .unwrap();
        layouts
            .set_layout_hotkey(g, Some("control+alt+Digit1".into()))
            .unwrap();
        let index = layouts.index();
        assert_eq!(index.layout(w).unwrap().hotkey, None, "pris par l'autre");
        assert_eq!(index.hotkeys(), vec![(g, "control+alt+Digit1".into())]);
        layouts.update_layout(w, &game, at(5)).unwrap();
        assert_eq!(load_state(layouts.store(), w).unwrap(), game);
        assert_eq!(layouts.index().layout(w).unwrap().boxes, 2);
        layouts.rename_layout(w, "Soir").unwrap();
        assert_eq!(layouts.index().layout(w).unwrap().name, "Soir");
        layouts.delete_layout(w).unwrap();
        assert!(!state_path(layouts.store(), w).exists());
        assert_eq!(layouts.index().layouts.len(), 1);
    }

    #[test]
    fn restoring_returns_unboxed_items_to_the_desktop() {
        let mut current = state_with(&["Docs"]);
        current.boxes[0].tabs[0]
            .items
            .extend([ItemRef("a.pdf".into()), ItemRef("b.pdf".into())]);
        current.unassigned.push(ItemRef("c.pdf".into()));
        current
            .loose
            .insert(ItemRef("c.pdf".into()), LooseIcon::default());
        let mut snapshot = state_with(&["Ancienne"]);
        snapshot.boxes[0].tabs[0]
            .items
            .extend([ItemRef("b.pdf".into()), ItemRef("c.pdf".into())]);
        snapshot
            .loose
            .insert(ItemRef("disparu.txt".into()), LooseIcon::default());

        let restored = restored_state(&current, &snapshot);
        assert_eq!(restored.boxes, snapshot.boxes);
        let loose: Vec<&str> = restored.loose.keys().map(|i| i.0.as_str()).collect();
        assert_eq!(loose, ["a.pdf"], "rendu au bureau ; c.pdf est rangé");
        assert!(restored.unassigned.is_empty());
    }

    #[test]
    fn resolution_policies() {
        let small = [screen("dell", 0.0, 1000.0, 500.0, true)];
        let big = [screen("dell", 0.0, 2000.0, 1000.0, true)];
        let mut p = Placement::default();
        p.remember(&small, &small[0], Rect::new(500.0, 100.0, 200.0, 100.0));
        assert_eq!(
            p.by_screen_set[0].screen_size,
            Size {
                width: 1000.0,
                height: 500.0
            }
        );
        // Mémoriser : même position, gardée dans l'écran.
        let (_, r) = resolve_placement(&p, &big, ResolutionPolicy::RememberPerResolution).unwrap();
        assert_eq!(r, Rect::new(500.0, 100.0, 200.0, 100.0));
        // Redimensionner : proportions gardées.
        let (id, r) = resolve_placement(&p, &big, ResolutionPolicy::Rescale).unwrap();
        assert_eq!(id, ScreenId("dell".into()));
        assert_eq!(r, Rect::new(1000.0, 200.0, 400.0, 200.0));
        // Ne rien faire : comme mémoriser au démarrage.
        assert_eq!(
            resolve_placement(&p, &big, ResolutionPolicy::Nothing),
            p.resolve(&big)
        );
        // Plus petit : mis à l'échelle puis gardé dans l'écran.
        let tiny = [screen("dell", 0.0, 500.0, 250.0, true)];
        let (_, r) = resolve_placement(&p, &tiny, ResolutionPolicy::Rescale).unwrap();
        assert_eq!(r, Rect::new(250.0, 50.0, 100.0, 50.0));
        // Écran inconnu : pas de position.
        let other = [screen("lg", 0.0, 1000.0, 500.0, true)];
        assert!(resolve_placement(&p, &other, ResolutionPolicy::Rescale).is_none());
    }

    #[test]
    fn forgetting_other_screen_sets() {
        let laptop = screen("mac", 0.0, 1000.0, 600.0, true);
        let external = screen("dell", 1000.0, 1920.0, 1080.0, false);
        let both = [laptop.clone(), external.clone()];
        let alone = [laptop.clone()];
        let mut p = Placement::default();
        p.remember(&alone, &laptop, Rect::new(10.0, 10.0, 100.0, 100.0));
        p.remember(&both, &external, Rect::new(1100.0, 10.0, 100.0, 100.0));
        assert!(forget_other_screen_sets(&mut p, &both));
        assert_eq!(p.by_screen_set.len(), 1);
        assert_eq!(p.by_screen_set[0].screen_set, ScreenSetKey::of(&both));
        assert!(!forget_other_screen_sets(&mut p, &both));
        // Configuration sans position : celle qui sert est gardée.
        let lone_external = [external.clone()];
        assert!(!forget_other_screen_sets(&mut p, &lone_external));
        assert!(p.resolve(&lone_external).is_some());
    }

    #[test]
    fn thumbnail_shows_boxes_of_the_primary_screen() {
        let main = screen("mac", 0.0, 1000.0, 500.0, true);
        let side = screen("dell", 1000.0, 1000.0, 500.0, false);
        let screens = [main.clone(), side.clone()];
        let mut state = state_with(&["Ici", "Là"]);
        state.boxes[0]
            .placement
            .remember(&screens, &main, Rect::new(100.0, 50.0, 200.0, 100.0));
        state.boxes[0].rolled_up = true;
        state.boxes[1]
            .placement
            .remember(&screens, &side, Rect::new(1100.0, 50.0, 200.0, 100.0));
        // Une boîte sans position : à sa place par défaut (en haut à droite).
        state.boxes.push(DeskBox::new("Sans place"));
        let (ratio, boxes) = thumbnail(&state, &screens, ResolutionPolicy::RememberPerResolution);
        assert_eq!(ratio, 2.0);
        assert_eq!(boxes.len(), 2, "la boîte de l'autre écran n'y est pas");
        assert_eq!(
            boxes[0],
            ThumbBox {
                rect: Rect::new(0.1, 0.1, 0.2, 0.2),
                rolled: true
            }
        );
        assert_eq!(
            boxes[1].rect,
            Rect::new(0.0, 0.08, 0.32, 0.44),
            "troisième colonne, gardée dans l'écran"
        );
        assert!(
            thumbnail(&state, &[], ResolutionPolicy::Rescale)
                .1
                .is_empty()
        );
    }
}
