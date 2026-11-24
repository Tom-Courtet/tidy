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
//! - `--restore-finder` : rend les icônes du Finder si Tidy les a masquées.

mod agent;
mod desktop;
mod ipc;
mod lang;
mod logging;
mod manager;

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
  tidy --restore-finder   rend les icônes du Finder si Tidy les a masquées
  tidy --quit             arrête l'agent
  tidy --version

Données jetables pour essayer : TIDY_DATA_DIR=/tmp/tidy-essai tidy";

/// Rôle de ce processus, d'après les arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Agent { background: bool },
    Desktop,
    Manager,
    RestoreFinder,
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
        Some("--restore-finder") => Role::RestoreFinder,
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
            if ipc::request(&store, &Request::Ping).is_ok() {
                // Relancé (Finder, Launchpad) alors qu'il tourne : on montre la fenêtre
                // principale, sauf au démarrage de session.
                if !background {
                    let _ = ipc::request(&store, &Request::OpenManager);
                }
                println!("Tidy tourne déjà (icône dans la barre des menus).");
                return Ok(());
            }
            agent::run(store, !background)
        }
        Role::Desktop => desktop::run(store),
        Role::Manager => {
            let page = std::env::args().nth(2);
            let page = page.as_deref().and_then(manager::Page::from_arg);
            manager::run(store, page)
        }
        Role::RestoreFinder => {
            let marker = store.finder_marker_path();
            if tidy_platform::finder::restore_icons(&tidy_platform::finder::SystemFinder, &marker)?
            {
                println!("Icônes du Finder rétablies.");
            } else {
                println!("Tidy n'avait pas masqué les icônes du Finder : rien à faire.");
            }
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

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Agent { .. } => "agent",
        Role::Desktop => "desktop",
        Role::Manager => "manager",
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
        assert_eq!(parse_role(Some("--nimporte")), None);
    }
}
