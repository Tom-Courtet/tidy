//! Moteur de rangement : qui va dans quelle boîte.
//!
//! Fonctions pures : elles lisent l'état du bureau, les réglages de rangement et la liste
//! des éléments du dossier, et rendent des [`DesktopCommand`] à faire appliquer par le
//! processus du bureau (seul écrivain de `desktop.json`). Aucun fichier n'est touché :
//! ranger un élément ne fait que le référencer dans une boîte.
//!
//! Ordre de décision pour un élément :
//! 1. les règles propres aux boîtes ([`DesktopState::rules`]) : la plus haute priorité
//!    gagne, puis la première de la liste ;
//! 2. la boîte choisie pour sa catégorie (écran « Rangement »,
//!    [`Organization::categories`]), avec les extensions personnalisées et l'option
//!    « ranger aussi les raccourcis » ;
//! 3. la boîte « fourre-tout » ([`Organization::catch_all`]) ;
//! 4. sinon, l'élément est « non affecté » (bandeau du bureau).
//!
//! Les règles qui visent une boîte disparue (ou qui n'est pas une boîte ordinaire) sont
//! ignorées.

use std::collections::BTreeSet;

use jiff::Timestamp;

use crate::category::{Category, classify_with, extension_of};
use crate::model::{BoxId, BoxKind, DeskBox, DesktopCommand, DesktopState, ItemRef};
use crate::rules::{ItemInfo, MatcherRegistry, Rule, target_for};
use crate::scan::{DesktopEntry, is_transient_name};
use crate::settings::{CategorySetting, Organization};

/// Éléments à ranger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Seulement ceux qui ne sont dans aucune boîte (« Appliquer aux non affectés »).
    Unassigned,
    /// Tous, y compris ceux déjà rangés (« Appliquer à tous »). Un élément rangé que
    /// plus aucune règle ne prend reste dans sa boîte.
    All,
}

/// Noms des boîtes créées par le démarrage rapide (traduits par l'appelant).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultBoxNames {
    /// Applications, exécutables, liens web et jeux.
    pub programs: String,
    pub folders: String,
    /// Tout le reste.
    pub files: String,
}

/// Résultat d'un rangement.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    /// Commandes à faire appliquer, dans l'ordre.
    pub commands: Vec<DesktopCommand>,
    /// Réglages de rangement modifiés par le démarrage rapide, à enregistrer avant
    /// d'appliquer les commandes.
    pub organization: Option<Organization>,
    /// Nombre d'éléments déplacés dans une boîte.
    pub moved: usize,
    /// Nombre d'éléments restés sans boîte.
    pub unassigned: usize,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && self.organization.is_none()
    }
}

/// Faut-il créer les boîtes par défaut ? Oui s'il n'existe ni boîte ordinaire ni
/// règle.
pub fn needs_quick_start(state: &DesktopState) -> bool {
    state.rules.is_empty() && !state.boxes.iter().any(|b| b.kind == BoxKind::Normal)
}

/// Démarrage rapide : les trois boîtes de l'original (« Programmes », « Dossiers »,
/// « Fichiers et documents ») et les catégories qui y mènent. Les extensions
/// personnalisées et les options des catégories sont gardées.
pub fn quick_start(org: &Organization, names: &DefaultBoxNames) -> (Vec<DeskBox>, Organization) {
    let programs = DeskBox::new(names.programs.clone());
    let folders = DeskBox::new(names.folders.clone());
    let files = DeskBox::new(names.files.clone());
    let mut org = org.clone();
    for category in Category::ALL {
        let target = match category {
            Category::Application | Category::Executable | Category::WebLink | Category::Game => {
                programs.id
            }
            Category::Folder => folders.id,
            Category::Document
            | Category::Image
            | Category::Video
            | Category::Music
            | Category::Archive
            | Category::Other => files.id,
        };
        org.categories
            .entry(category)
            .and_modify(|c| c.target = Some(target))
            .or_insert(CategorySetting {
                target: Some(target),
                extensions: None,
                include_aliases: false,
            });
    }
    (vec![programs, folders, files], org)
}

