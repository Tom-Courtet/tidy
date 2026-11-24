# Règles communes des lots (vague 1)

Projet : **Tidy**, organiseur de bureau en Rust (edition 2024, egui/eframe 0.37 glow, winit 0.30, tray-icon 0.26, objc2 0.6 / objc2-* 0.3, windows-sys 0.61 et windows 0.62). **Cible prioritaire : Windows 10/11**, macOS en parallèle. Développement sur Mac ; la cible Windows est vérifiée par `cargo xwin clippy` et par la CI GitHub.

À lire avant de commencer : `CLAUDE.md`, `PLAN.md` (§ 1 bis et § 5 « Vague 1 »), `docs/FEATURES.md`, les ADR 001, 003, 011, 012 dans `docs/adr/`. Cahiers des charges d'origine : `/Users/tom/Downloads/SnappyxTidy/prompt-easy-desktop-rust.md` (le « prompt ») et `/Users/tom/Downloads/SnappyxTidy/iTop_Easy_Desktop.md` (le « bilan », observations sur l'original) ; le manuel officiel de l'original est résumé dans l'ADR 011.

## Contrats déjà posés (commit « contrats de la vague 1 »)
- **Un seul écrivain de `desktop.json` : le processus du bureau** (`app/src/desktop`). Les autres envoient une `tidy_core::DesktopCommand` par `Request::Desktop { command }` ; l'agent la relaie (`Event::Desktop`) ou l'applique au fichier si le bureau ne tourne pas (`agent/organizer.rs::route`). `DesktopState::apply` est la seule façon de modifier l'état.
- `Event::DesktopFilesChanged` : les fichiers du bureau ont changé (publié par l'agent, lot A ; consommé par le bureau, lot C).
- `Request::ApplyRules { all }`, `Request::UndoOrganize` : implémentés par le lot A dans `agent/organizer.rs` ; appelés par la fenêtre principale (lot D).
- `tidy_core::scan::scan_dir`, `tidy_core::sort::sort_items`, `tidy_core::snap::snap` : versions simples à compléter par le lot A **sans changer les signatures**.
- `DesktopPlatform` : méthodes de la vague 1 avec valeur par défaut « pas disponible » (`file_icon`, `show_file_menu`, `start_file_drag`, `trash`, `watch_screens`, `watch_desktop_drag`, `raise_desktop_surface`) : implémentées par le lot B. Les lots C et D doivent marcher même si elles rendent « pas disponible » (repli egui).
- Icônes rangées (Windows) : `app/src/icons.rs::sync` les masque en les déplaçant hors de l'écran (ADR 012). À rappeler après tout changement du contenu des boîtes.

## Exigences (non négociables)
1. **Ne jamais perdre un fichier.** Pas de suppression définitive (corbeille uniquement, via `DesktopPlatform::trash`) ; ranger un élément ne déplace pas le fichier (métadonnée seulement). Pas d'essai sur le vrai `~/Desktop` ni sur les vraies données : `TIDY_DATA_DIR=$(mktemp -d)` et `TIDY_DESKTOP_DIR=<faux bureau>`.
2. Ne pas changer un contrat ci-dessus ni le travail d'un autre lot. Si c'est nécessaire : ne pas le faire, le noter dans le compte rendu (« Décisions à demander »).
3. Rester dans ses fichiers (voir son lot). Petits ajouts tolérés ailleurs (une ligne `mod`, un appel) s'ils sont indispensables ; les signaler.
4. Rien d'irréversible ni de visible à l'extérieur : **pas de push**, pas de publication, pas de changement du Finder ou de l'Explorateur réels. Ne pas arrêter un Tidy qui tournerait sur la machine.

## Façon de travailler
- Code, commentaires (`//!`, `///`) et textes d'interface **en français**. Tout texte visible passe par `t!("section.clé")` : ajouter la clé **dans `fr.toml` et `en.toml`** (un test vérifie). Pour limiter les conflits, chaque lot ajoute ses clés dans **sa propre section** TOML (indiquée dans son lot), en fin de fichier.
- Couleurs, polices, widgets : uniquement `tidy_ui::theme` et `tidy_ui::widgets`. Icônes : `tidy_ui::icons` ; pour en ajouter, l'ajouter dans `tools/subset_icons.py` et relancer le script (paquet `lucide-static` 1.77.0, fontTools) — **seul le lot D le fait** ; les autres lots utilisent les icônes existantes ou le demandent dans le compte rendu.
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
