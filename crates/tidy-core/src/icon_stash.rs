//! Icônes rangées dans une boîte : masquées du bureau du système **en les déplaçant
//! hors de l'écran** (piste 1 retenue le 24/11/2026, voir PLAN § 1 bis).
//!
//! Aucun fichier n'est touché : seule la position de l'icône change. La position
//! d'origine de chaque icône est notée dans `icon-stash.json` **avant** de la déplacer ;
//! tant que ce journal existe, Tidy sait remettre chaque icône à sa place : en quittant,
//! en désinstallant, avec `tidy --restore-desktop`, ou au démarrage suivant après un
//! plantage.
//!
//! L'accès aux icônes du système passe par le trait [`DesktopIcons`] (implémenté par
//! `tidy-platform` sous Windows, remplacé par un faux dans les tests).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::store::{Store, Versioned};

/// Position d'une icône dans la vue du bureau (unités du système, opaques ici).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconPos {
    pub x: i32,
    pub y: i32,
}

/// Où envoyer les icônes rangées : loin hors de tout écran.
pub const OFFSCREEN: IconPos = IconPos {
    x: -30_000,
    y: -30_000,
};

/// Second essai si l'Explorateur refuse les positions négatives : loin à droite et en
/// bas.
pub const OFFSCREEN_ALT: IconPos = IconPos {
    x: 30_000,
    y: 30_000,
};

impl IconPos {
    /// L'icône est-elle hors de l'écran (déjà masquée) ?
    pub fn is_offscreen(self) -> bool {
        self.x.abs() >= OFFSCREEN_ALT.x / 2 || self.y.abs() >= OFFSCREEN_ALT.y / 2
    }
}

/// Icônes du bureau du système.
pub trait DesktopIcons {
    /// Chaque icône du bureau : chemin complet de l'élément et position.
    fn positions(&self) -> Result<Vec<(PathBuf, IconPos)>>;
    /// Déplace des icônes (les chemins absents du bureau sont ignorés).
    fn move_icons(&self, moves: &[(PathBuf, IconPos)]) -> Result<()>;
    /// Disposition automatique activée (elle empêche de placer les icônes) ?
    fn auto_arrange(&self) -> Result<bool>;
    fn set_auto_arrange(&self, on: bool) -> Result<()>;
    /// « Aligner les icônes sur la grille » activé (il peut ramener une icône à
    /// l'écran) ?
    fn snap_to_grid(&self) -> Result<bool> {
        Ok(false)
    }
    fn set_snap_to_grid(&self, _on: bool) -> Result<()> {
        Ok(())
    }
}

/// Journal des positions d'origine (`icon-stash.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IconStash {
    pub schema_version: u32,
    /// Position d'origine de chaque icône déplacée hors de l'écran.
    #[serde(default)]
    pub original: BTreeMap<PathBuf, IconPos>,
    /// Tidy a coupé la disposition automatique : à rétablir quand plus rien n'est
    /// masqué.
    #[serde(default)]
    pub auto_arrange_was_on: bool,
    /// Tidy a coupé l'alignement sur la grille (une icône revenait à l'écran) : à
    /// rétablir quand plus rien n'est masqué.
    #[serde(default)]
    pub snap_was_on: bool,
}

impl Default for IconStash {
    fn default() -> Self {
        Self {
            schema_version: Self::CURRENT_VERSION,
            original: BTreeMap::new(),
            auto_arrange_was_on: false,
            snap_was_on: false,
        }
    }
}

impl Versioned for IconStash {
    const CURRENT_VERSION: u32 = 1;
    const FILE_NAME: &'static str = "icon-stash.json";
}

/// Même chemin, sans tenir compte de la casse (bureau Windows et APFS par défaut).
fn same(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

fn find<'a>(list: &'a [(PathBuf, IconPos)], path: &Path) -> Option<&'a (PathBuf, IconPos)> {
    list.iter().find(|(p, _)| same(p, path))
}

/// Ce qu'a fait [`sync`].
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub hidden: usize,
    pub restored: usize,
    /// Icônes du bureau vues par le système.
    pub on_desktop: usize,
    /// Éléments rangés que le système ne montre pas sur le bureau (chemin différent,
    /// élément disparu) : rien à masquer.
    pub not_found: Vec<PathBuf>,
    /// Icônes rangées restées à l'écran malgré tous les essais.
    pub stuck: Vec<PathBuf>,
}