/// Catégorie d'un élément selon les réglages : extensions personnalisées et option
/// « ranger aussi les raccourcis » de la catégorie visée.
pub fn effective_category(entry: &DesktopEntry, org: &Organization) -> Category {
    if let Some(target) = entry.alias_of
        && org
            .categories
            .get(&target)
            .is_some_and(|c| c.include_aliases)
    {
        return target;
    }
    let info = &entry.info;
    // Catégorie tirée du contenu (raccourcis, liens de jeux) : on la garde.
    let by_content = info.category == Category::Game
        || matches!(
            extension_of(&info.name).as_deref(),
            Some("lnk" | "url" | "webloc" | "inetloc")
        );
    let customized = org.categories.values().any(|c| c.extensions.is_some());
    if by_content || !customized {
        return info.category;
    }
    classify_with(&info.name, info.is_dir, |category, ext| {
        match org
            .categories
            .get(&category)
            .and_then(|c| c.extensions.as_ref())
        {
            Some(list) => list.iter().any(|e| e.eq_ignore_ascii_case(ext)),
            None => category.default_extensions().contains(&ext),
        }
    })
}

/// Ce qui décide de la boîte d'un élément, préparé une fois pour tout un rangement.
pub struct Resolver<'a> {
    org: &'a Organization,
    rules: Vec<Rule>,
    boxes: BTreeSet<BoxId>,
    registry: &'a MatcherRegistry,
    now: Timestamp,
}

impl<'a> Resolver<'a> {
    pub fn new(
        state: &DesktopState,
        org: &'a Organization,
        registry: &'a MatcherRegistry,
        now: Timestamp,
    ) -> Self {
        let boxes: BTreeSet<BoxId> = state
            .boxes
            .iter()
            .filter(|b| b.kind == BoxKind::Normal)
            .map(|b| b.id)
            .collect();
        let rules = state
            .rules
            .iter()
            .filter(|r| boxes.contains(&r.target))
            .cloned()
            .collect();
        Self {
            org,
            rules,
            boxes,
            registry,
            now,
        }
    }

    /// Boîte où ranger `entry`, ou `None` s'il n'y en a pas.
    pub fn target(&self, entry: &DesktopEntry) -> Option<BoxId> {
        let category = effective_category(entry, self.org);
        let info = ItemInfo {
            category,
            ..entry.info.clone()
        };
        let exists = |id: &BoxId| self.boxes.contains(id);
        target_for(&self.rules, &info, self.registry, self.now)
            .or_else(|| {
                self.org
                    .categories
                    .get(&category)
                    .and_then(|c| c.target)
                    .filter(exists)
            })
            .or_else(|| self.org.catch_all.filter(exists))
    }
}

/// Range les éléments du bureau `entries` (« Appliquer aux non affectés » ou « à
/// tous »). Avec `quick_start_names` et [`Scope::All`], crée d'abord les boîtes par
/// défaut si [`needs_quick_start`].
pub fn plan(
    state: &DesktopState,
    org: &Organization,
    entries: &[DesktopEntry],
    scope: Scope,
    quick_start_names: Option<&DefaultBoxNames>,
    registry: &MatcherRegistry,
    now: Timestamp,
) -> Plan {
    let mut result = Plan::default();
    let mut view = state.clone();
    let mut org = org.clone();
    if scope == Scope::All
        && let Some(names) = quick_start_names
        && needs_quick_start(state)
    {
        let (boxes, new_org) = quick_start(&org, names);
        for b in boxes {
            let command = DesktopCommand::UpsertBox { b };
            view.apply(command.clone());
            result.commands.push(command);
        }
        org = new_org;
        result.organization = Some(org.clone());
    }

    let resolver = Resolver::new(&view, &org, registry, now);
    let mut moves = Vec::new();
    let mut unassigned = Vec::new();
    for entry in entries {
        if is_transient_name(&entry.info.name) {
            continue;
        }
        let item = ItemRef(entry.info.name.clone());
        let current = view.box_of(&item);
        if scope == Scope::Unassigned && current.is_some() {
            continue;
        }
        match resolver.target(entry) {
            Some(target) if current != Some(target) => moves.push((item, Some(target))),
            Some(_) => {}
            None if current.is_none() => unassigned.push(item),
            None => {}
        }
    }
    result.moved = moves.len();
    result.unassigned = unassigned.len();
    if !moves.is_empty() {
        result
            .commands
            .push(DesktopCommand::Assign { items: moves });
    }
    if unassigned != view.unassigned {
        result
            .commands
            .push(DesktopCommand::SetUnassigned { items: unassigned });
    }
    result
}

