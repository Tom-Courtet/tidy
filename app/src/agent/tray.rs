//! Icône de Tidy dans la barre des menus et son menu.
//!
//! Vit sur le thread principal. Les choix du menu repartent vers la boucle de l'agent.

use anyhow::{Context, Result};
use tidy_ui::t;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Ce qu'affiche le menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayView {
    pub boxes_enabled: bool,
    pub desktop_hidden: bool,
}

/// Choix fait dans le menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuChoice {
    OpenManager,
    ToggleBoxes,
    ToggleDesktop,
    Quit,
}

const OPEN_ID: &str = "open";
const BOXES_ID: &str = "boxes";
const DESKTOP_ID: &str = "desktop";
const QUIT_ID: &str = "quit";

fn parse_menu_id(id: &str) -> Option<MenuChoice> {
    match id {
        OPEN_ID => Some(MenuChoice::OpenManager),
        BOXES_ID => Some(MenuChoice::ToggleBoxes),
        DESKTOP_ID => Some(MenuChoice::ToggleDesktop),
        QUIT_ID => Some(MenuChoice::Quit),
        _ => None,
    }
}

pub struct Tray {
    icon: TrayIcon,
    shown: Option<TrayView>,
}

impl Tray {
    pub fn new(on_choice: impl Fn(MenuChoice) + Send + Sync + 'static) -> Result<Self> {
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some(choice) = parse_menu_id(&event.id().0) {
                on_choice(choice);
            }
        }));
        let builder = TrayIconBuilder::new();
        // macOS : icône « modèle », adaptée au thème de la barre des menus. Ailleurs :
        // le logo en couleur.
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(template_icon(44)?);
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(
            Icon::from_rgba(
                include_bytes!("../../../assets/icon_64.rgba").to_vec(),
                64,
                64,
            )
            .context("icône de la zone de notification")?,
        );
        let icon = builder
            .with_tooltip(t!("tray.tooltip"))
            .with_menu_on_left_click(true)
            .build()
            .context("icône de la barre des menus")?;
        Ok(Self { icon, shown: None })
    }

    pub fn show(&mut self, view: TrayView) {
        if self.shown == Some(view) {
            return;
        }
        if let Err(e) = self.rebuild_menu(view) {
            log::warn!("menu de la barre des menus : {e:#}");
        }
        self.shown = Some(view);
    }

    fn rebuild_menu(&mut self, view: TrayView) -> Result<()> {
        let menu = Menu::new();
        let desktop_label = if view.desktop_hidden {
            t!("tray.show_desktop")
        } else {
            t!("tray.hide_desktop")
        };
        menu.append_items(&[
            &MenuItem::with_id(OPEN_ID, t!("tray.open"), true, None),
            &PredefinedMenuItem::separator(),
            &CheckMenuItem::with_id(
                BOXES_ID,
                t!("tray.boxes_enabled"),
                true,
                view.boxes_enabled,
                None,
            ),
            &MenuItem::with_id(DESKTOP_ID, desktop_label, view.boxes_enabled, None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(QUIT_ID, t!("tray.quit"), true, None),
        ])?;
        self.icon.set_menu(Some(Box::new(menu)));
        Ok(())
    }
}

/// Icône « modèle » (noir et transparence, macOS l'adapte au thème) : le logo de Tidy
/// en silhouette, sur une grille de 48 ramenée à `size` pixels.
#[cfg(target_os = "macos")]
fn template_icon(size: u32) -> Result<Icon> {
    Icon::from_rgba(template_rgba(size), size, size).context("icône de la barre des menus")
}

/// Pixels RGBA de l'icône modèle.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn template_rgba(size: u32) -> Vec<u8> {
    let unit = size as f32 / 48.0;
    // Dans la grille de 48, avec une marge de 4 : contour de la boîte, barre de titre
    // pleine, quatre icônes pleines.
    let inside = |x: f32, y: f32, (x0, y0, x1, y1, r): (f32, f32, f32, f32, f32)| {
        if x < x0 || x > x1 || y < y0 || y > y1 {
            return false;
        }
        let r = r.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
        let cx = x.clamp(x0 + r, x1 - r);
        let cy = y.clamp(y0 + r, y1 - r);
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r
    };
    let outer = (4.0, 6.0, 44.0, 42.0, 7.0);
    let inner = (7.5, 9.5, 40.5, 38.5, 4.0);
    let title = (4.0, 6.0, 44.0, 15.0, 7.0);
    let icons = [
        (12.0, 19.0, 21.0, 26.0, 2.0),
        (27.0, 19.0, 36.0, 26.0, 2.0),
        (12.0, 30.0, 21.0, 37.0, 2.0),
        (27.0, 30.0, 36.0, 37.0, 2.0),
    ];
    const SUPERSAMPLE: u32 = 4;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let mut hits = 0u32;
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let x = (px as f32 + (sx as f32 + 0.5) / SUPERSAMPLE as f32) / unit;
                    let y = (py as f32 + (sy as f32 + 0.5) / SUPERSAMPLE as f32) / unit;
                    let ring = inside(x, y, outer) && !inside(x, y, inner);
                    let filled = inside(x, y, title) || icons.iter().any(|&r| inside(x, y, r));
                    if ring || filled {
                        hits += 1;
                    }
                }
            }
            let alpha = (hits * 255 / (SUPERSAMPLE * SUPERSAMPLE)) as u8;
            rgba.extend_from_slice(&[0, 0, 0, alpha]);
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_round_trip() {
        for (id, choice) in [
            (OPEN_ID, MenuChoice::OpenManager),
            (BOXES_ID, MenuChoice::ToggleBoxes),
            (DESKTOP_ID, MenuChoice::ToggleDesktop),
            (QUIT_ID, MenuChoice::Quit),
        ] {
            assert_eq!(parse_menu_id(id), Some(choice));
        }
        assert_eq!(parse_menu_id("autre"), None);
    }

    #[test]
    fn template_icon_has_ink_and_holes() {
        let rgba = template_rgba(44);
        assert_eq!(rgba.len(), 44 * 44 * 4);
        let alpha = |x: u32, y: u32| rgba[((y * 44 + x) * 4 + 3) as usize];
        // Coin de l'image : transparent ; barre de titre : pleine ; entre deux icônes :
        // transparent.
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(22, 10), 255);
        assert_eq!(alpha(22, 26), 0);
    }
}
