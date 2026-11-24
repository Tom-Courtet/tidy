# Tidy : consignes pour Claude Code

Organiseur de bureau léger et personnalisable pour macOS (Windows plus tard), en Rust. Réimplémentation fonctionnelle d'iTop Easy Desktop, sans rien reprendre de son identité. Plan et vagues de travail : `PLAN.md`. Parité : `docs/FEATURES.md`. Décisions : `docs/adr/`.

Projets frères dont Tidy reprend la stack et la DA : `../snappy`, `../tappy`, `../Soundboard` (crate `soundy`).

## Commandes

```sh
cargo build                                   # tout le workspace (binaire : target/debug/tidy)
cargo test --workspace                        # tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo xwin check --target x86_64-pc-windows-msvc   # la cible Windows doit compiler
sh tools/bundle_macos.sh                      # dist/Tidy.app (release)
```

Essais sans toucher aux vraies données : `TIDY_DATA_DIR=$(mktemp -d) target/debug/tidy`.
Niveau des journaux : `TIDY_LOG=debug`. Journaux : `<données>/logs/`.

**Ne jamais** masquer les icônes du vrai Finder (mode géré), ni toucher au vrai `~/Desktop`, sans l'accord explicite du propriétaire. En cas de doute : `target/debug/tidy --restore-finder`.

## Architecture

Un exécutable `tidy`, plusieurs processus :
- `tidy` : **agent** (barre des menus, raccourcis, IPC, surveillance du bureau, Finder) ;
- `tidy --desktop` : surfaces posées sur le bureau (lancé et relancé par l'agent) ;
- `tidy --manager [page]` : fenêtre principale, à la demande ;
- `tidy --restore-finder`, `--quit`, `--version`.

Dialogue : JSON ligne par ligne sur le socket `tidy.sock` (`app/src/ipc.rs`). `Request::Subscribe` donne le flux des `tidy_core::events::Event`.

Crates (`crates/`) :
- `tidy-core` : modèle, règles, réglages, stockage versionné (écriture atomique, `.bad`, migrations), événements. **Aucune dépendance d'UI ni d'OS.**
- `tidy-ui` : thème (noir, violet `#7F3AED`, lime `#C6FF34`), polices Unbounded/Outfit, icônes Lucide, widgets partagés, traductions (`t!`).
- `tidy-platform` : trait `DesktopPlatform` (écrans, surfaces du bureau, clics traversants, ouverture), Finder, ouverture de session. **Seule crate où `unsafe` est permis.**
- `tidy-search`, `tidy-vault`, `tidy-wallpaper`, `tidy-widgets`, `tidy-integrations` : à venir (vagues 2 et 3).
- `app/` : le binaire (agent, bureau, fenêtre principale).

Points d'extension : `RuleMatcher` (core), `DesktopPlatform` (platform), puis `Widget`, `WallpaperSource`, `AiProvider`, `CalendarProvider`, `WeatherProvider`, `VaultBackend`.

## Conventions

- Code, commentaires (`//!`, `///`) et textes d'interface en **français**. Textes visibles via `t!("section.clé")` (`crates/tidy-ui/locales/fr.toml` et `en.toml`, mêmes clés, vérifié par test).
- Couleurs et polices uniquement depuis `tidy_ui::theme`, widgets depuis `tidy_ui::widgets`. Icônes : `tidy_ui::icons` (régénérées par `tools/subset_icons.py` ; pour en ajouter une, l'ajouter au script et le relancer avec le paquet `lucide-static` 1.77.0).
- `unsafe` seulement dans `tidy-platform`, avec `// SAFETY:` (clippy `undocumented_unsafe_blocks`).
- Nouveaux champs persistés : `#[serde(default)]` ; format incompatible : `Versioned::migrate_step`.
- Jamais de suppression de fichier utilisateur : corbeille uniquement (crate `trash`, à venir). Supprimer une boîte rend ses éléments au bureau.
- Commits petits, en français, Conventional Commits (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`), **sans mention de Claude** (pas de `Co-Authored-By`).
- Mesures de mémoire avec `footprint <pid>` (valeur d'Activity Monitor), en release.