/// Verrou entre processus : l'agent et le bureau synchronisent chacun de leur côté ;
/// sans lui, l'un pourrait réécrire le journal sans les positions notées par l'autre.
fn lock(store: &Store) -> Result<std::fs::File> {
    let path = store.path_of::<IconStash>().with_extension("lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)?;
    file.lock()?;
    Ok(file)
}

/// Icônes de `wanted` encore à l'écran.
fn still_visible(icons: &dyn DesktopIcons, wanted: &[(PathBuf, IconPos)]) -> Result<Vec<PathBuf>> {
    let now = icons.positions()?;
    Ok(wanted
        .iter()
        .filter(|(path, _)| find(&now, path).is_some_and(|(_, pos)| !pos.is_offscreen()))
        .map(|(path, _)| path.clone())
        .collect())
}

/// Masque les icônes de `boxed` (éléments rangés) et rend toutes les autres icônes
/// masquées par Tidy. `boxed` vide : tout est rendu.
pub fn sync(store: &Store, icons: &dyn DesktopIcons, boxed: &[PathBuf]) -> Result<SyncReport> {
    let _lock = lock(store)?;
    // Lecture stricte : un journal illisible remplacé par un journal vide perdrait les
    // positions d'origine des icônes déjà masquées (elles resteraient hors de l'écran).
    let mut stash: IconStash = store.try_load()?;
    let current = icons.positions()?;
    let mut report = SyncReport {
        on_desktop: current.len(),
        ..SyncReport::default()
    };

    // 1. Rendre ce qui n'est plus rangé (ou plus sur le bureau).
    let mut back = Vec::new();
    stash.original.retain(|path, original| {
        if boxed.iter().any(|b| same(b, path)) {
            return true;
        }
        if find(&current, path).is_some() {
            back.push((path.clone(), *original));
        }
        false
    });
    if !back.is_empty() {
        icons.move_icons(&back)?;
        report.restored = back.len();
    }

    // 2. Masquer ce qui est rangé : noter la position d'origine d'abord.
    let mut away = Vec::new();
    for path in boxed {
        let Some((real, pos)) = find(&current, path) else {
            report.not_found.push(path.clone());
            continue;
        };
        if pos.is_offscreen() {
            continue;
        }
        // Déjà noté (le système l'a remise à l'écran) : garder la position d'origine.
        stash.original.entry(real.clone()).or_insert(*pos);
        away.push((real.clone(), OFFSCREEN));
    }
    if !away.is_empty() && icons.auto_arrange()? {
        stash.auto_arrange_was_on = true;
        store.save(&stash)?;
        icons.set_auto_arrange(false)?;
    }
    store.save(&stash)?;
    if !away.is_empty() {
        icons.move_icons(&away)?;
        report.hidden = away.len();
        // L'Explorateur peut refuser la position : vérifier, puis réessayer sans
        // l'alignement sur la grille, puis de l'autre côté de l'écran.
        let mut stuck = still_visible(icons, &away)?;
        if !stuck.is_empty() && icons.snap_to_grid()? {
            stash.snap_was_on = true;
            store.save(&stash)?;
            icons.set_snap_to_grid(false)?;
            let retry: Vec<_> = stuck.iter().map(|p| (p.clone(), OFFSCREEN)).collect();
            icons.move_icons(&retry)?;
            stuck = still_visible(icons, &retry)?;
        }
        if !stuck.is_empty() {
            let retry: Vec<_> = stuck.iter().map(|p| (p.clone(), OFFSCREEN_ALT)).collect();
            icons.move_icons(&retry)?;
            stuck = still_visible(icons, &retry)?;
        }
        report.hidden -= stuck.len();
        report.stuck = stuck;
    }

    // 3. Plus rien de masqué : rétablir ce que Tidy avait coupé.
    if stash.original.is_empty() && (stash.auto_arrange_was_on || stash.snap_was_on) {
        if stash.snap_was_on {
            icons.set_snap_to_grid(true)?;
            stash.snap_was_on = false;
        }
        if stash.auto_arrange_was_on {
            icons.set_auto_arrange(true)?;
            stash.auto_arrange_was_on = false;
        }
        store.save(&stash)?;
    }
    Ok(report)
}

/// Rend toutes les icônes masquées par Tidy.
pub fn restore_all(store: &Store, icons: &dyn DesktopIcons) -> Result<usize> {
    if !store.path_of::<IconStash>().exists() {
        return Ok(0);
    }
    sync(store, icons, &[]).map(|r| r.restored)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[derive(Default)]
    struct Fake {
        icons: RefCell<Vec<(PathBuf, IconPos)>>,
        auto: RefCell<bool>,
        snap: RefCell<bool>,
        /// Comme l'Explorateur avec l'alignement sur la grille : refuse les positions
        /// négatives.
        refuse_negative_with_snap: bool,
        /// Refuse toute position hors de l'écran.
        refuse_offscreen: bool,
    }

    impl Fake {
        fn with(items: &[(&str, i32, i32)]) -> Self {
            let fake = Fake::default();
            *fake.icons.borrow_mut() = items
                .iter()
                .map(|(p, x, y)| (PathBuf::from(p), IconPos { x: *x, y: *y }))
                .collect();
            fake
        }
        fn pos(&self, path: &str) -> IconPos {
            find(&self.icons.borrow(), Path::new(path)).unwrap().1
        }
    }

    impl DesktopIcons for Fake {
        fn positions(&self) -> Result<Vec<(PathBuf, IconPos)>> {
            Ok(self.icons.borrow().clone())
        }
        fn move_icons(&self, moves: &[(PathBuf, IconPos)]) -> Result<()> {
            assert!(
                !*self.auto.borrow(),
                "déplacement avec la disposition automatique"
            );
            for (path, pos) in moves {
                let negative = pos.x < 0 || pos.y < 0;
                if self.refuse_offscreen && pos.is_offscreen()
                    || self.refuse_negative_with_snap && *self.snap.borrow() && negative
                {
                    continue;
                }
                if let Some(icon) = self
                    .icons
                    .borrow_mut()
                    .iter_mut()
                    .find(|(p, _)| same(p, path))
                {
                    icon.1 = *pos;
                }
            }
            Ok(())
        }
        fn auto_arrange(&self) -> Result<bool> {
            Ok(*self.auto.borrow())
        }
        fn set_auto_arrange(&self, on: bool) -> Result<()> {
            *self.auto.borrow_mut() = on;
            Ok(())
        }
        fn snap_to_grid(&self) -> Result<bool> {
            Ok(*self.snap.borrow())
        }
        fn set_snap_to_grid(&self, on: bool) -> Result<()> {
            *self.snap.borrow_mut() = on;
            Ok(())
        }
    }

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn unreadable_stash_stops_everything() {
        let (_dir, store) = store();
        let fake = Fake::with(&[(r"C:\Bureau\a.pdf", 10, 20)]);
        std::fs::write(store.path_of::<IconStash>(), "{ abîmé").unwrap();
        let boxed = [PathBuf::from(r"C:\Bureau\a.pdf")];
        assert!(sync(&store, &fake, &boxed).is_err());
        assert!(restore_all(&store, &fake).is_err());
        // Rien n'a bougé, et le journal est resté tel quel.
        assert_eq!(fake.pos(r"C:\Bureau\a.pdf"), IconPos { x: 10, y: 20 });
        assert_eq!(
            std::fs::read_to_string(store.path_of::<IconStash>()).unwrap(),
            "{ abîmé"
        );
    }

    #[test]
    fn boxed_icons_go_offscreen_and_come_back() {
        let (_dir, store) = store();
        let fake = Fake::with(&[(r"C:\Bureau\a.pdf", 10, 20), (r"C:\Bureau\b.txt", 30, 40)]);
        let boxed = [PathBuf::from(r"C:\Bureau\a.pdf")];
        let report = sync(&store, &fake, &boxed).unwrap();
        assert_eq!(
            report,
            SyncReport {
                hidden: 1,
                on_desktop: 2,
                ..SyncReport::default()
            }
        );
        assert!(fake.pos(r"C:\Bureau\a.pdf").is_offscreen());
        assert_eq!(fake.pos(r"C:\Bureau\b.txt"), IconPos { x: 30, y: 40 });
        // Le journal existe avant tout retour.
        let stash: IconStash = store.load();
        assert_eq!(stash.original.len(), 1);

        assert_eq!(restore_all(&store, &fake).unwrap(), 1);
        assert_eq!(fake.pos(r"C:\Bureau\a.pdf"), IconPos { x: 10, y: 20 });
        assert!(store.load::<IconStash>().original.is_empty());
    }

    #[test]
    fn original_position_survives_a_second_hide() {
        let (_dir, store) = store();
        let fake = Fake::with(&[(r"C:\Bureau\a.pdf", 10, 20)]);
        let boxed = [PathBuf::from(r"c:\bureau\A.PDF")];
        sync(&store, &fake, &boxed).unwrap();
        // L'Explorateur la remet à l'écran ailleurs (redémarrage, par exemple).
        fake.move_icons(&[(
            PathBuf::from(r"C:\Bureau\a.pdf"),
            IconPos { x: 500, y: 500 },
        )])
        .unwrap();
        sync(&store, &fake, &boxed).unwrap();
        assert!(fake.pos(r"C:\Bureau\a.pdf").is_offscreen());
        restore_all(&store, &fake).unwrap();
        assert_eq!(fake.pos(r"C:\Bureau\a.pdf"), IconPos { x: 10, y: 20 });
    }

    #[test]
    fn auto_arrange_is_turned_off_then_restored() {
        let (_dir, store) = store();
        let fake = Fake::with(&[(r"C:\Bureau\a.pdf", 10, 20)]);
        *fake.auto.borrow_mut() = true;
        sync(&store, &fake, &[PathBuf::from(r"C:\Bureau\a.pdf")]).unwrap();
        assert!(!*fake.auto.borrow());
        restore_all(&store, &fake).unwrap();
        assert!(*fake.auto.borrow());
    }

    #[test]
    fn deleted_items_are_forgotten() {
        let (_dir, store) = store();
        let fake = Fake::with(&[(r"C:\Bureau\a.pdf", 10, 20)]);
        sync(&store, &fake, &[PathBuf::from(r"C:\Bureau\a.pdf")]).unwrap();
        fake.icons.borrow_mut().clear();
        assert_eq!(restore_all(&store, &fake).unwrap(), 0);
        assert!(store.load::<IconStash>().original.is_empty());
    }

    #[test]
    fn nothing_to_restore_without_a_journal() {
        let (_dir, store) = store();
        let fake = Fake::with(&[(r"C:\Bureau\a.pdf", 10, 20)]);
        assert_eq!(restore_all(&store, &fake).unwrap(), 0);
        assert!(!store.path_of::<IconStash>().exists());
    }

    #[test]
    fn snap_to_grid_is_turned_off_when_it_brings_icons_back() {
        let (_dir, store) = store();
        let mut fake = Fake::with(&[(r"C:\Bureau\a.pdf", 10, 20)]);
        fake.refuse_negative_with_snap = true;
        *fake.snap.borrow_mut() = true;
        let report = sync(&store, &fake, &[PathBuf::from(r"C:\Bureau\a.pdf")]).unwrap();
        assert_eq!(report.hidden, 1);
        assert!(report.stuck.is_empty());
        assert!(fake.pos(r"C:\Bureau\a.pdf").is_offscreen());
        assert!(!*fake.snap.borrow());
        restore_all(&store, &fake).unwrap();
        assert_eq!(fake.pos(r"C:\Bureau\a.pdf"), IconPos { x: 10, y: 20 });
        assert!(*fake.snap.borrow());
    }

    #[test]
    fn refused_moves_are_reported() {
        let (_dir, store) = store();
        let mut fake = Fake::with(&[(r"C:\Bureau\a.pdf", 10, 20)]);
        fake.refuse_offscreen = true;
        let boxed = [
            PathBuf::from(r"C:\Bureau\a.pdf"),
            PathBuf::from(r"C:\Ailleurs\b.pdf"),
        ];
        let report = sync(&store, &fake, &boxed).unwrap();
        assert_eq!(report.hidden, 0);
        assert_eq!(report.stuck, vec![PathBuf::from(r"C:\Bureau\a.pdf")]);
        assert_eq!(report.not_found, vec![PathBuf::from(r"C:\Ailleurs\b.pdf")]);
        // La position d'origine reste notée : rien n'est perdu.
        assert_eq!(fake.pos(r"C:\Bureau\a.pdf"), IconPos { x: 10, y: 20 });
        restore_all(&store, &fake).unwrap();
        assert_eq!(fake.pos(r"C:\Bureau\a.pdf"), IconPos { x: 10, y: 20 });
    }
}
