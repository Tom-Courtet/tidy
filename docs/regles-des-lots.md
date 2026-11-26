# Règles communes des lots (vagues 1 à 3)

Projet : **Tidy**, organiseur de bureau en Rust (edition 2024, egui/eframe 0.37 glow, winit 0.30, tray-icon 0.26, objc2 0.6 / objc2-* 0.3, windows-sys 0.61 et windows 0.62). **Cible prioritaire : Windows 10/11**, macOS en parallèle. Développement sur Mac ; la cible Windows est vérifiée par `cargo xwin clippy` et par la CI GitHub.

À lire avant de commencer : `CLAUDE.md`, `PLAN.md` (§ 1 bis et § 5 « Vague 1 »), `docs/FEATURES.md`, les ADR 001, 003, 011, 012 dans `docs/adr/`. Cahiers des charges d'origine : `/Users/tom/Downloads/SnappyxTidy/prompt-easy-desktop-rust.md` (le « prompt ») et `/Users/tom/Downloads/SnappyxTidy/iTop_Easy_Desktop.md` (le « bilan », observations sur l'original) ; le manuel officiel de l'original est résumé dans l'ADR 011.

## Contrats déjà posés

Vague 1 (commit « contrats de la vague 1 ») :
- **Un seul écrivain de `desktop.json` : le processus du bureau** (`app/src/desktop`). Les autres envoient une `tidy_core::DesktopCommand` par `Request::Desktop { command }` ; l'agent la relaie (`Event::Desktop`) ou l'applique au fichier si le bureau ne tourne pas (`agent/organizer.rs::route`). `DesktopState::apply` est la seule façon de modifier l'état.
- `Event::DesktopFilesChanged`, `Request::ApplyRules { all }`, `Request::UndoOrganize`, `tidy_core::scan`, `sort`, `snap`, `grid` (grille libre).
- `DesktopPlatform` : méthodes avec valeur par défaut « pas disponible » ; l'appli marche sans elles (repli egui).
- Icônes rangées (Windows) : `app/src/icons.rs::sync` (ADR 012). Éléments du bureau : `tidy_core::store::desktop_dirs()` (bureau de l'utilisateur + bureau public de Windows), `scan_desktop`, `resolve_desktop_item`.

Vagues 2 et 3 (commit « contrats des vagues 2 et 3 ») :
- **Messages de module** : `Request::Module { module, action, data }` (IPC) et `Event::Module { module, action, data }`. `agent/modules.rs::handle` aiguille par module (**une ligne par lot**, vers une fonction dans son propre fichier `agent/<module>.rs`) ; sans aiguillage, la demande est relayée à tous les abonnés. `Response::Data { data }` pour répondre. Noms de modules : `layouts`, `vault`, `search`, `widgets`, `wallpaper`.
- **Réglages par lot** : `Settings::{search, vault, widgets, wallpaper}`, chacun dans son fichier `crates/tidy-core/src/settings/<module>.rs` (structure vide à remplir, champs `#[serde(default)]`). Dispositions : `LayoutSettings` et `SnapshotSettings` existent déjà dans `settings.rs`.
- **Modèle** : `DeskBox::{frozen, search_in_title}`, `BoxKind::Private`, `DesktopState::widgets` (`WidgetInstance`), `DesktopCommand::{UpsertWidget, RemoveWidget}`.
- **Widgets** : trait `tidy_widgets::Widget`, `WidgetKind`, registre `kinds()` (une ligne par widget), `WidgetCx`, `WidgetAction`. Hôte : `app/src/desktop/widgets.rs::WidgetHost::show`, déjà appelé à chaque image par le bureau (il rend des `DesktopCommand`).
- **Processus** : `tidy --search [texte]` → `app/src/search/mod.rs::run` ; `tidy --wallpaper` → `app/src/wallpaper/mod.rs::run` (squelettes à remplir).
- **Pages de la fenêtre principale** : `Page::{Layouts, Wallpapers, Widgets, Search}` existent (« à venir ») ; chaque lot crée `app/src/manager/<page>.rs` et remplace la ligne `page => coming_soon` correspondante dans `manager/pages.rs` par un appel à sa page.
- **Icônes** : utiliser celles de `tidy_ui::icons` (LOCK, UNLOCK, KEY, SHIELD, SEARCH, CLOCK, NOTE, TIMER, WEATHER, CALENDAR, AI, TOOLS, CAMERA, BELL, LAYOUT, SNAPSHOT, WALLPAPER, WIDGETS, PLAY, PAUSE, VOLUME, MUTE, PIN, REFRESH, GLASS, IMAGE, VIDEO…). Pas de régénération de la police dans ces vagues.

## Exigences (non négociables)
1. **Ne jamais perdre un fichier.** Pas de suppression définitive (corbeille uniquement, via `DesktopPlatform::trash`) ; ranger un élément ne déplace pas le fichier (métadonnée seulement). Pas d'essai sur le vrai `~/Desktop` ni sur les vraies données : `TIDY_DATA_DIR=$(mktemp -d)` et `TIDY_DESKTOP_DIR=<faux bureau>`.
2. Ne pas changer un contrat ci-dessus ni le travail d'un autre lot. Si c'est nécessaire : ne pas le faire, le noter dans le compte rendu (« Décisions à demander »).
3. Rester dans ses fichiers (voir son lot). Petits ajouts tolérés ailleurs (une ligne `mod`, un appel) s'ils sont indispensables ; les signaler.
4. Rien d'irréversible ni de visible à l'extérieur : **pas de push**, pas de publication, pas de changement du Finder ou de l'Explorateur réels. Ne pas arrêter un Tidy qui tournerait sur la machine.

## Façon de travailler
- Code, commentaires (`//!`, `///`) et textes d'interface **en français**. Tout texte visible passe par `t!("section.clé")` : ajouter la clé **dans `fr.toml` et `en.toml`** (un test vérifie). Pour limiter les conflits, chaque lot ajoute ses clés dans **sa propre section** TOML (indiquée dans son lot), en fin de fichier.
- Windows d'abord : chaque fonction doit marcher sous Windows 10/11 ; macOS ensuite, ou repli propre (« pas disponible sur cet OS ») noté dans le compte rendu.
- Couleurs, polices, widgets : uniquement `tidy_ui::theme` et `tidy_ui::widgets`. Icônes : `tidy_ui::icons` existantes uniquement ; un besoin d'icône nouvelle va dans le compte rendu.
- `unsafe` seulement dans `tidy-platform`, avec `// SAFETY:` à chaque bloc.
- Nouveaux champs persistés : `#[serde(default)]`, en fin de structure.
- Dépendances : préférer les crates légères ; ajouter en fin de section, commentées ; signaler toute licence GPL/AGPL au lieu de l'ajouter. `notify` (surveillance de dossiers) et `trash` sont admis.
- Tests unitaires pour toute logique pure ; tests sur un faux bureau (dossier temporaire).
- Vérifier avant chaque commit :
  ```sh
  cargo fmt --all
  cargo clippy --workspace --all-targets -- -D warnings
  cargo xwin clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
  cargo test --workspace
  ```
  Pas de build release (place sur le disque).
- Commits petits, en français, Conventional Commits (`feat(lot-x): …`), **sans aucune mention de Claude** (pas de `Co-Authored-By`). Pas de push.
- À la fin : `cargo clean` dans ta copie du dépôt.

## Compte rendu final (en français, concis)
1. Ajouté (par fonction, avec fichiers).
2. Touché hors de ton lot (fichier, pourquoi).
3. Décisions à demander (question · options · recommandation).
4. À vérifier sur une vraie machine (Windows surtout) et limites connues.
5. Nom de ta branche et liste des commits.