/// Nouveaux éléments arrivés sur le bureau : rangés selon les règles si
/// [`Organization::auto_apply_to_new`], sinon (ou sans règle) ajoutés aux non affectés.
/// Ceux déjà dans une boîte ne bougent pas.
pub fn plan_new_items(
    state: &DesktopState,
    org: &Organization,
    entries: &[DesktopEntry],
    registry: &MatcherRegistry,
    now: Timestamp,
) -> Vec<DesktopCommand> {
    let resolver = Resolver::new(state, org, registry, now);
    let mut moves = Vec::new();
    let mut unassigned = state.unassigned.clone();
    for entry in entries {
        if is_transient_name(&entry.info.name) {
            continue;
        }
        let item = ItemRef(entry.info.name.clone());
        if state.box_of(&item).is_some() {
            continue;
        }
        let target = org
            .auto_apply_to_new
            .then(|| resolver.target(entry))
            .flatten();
        match target {
            Some(target) => {
                unassigned.retain(|i| i != &item);
                moves.push((item, Some(target)));
            }
            None if !unassigned.contains(&item) => unassigned.push(item),
            None => {}
        }
    }
    let mut commands = Vec::new();
    if unassigned != state.unassigned {
        commands.push(DesktopCommand::SetUnassigned { items: unassigned });
    }
    if !moves.is_empty() {
        commands.push(DesktopCommand::Assign { items: moves });
    }
    commands
}

/// Éléments disparus du bureau : retirés de leur boîte (sans changer l'ordre des
/// autres) et des non affectés.
pub fn plan_removed(state: &DesktopState, removed: &[ItemRef]) -> Vec<DesktopCommand> {
    let gone = |i: &ItemRef| removed.contains(i);
    let mut commands = Vec::new();
    for b in &state.boxes {
        if b.kind == BoxKind::Normal && b.items().any(gone) {
            let mut b = b.clone();
            for tab in &mut b.tabs {
                tab.items.retain(|i| !gone(i));
            }
            commands.push(DesktopCommand::UpsertBox { b });
        }
    }
    if state.unassigned.iter().any(gone) {
        let items = state
            .unassigned
            .iter()
            .filter(|i| !gone(i))
            .cloned()
            .collect();
        commands.push(DesktopCommand::SetUnassigned { items });
    }
    commands
}

/// Élément renommé : il garde sa boîte, son onglet et sa place (ou sa place parmi les
/// non affectés).
pub fn plan_renamed(state: &DesktopState, from: &ItemRef, to: &ItemRef) -> Vec<DesktopCommand> {
    let mut commands = Vec::new();
    for b in &state.boxes {
        if b.kind == BoxKind::Normal && b.contains(from) {
            let mut b = b.clone();
            for tab in &mut b.tabs {
                tab.items.retain(|i| i != to);
                for i in &mut tab.items {
                    if i == from {
                        *i = to.clone();
                    }
                }
            }
            commands.push(DesktopCommand::UpsertBox { b });
        }
    }
    if state.unassigned.contains(from) {
        let mut items: Vec<ItemRef> = Vec::new();
        for i in &state.unassigned {
            let i = if i == from { to } else { i };
            if !items.contains(i) {
                items.push(i.clone());
            }
        }
        commands.push(DesktopCommand::SetUnassigned { items });
    }
    commands
}

