// Windows, en release : pas de fenêtre console à côté de l'app.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! Tidy : organiseur de bureau léger et personnalisable.
//!
//! Un seul exécutable, plusieurs rôles (un processus chacun) :
//! - sans argument : l'**agent**, dans la barre des menus. Il tient les raccourcis
//!   globaux, l'état du bureau et surveille le processus du bureau ;
//! - `--desktop` : les surfaces posées sur le bureau (boîtes, icônes, widgets), lancé
//!   et surveillé par l'agent ;
//! - `--manager` : la fenêtre principale (réglages), à la demande ;
//! - `--create-box` : crée une boîte sous la souris (menu contextuel du bureau) ;
//! - `--restore-desktop` (ou `--restore-finder`) : rend les icônes du bureau ;
//! - `--install` : installe Tidy pour cet utilisateur (Windows) ;
//! - `--uninstall` : retire tout ce que Tidy a posé hors de son dossier de données.

mod agent;
mod desktop;
mod icons;
mod install;
mod ipc;
mod lang;
mod logging;
mod manager;
mod search;
mod wallpaper;

use anyhow::Result;
use ipc::Request;
use tidy_core::Store;

const USAGE: &str = "\
Tidy : organiseur de bureau léger et personnalisable.

Usage :
  tidy                    lance l'agent (barre des menus)
  tidy --background       lance l'agent sans ouvrir la fenêtre principale
  tidy --manager [page]   ouvre la fenêtre principale (page : options, diagnostic…)
  tidy --desktop          surfaces du bureau (lancé par l'agent)
  tidy --search [texte]   lanceur de recherche rapide
  tidy --wallpaper        fonds d'écran animés (lancé par l'agent)
  tidy --create-box       crée une boîte sous la souris (menu du bureau)
  tidy --toggle-desktop   masque ou montre le bureau (boîtes et icônes)
  tidy --organize         range tout le bureau selon les règles (démarrage rapide si
                          aucune boîte) ; --organize-new : seulement les non affectés
  tidy --undo-organize    annule le dernier rangement (« Tout annuler »)
  tidy --restore-desktop  rend les icônes du bureau si Tidy les a masquées
  tidy --desktop-icons    liste les icônes du bureau et leur position (Windows)
  tidy --hide-icon <nom>  essai : masque l'icône d'un élément du bureau (Windows)
  tidy --install [--force]
                          installe Tidy dans %LOCALAPPDATA%\\Programs\\Tidy (Windows),
                          ou le met à jour ; --force : même sur une version plus récente
  tidy --uninstall        retire Tidy du système (processus, icônes, fond d'écran,
                          registre, menu Démarrer) ; les données restent
  tidy --quit             arrête l'agent
  tidy --version

Données jetables pour essayer : TIDY_DATA_DIR=/tmp/tidy-essai tidy";

/// Rôle de ce processus, d'après les arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Agent { background: bool },
    Desktop,
    Manager,
    Search,
    Wallpaper,
    RestoreFinder,
    CreateBox,
    ToggleDesktop,
    Organize { all: bool },
    UndoOrganize,
    ListIcons,
    HideIcon,
    Install,
    Uninstall,
    Quit,
    Version,
    Help,
}

fn parse_role(arg: Option<&str>) -> Option<Role> {
    Some(match arg {
        None => Role::Agent { background: false },
        Some("--background") => Role::Agent { background: true },
        Some("--desktop") => Role::Desktop,
        Some("--manager") => Role::Manager,
        Some("--search") => Role::Search,
        Some("--wallpaper") => Role::Wallpaper,
        Some("--restore-finder" | "--restore-desktop") => Role::RestoreFinder,
        Some("--create-box") => Role::CreateBox,
        Some("--toggle-desktop") => Role::ToggleDesktop,
        Some("--organize") => Role::Organize { all: true },
        Some("--organize-new") => Role::Organize { all: false },
        Some("--undo-organize") => Role::UndoOrganize,
        Some("--desktop-icons") => Role::ListIcons,
        Some("--hide-icon") => Role::HideIcon,
        Some("--install") => Role::Install,
        Some("--uninstall") => Role::Uninstall,
        Some("--quit") => Role::Quit,
        Some("--version" | "-V") => Role::Version,
        Some("--help" | "-h") => Role::Help,
        Some(_) => return None,
    })
}

fn main() -> Result<()> {
    let arg = std::env::args().nth(1);
    let Some(role) = parse_role(arg.as_deref()) else {
        eprintln!("argument inconnu : {}\n\n{USAGE}", arg.unwrap_or_default());
        std::process::exit(2);
    };
    let store = Store::open_default()?;
    let _log_guard = logging::init(&store, role_name(role));
    match role {
        Role::Agent { background } => {
            // Une seule instance : le verrou `agent.lock`. Sans lui, un autre agent
            // tourne (même s'il ne répond pas encore).
            let Some(lock) = ipc::lock_instance(&store)? else {
                // Relancé (Finder, Launchpad) alors qu'il tourne : on montre la fenêtre
                // principale, sauf au démarrage de session.
                // Windows : un exécutable téléchargé, alors qu'une copie installée
                // tourne, ouvre sa propre fenêtre sur les Options, où la mise à jour
                // est proposée.
                let offer = install::current_situation().is_some_and(|(_, s)| {
                    matches!(
                        s,
                        install::Situation::Older { .. } | install::Situation::Same { .. }
                    )
                });
                if offer && !background {
                    install::spawn_self(&["--manager", "options"])?;
                } else if !background {
                    let _ = ipc::request(&store, &Request::OpenManager { page: None });
                }
                println!("Tidy tourne déjà (icône dans la barre des menus).");
                return Ok(());
            };
            agent::run(store, lock, !background)
        }
        Role::Desktop => desktop::run(store),
        Role::Search => search::run(store, std::env::args().nth(2)),
        Role::Wallpaper => wallpaper::run(store),
        Role::Manager => {
            let page = std::env::args().nth(2);
            let page = page.as_deref().and_then(manager::Page::from_arg);
            manager::run(store, page)
        }
        Role::RestoreFinder => {
            let marker = store.finder_marker_path();
            let finder = tidy_platform::finder::restore_icons(
                &tidy_platform::finder::SystemFinder,
                &marker,
            )?;
            let platform = tidy_platform::current();
            let explorer = platform.desktop_icons_hidden();
            if explorer {
                platform.set_desktop_icons_hidden(false)?;
            }
            let stashed = icons::restore_all(&store);
            if stashed > 0 {
                println!("{stashed} icône(s) rangée(s) remise(s) à leur place.");
            }
            // Icônes encore hors de l'écran sans entrée au journal (Windows).
            let rescued = icons::rescue_offscreen(&store);
            if rescued > 0 {
                println!("{rescued} icône(s) hors de l'écran ramenée(s) sur le bureau.");
            }
            if finder || explorer || stashed > 0 || rescued > 0 {
                println!("Icônes du bureau rétablies.");
            } else {
                println!("Tidy n'avait pas masqué les icônes du bureau : rien à faire.");
            }
            Ok(())
        }
        Role::Organize { all } => agent_request(&store, &Request::ApplyRules { all }),
        Role::UndoOrganize => agent_request(&store, &Request::UndoOrganize),
        Role::ToggleDesktop => match ipc::request(&store, &Request::ToggleDesktop) {
            Ok(_) => Ok(()),
            Err(_) => {
                eprintln!("Tidy ne tourne pas.");
                std::process::exit(1);
            }
        },
        Role::CreateBox => match ipc::request(&store, &Request::CreateBox) {
            Ok(_) => Ok(()),
            Err(_) => {
                eprintln!("Tidy ne tourne pas : lance-le d'abord.");
                std::process::exit(1);
            }
        },
        Role::ListIcons => {
            let Some(list) = tidy_platform::current().desktop_icon_positions() else {
                println!("Positions des icônes du bureau : pas disponible sur cet OS.");
                return Ok(());
            };
            for (path, pos) in list.positions()? {
                let state = if pos.is_offscreen() {
                    "  (masquée par Tidy)"
                } else {
                    ""
                };
                println!("{:>7} {:>7}  {}{state}", pos.x, pos.y, path.display());
            }
            Ok(())
        }
        Role::HideIcon => {
            // Essai de la piste 1 : range virtuellement un élément du bureau.
            let Some(name) = std::env::args().nth(2) else {
                eprintln!("usage : tidy --hide-icon <nom d'un élément du bureau>");
                std::process::exit(2);
            };
            let dirs = tidy_core::store::desktop_dirs();
            let Some(path) = tidy_core::store::resolve_desktop_item(&dirs, &name) else {
                anyhow::bail!("dossier du bureau introuvable");
            };
            match icons::sync(&store, &[path]) {
                Some(report) => {
                    let report = report?;
                    println!(
                        "{} icône(s) masquée(s) sur {} icône(s) du bureau. `tidy --restore-desktop` les remet à leur place.",
                        report.hidden, report.on_desktop
                    );
                    for path in &report.not_found {
                        println!("absente du bureau de l'Explorateur : {}", path.display());
                    }
                    for path in &report.stuck {
                        println!("restée à l'écran (déplacement refusé) : {}", path.display());
                    }
                }
                None => println!("Pas disponible sur cet OS."),
            }
            Ok(())
        }
        Role::Install => {
            lang::apply(store.load::<tidy_core::Settings>().language);
            let force = std::env::args().nth(2).as_deref() == Some("--force");
            let title = tidy_ui::t!("finish.install_title");
            match install::install(&store, force) {
                Ok(dir) => {
                    let text = tidy_ui::t!("finish.installed", path = dir.display());
                    println!("{text}");
                    tidy_platform::install::message_box(title, &text, false);
                    Ok(())
                }
                Err(e) => {
                    let text = format!("{}\n\n{e:#}", tidy_ui::t!("finish.install_failed"));
                    log::error!("installation : {e:#}");
                    eprintln!("{text}");
                    tidy_platform::install::message_box(title, &text, true);
                    std::process::exit(1);
                }
            }
        }
        Role::Uninstall => {
            lang::apply(store.load::<tidy_core::Settings>().language);
            let report = install::uninstall(&store);
            let text = report.text(store.root());
            log::info!("désinstallation : {text}");
            println!("{text}");
            tidy_platform::install::message_box(
                tidy_ui::t!("finish.uninstall_title"),
                &text,
                !report.warnings.is_empty(),
            );
            Ok(())
        }
        Role::Quit => {
            match ipc::request(&store, &Request::Quit) {
                Ok(_) => println!("Tidy s'arrête."),
                Err(_) => println!("Tidy ne tourne pas."),
            }
            Ok(())
        }
        Role::Version => {
            println!("tidy {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Role::Help => {
            println!("{USAGE}");
            Ok(())
        }
    }
}

/// Requête à l'agent depuis la ligne de commande : message clair en cas d'erreur.
fn agent_request(store: &Store, req: &Request) -> Result<()> {
    match ipc::request(store, req) {
        Ok(ipc::Response::Error { message }) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
        Ok(_) => Ok(()),
        Err(_) => {
            eprintln!("Tidy ne tourne pas : lance-le d'abord.");
            std::process::exit(1);
        }
    }
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Agent { .. } => "agent",
        Role::Desktop => "desktop",
        Role::Manager => "manager",
        Role::Search => "search",
        Role::Wallpaper => "wallpaper",
        _ => "cli",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_from_arguments() {
        assert_eq!(parse_role(None), Some(Role::Agent { background: false }));
        assert_eq!(
            parse_role(Some("--background")),
            Some(Role::Agent { background: true })
        );
        assert_eq!(parse_role(Some("--desktop")), Some(Role::Desktop));
        assert_eq!(
            parse_role(Some("--restore-finder")),
            Some(Role::RestoreFinder)
        );
        assert_eq!(parse_role(Some("--install")), Some(Role::Install));
        assert_eq!(parse_role(Some("--uninstall")), Some(Role::Uninstall));
        assert_eq!(parse_role(Some("--nimporte")), None);
    }
}