/// Éléments du bureau que l'état ne connaît pas (ni rangés, ni hors boîte, ni non
/// affectés) : arrivés pendant que Tidy ne tournait pas.
pub fn unknown_entries<'e>(
    state: &DesktopState,
    entries: &'e [DesktopEntry],
) -> Vec<&'e DesktopEntry> {
    entries
        .iter()
        .filter(|e| {
            let item = ItemRef(e.info.name.clone());
            state.box_of(&item).is_none()
                && !state.loose.contains_key(&item)
                && !state.unassigned.contains(&item)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Criterion;

    fn entry(name: &str, category: Category) -> DesktopEntry {
        DesktopEntry::new(ItemInfo {
            name: name.into(),
            is_dir: category == Category::Folder,
            category,
            size: 10,
            modified: None,
            uti: None,
        })
    }

    fn names() -> DefaultBoxNames {
        DefaultBoxNames {
            programs: "Programmes".into(),
            folders: "Dossiers".into(),
            files: "Fichiers et documents".into(),
        }
    }

    /// Applique le plan à l'état, comme le ferait le processus du bureau.
    fn run(state: &mut DesktopState, org: &mut Organization, plan: Plan) {
        if let Some(o) = plan.organization {
            *org = o;
        }
        for c in plan.commands {
            state.apply(c);
        }
    }

    fn box_named(state: &DesktopState, name: &str) -> BoxId {
        state.boxes.iter().find(|b| b.name == name).unwrap().id
    }

    fn reg() -> MatcherRegistry {
        MatcherRegistry::default()
    }

    fn desktop() -> Vec<DesktopEntry> {
        vec![
            entry("Firefox.lnk", Category::Application),
            entry("Dota 2.url", Category::Game),
            entry("Projets", Category::Folder),
            entry("Rapport.pdf", Category::Document),
            entry("photo.jpg", Category::Image),
            entry("notes", Category::Other),
            entry("Setup.exe", Category::Executable),
            entry("Wikipédia.url", Category::WebLink),
        ]
    }

    #[test]
    fn quick_start_creates_the_three_default_boxes() {
        let mut state = DesktopState::default();
        let mut org = Organization::default();
        let entries = desktop();
        let plan = plan(
            &state,
            &org,
            &entries,
            Scope::All,
            Some(&names()),
            &reg(),
            Timestamp::now(),
        );
        assert_eq!(plan.moved, entries.len());
        assert_eq!(plan.unassigned, 0);
        run(&mut state, &mut org, plan);
        assert_eq!(state.boxes.len(), 3);
        let programs = box_named(&state, "Programmes");
        let folders = box_named(&state, "Dossiers");
        let files = box_named(&state, "Fichiers et documents");
        let box_of = |n: &str| state.box_of(&ItemRef(n.into()));
        for n in ["Firefox.lnk", "Dota 2.url", "Setup.exe", "Wikipédia.url"] {
            assert_eq!(box_of(n), Some(programs), "{n}");
        }
        assert_eq!(box_of("Projets"), Some(folders));
        for n in ["Rapport.pdf", "photo.jpg", "notes"] {
            assert_eq!(box_of(n), Some(files), "{n}");
        }
        assert_eq!(
            org.categories[&Category::Game].target,
            Some(programs),
            "les réglages pointent vers les nouvelles boîtes"
        );

        // Deuxième passage : plus de démarrage rapide, rien à faire.
        let again = super::plan(
            &state,
            &org,
            &entries,
            Scope::All,
            Some(&names()),
            &reg(),
            Timestamp::now(),
        );
        assert!(again.is_empty(), "{again:?}");
    }

    #[test]
    fn no_quick_start_when_boxes_or_rules_exist_or_for_unassigned_scope() {
        let mut state = DesktopState::default();
        let plan_unassigned = plan(
            &state,
            &Organization::default(),
            &desktop(),
            Scope::Unassigned,
            Some(&names()),
            &reg(),
            Timestamp::now(),
        );
        assert!(state.boxes.is_empty());
        assert!(plan_unassigned.organization.is_none());
        // Aucune règle : tout est non affecté.
        assert_eq!(plan_unassigned.unassigned, desktop().len());

        state.boxes.push(DeskBox::new("À moi"));
        assert!(!needs_quick_start(&state));
        let p = plan(
            &state,
            &Organization::default(),
            &desktop(),
            Scope::All,
            Some(&names()),
            &reg(),
            Timestamp::now(),
        );
        assert!(p.organization.is_none());
        assert!(
            !p.commands
                .iter()
                .any(|c| matches!(c, DesktopCommand::UpsertBox { .. }))
        );
    }

    #[test]
    fn box_rules_beat_categories_then_catch_all_then_unassigned() {
        let mut state = DesktopState::default();
        let docs = DeskBox::new("Documents");
        let invoices = DeskBox::new("Factures");
        let misc = DeskBox::new("Fourre-tout");
        state
            .boxes
            .extend([docs.clone(), invoices.clone(), misc.clone()]);
        let mut low = Rule::new(
            docs.id,
            Criterion::NamePattern {
                pattern: "facture*".into(),
            },
        );
        low.priority = 1;
        let mut high = Rule::new(
            invoices.id,
            Criterion::NamePattern {
                pattern: "facture*".into(),
            },
        );
        high.priority = 5;
        state.rules = vec![low, high];
        let mut org = Organization::default();
        org.categories.insert(
            Category::Document,
            CategorySetting {
                target: Some(docs.id),
                extensions: None,
                include_aliases: false,
            },
        );
        let entries = vec![
            entry("Facture mars.pdf", Category::Document),
            entry("Rapport.pdf", Category::Document),
            entry("photo.jpg", Category::Image),
        ];
        let now = Timestamp::now();
        let p = plan(&state, &org, &entries, Scope::Unassigned, None, &reg(), now);
        let mut s = state.clone();
        run(&mut s, &mut org.clone(), p);
        assert_eq!(
            s.box_of(&ItemRef("Facture mars.pdf".into())),
            Some(invoices.id)
        );
        assert_eq!(s.box_of(&ItemRef("Rapport.pdf".into())), Some(docs.id));
        assert_eq!(s.box_of(&ItemRef("photo.jpg".into())), None);
        assert_eq!(s.unassigned, vec![ItemRef("photo.jpg".into())]);

        // Avec une boîte fourre-tout, plus rien n'est non affecté.
        org.catch_all = Some(misc.id);
        let p = plan(&state, &org, &entries, Scope::Unassigned, None, &reg(), now);
        let mut s = state.clone();
        run(&mut s, &mut org.clone(), p);
        assert_eq!(s.box_of(&ItemRef("photo.jpg".into())), Some(misc.id));
        assert!(s.unassigned.is_empty());

        // Une règle vers une boîte disparue est ignorée.
        org.catch_all = None;
        let mut gone = state.clone();
        gone.boxes.retain(|b| b.id != invoices.id);
        let p = plan(&gone, &org, &entries, Scope::Unassigned, None, &reg(), now);
        run(&mut gone, &mut org.clone(), p);
        assert_eq!(
            gone.box_of(&ItemRef("Facture mars.pdf".into())),
            Some(docs.id)
        );
    }

    #[test]
    fn unassigned_scope_leaves_boxed_items_alone_and_all_moves_them() {
        let mut state = DesktopState::default();
        let a = DeskBox::new("A");
        let b = DeskBox::new("B");
        state.boxes.extend([a.clone(), b.clone()]);
        state.rules = vec![Rule::category(b.id, Category::Image)];
        state.apply(DesktopCommand::Assign {
            items: vec![
                (ItemRef("photo.jpg".into()), Some(a.id)),
                (ItemRef("notes".into()), Some(a.id)),
            ],
        });
        let org = Organization::default();
        let entries = vec![
            entry("photo.jpg", Category::Image),
            entry("notes", Category::Other),
        ];
        let now = Timestamp::now();
        let p = plan(&state, &org, &entries, Scope::Unassigned, None, &reg(), now);
        assert!(p.is_empty(), "{p:?}");
        let p = plan(&state, &org, &entries, Scope::All, None, &reg(), now);
        assert_eq!(p.moved, 1);
        let mut s = state.clone();
        run(&mut s, &mut org.clone(), p);
        assert_eq!(s.box_of(&ItemRef("photo.jpg".into())), Some(b.id));
        // Sans règle, il reste dans sa boîte.
        assert_eq!(s.box_of(&ItemRef("notes".into())), Some(a.id));
    }

    #[test]
    fn custom_extensions_and_shortcut_option() {
        let mut state = DesktopState::default();
        let docs = DeskBox::new("Docs");
        let apps = DeskBox::new("Applis");
        state.boxes.extend([docs.clone(), apps.clone()]);
        let mut org = Organization::default();
        org.categories.insert(
            Category::Document,
            CategorySetting {
                target: Some(docs.id),
                extensions: Some(vec!["pdf".into(), "LOG".into()]),
                include_aliases: false,
            },
        );
        org.categories.insert(
            Category::Application,
            CategorySetting {
                target: Some(apps.id),
                extensions: None,
                include_aliases: false,
            },
        );
        let mut shortcut = entry("Rapport.lnk", Category::Application);
        shortcut.alias_of = Some(Category::Document);
        let entries = [
            entry("journal.log", Category::Other),
            entry("lettre.docx", Category::Document),
            shortcut,
        ];
        let now = Timestamp::now();
        let registry = reg();
        let resolver = Resolver::new(&state, &org, &registry, now);
        assert_eq!(resolver.target(&entries[0]), Some(docs.id));
        // « docx » retiré de la liste : plus un document.
        assert_eq!(resolver.target(&entries[1]), None);
        assert_eq!(resolver.target(&entries[2]), Some(apps.id));
        org.categories
            .get_mut(&Category::Document)
            .unwrap()
            .include_aliases = true;
        let resolver = Resolver::new(&state, &org, &registry, now);
        assert_eq!(resolver.target(&entries[2]), Some(docs.id));
    }

    #[test]
    fn new_items_follow_auto_apply() {
        let mut state = DesktopState::default();
        let images = DeskBox::new("Images");
        state.boxes.push(images.clone());
        state.rules = vec![Rule::category(images.id, Category::Image)];
        let mut org = Organization::default();
        let now = Timestamp::now();
        let new = vec![
            entry("photo.jpg", Category::Image),
            entry("notes", Category::Other),
            entry("film.mp4.crdownload", Category::Other),
        ];
        let mut s = state.clone();
        for c in plan_new_items(&s.clone(), &org, &new, &reg(), now) {
            s.apply(c);
        }
        assert_eq!(s.box_of(&ItemRef("photo.jpg".into())), Some(images.id));
        assert_eq!(s.unassigned, vec![ItemRef("notes".into())]);

        org.auto_apply_to_new = false;
        let mut s = state.clone();
        for c in plan_new_items(&s.clone(), &org, &new, &reg(), now) {
            s.apply(c);
        }
        assert_eq!(s.box_of(&ItemRef("photo.jpg".into())), None);
        assert_eq!(
            s.unassigned,
            vec![ItemRef("photo.jpg".into()), ItemRef("notes".into())]
        );
        // Déjà connu : pas de doublon.
        assert!(plan_new_items(&s, &org, &new, &reg(), now).is_empty());
    }

    #[test]
    fn removed_and_renamed_items() {
        let mut state = DesktopState::default();
        let mut b = DeskBox::new("A");
        b.tabs[0].items = vec![
            ItemRef("a".into()),
            ItemRef("b".into()),
            ItemRef("c".into()),
        ];
        let mut second = crate::model::Tab::new("2");
        second.items.push(ItemRef("d".into()));
        b.tabs.push(second);
        let id = b.id;
        state.boxes.push(b);
        state.unassigned = vec![ItemRef("x".into()), ItemRef("y".into())];

        let mut s = state.clone();
        for c in plan_renamed(&s.clone(), &ItemRef("b".into()), &ItemRef("B2".into())) {
            s.apply(c);
        }
        for c in plan_renamed(&s.clone(), &ItemRef("d".into()), &ItemRef("D2".into())) {
            s.apply(c);
        }
        for c in plan_renamed(&s.clone(), &ItemRef("x".into()), &ItemRef("X2".into())) {
            s.apply(c);
        }
        let tabs = &s.find_box(id).unwrap().tabs;
        assert_eq!(
            tabs[0].items,
            vec![
                ItemRef("a".into()),
                ItemRef("B2".into()),
                ItemRef("c".into())
            ]
        );
        assert_eq!(tabs[1].items, vec![ItemRef("D2".into())]);
        assert_eq!(
            s.unassigned,
            vec![ItemRef("X2".into()), ItemRef("y".into())]
        );
        assert!(plan_renamed(&s, &ItemRef("inconnu".into()), &ItemRef("z".into())).is_empty());

        for c in plan_removed(&s.clone(), &[ItemRef("a".into()), ItemRef("y".into())]) {
            s.apply(c);
        }
        let tabs = &s.find_box(id).unwrap().tabs;
        assert_eq!(
            tabs[0].items,
            vec![ItemRef("B2".into()), ItemRef("c".into())]
        );
        assert_eq!(s.unassigned, vec![ItemRef("X2".into())]);
        assert!(plan_removed(&s, &[ItemRef("absent".into())]).is_empty());
    }

    #[test]
    fn unknown_entries_are_those_tidy_never_saw() {
        let mut state = DesktopState::default();
        let b = DeskBox::new("A");
        let id = b.id;
        state.boxes.push(b);
        state.apply(DesktopCommand::Assign {
            items: vec![
                (ItemRef("rangé".into()), Some(id)),
                (ItemRef("posé".into()), None),
            ],
        });
        state.unassigned.push(ItemRef("signalé".into()));
        let entries = vec![
            entry("rangé", Category::Other),
            entry("posé", Category::Other),
            entry("signalé", Category::Other),
            entry("nouveau", Category::Other),
        ];
        let unknown: Vec<_> = unknown_entries(&state, &entries)
            .iter()
            .map(|e| e.info.name.as_str())
            .collect();
        assert_eq!(unknown, ["nouveau"]);
    }
}
