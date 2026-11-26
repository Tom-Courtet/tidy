//! Finition (lot K) : cartes de la page Options (disposition des boîtes,
//! installation, autorisations de macOS, à propos), proposition d'installation de
//! l'Accueil et de l'assistant.

use tidy_ui::{icons, t, widgets};

use super::pages::card_title;
use super::{Confirm, Manager};
use crate::install::{self, Situation};

/// Écart maximal entre boîtes aimantées, en points.
const MAX_SPACING: f32 = 40.0;

/// Volet des réglages de confidentialité de macOS (Fichiers et dossiers).
#[cfg(target_os = "macos")]
const PRIVACY_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders";

/// Licences des composants repris (polices, icônes, principales bibliothèques).
const THIRD_PARTY: [(&str, &str); 14] = [
    ("Outfit, Unbounded", "SIL OFL 1.1"),
    ("Lucide", "ISC"),
    ("DejaVu Sans", "Bitstream Vera / DejaVu"),
    ("egui, eframe", "MIT / Apache-2.0"),
    ("winit, tray-icon, global-hotkey", "MIT / Apache-2.0"),
    ("windows-rs, objc2", "MIT / Apache-2.0, MIT"),
    ("serde, anyhow, log, tracing", "MIT / Apache-2.0"),
    ("jiff, uuid", "MIT / Apache-2.0, Unlicense"),
    ("notify", "CC0-1.0 / Artistic-2.0"),
    ("trash, rfd", "MIT"),
    ("argon2, chacha20poly1305", "MIT / Apache-2.0"),
    ("image", "MIT / Apache-2.0"),
    ("ureq, keyring", "MIT / Apache-2.0"),
    ("nucleo-matcher", "MPL-2.0"),
];

/// Disposition des boîtes : aimantation, écart, taille en icônes, enroulement au bord.
pub fn box_layout(m: &mut Manager, ui: &mut egui::Ui) {
    widgets::card(ui, |ui| {
        card_title(ui, icons::MAGNET, t!("finish.layout_title"));
        let layout = &mut m.settings.layout;
        let mut snap = layout.snap;
        let mut changed = widgets::setting_row(
            ui,
            icons::MAGNET,
            t!("finish.snap"),
            Some(t!("finish.snap_hint")),
            |ui| widgets::toggle(ui, &mut snap).changed(),
        );
        widgets::row_separator(ui);
        let mut spacing = layout.spacing.clamp(0.0, MAX_SPACING);
        let slid = widgets::setting_row(
            ui,
            icons::CORNERS,
            t!("finish.spacing"),
            Some(t!("finish.spacing_hint")),
            |ui| {
                ui.add_enabled(
                    snap,
                    egui::Slider::new(&mut spacing, 0.0..=MAX_SPACING)
                        .step_by(1.0)
                        .suffix(" pt"),
                )
                .changed()
            },
        );
        widgets::row_separator(ui);
        let mut size_in_icons = layout.size_in_icons;
        changed |= widgets::setting_row(
            ui,
            icons::GRID,
            t!("finish.size_in_icons"),
            Some(t!("finish.size_in_icons_hint")),
            |ui| widgets::toggle(ui, &mut size_in_icons).changed(),
        );
        widgets::row_separator(ui);
        let mut roll_up = layout.roll_up_at_edge;
        changed |= widgets::setting_row(
            ui,
            icons::ROLL_UP,
            t!("finish.roll_up_at_edge"),
            Some(t!("finish.roll_up_at_edge_hint")),
            |ui| widgets::toggle(ui, &mut roll_up).changed(),
        );
        layout.snap = snap;
        layout.spacing = spacing;
        layout.size_in_icons = size_in_icons;
        layout.roll_up_at_edge = roll_up;
        if changed {
            m.save();
        } else if slid {
            m.save_later();
        }
    });
}

/// Lance l'installation (un autre processus : celui-ci sera fermé avec Tidy).
fn start_install(m: &mut Manager) {
    match install::spawn_self(&["--install"]) {
        Ok(()) => m.show_notice(t!("finish.installing")),
        Err(e) => m.error = Some(format!("{e:#}")),
    }
}

/// Texte et bouton d'une situation qui propose l'installation.
fn offer_texts(layout: &install::Layout, situation: &Situation) -> (String, &'static str) {
    let dir = layout.dir.display().to_string();
    match situation {
        Situation::Older { installed } => (
            t!(
                "finish.offer_update",
                installed = installed,
                current = install::VERSION
            ),
            t!("finish.update_button"),
        ),
        Situation::Same { .. } => (
            t!("finish.offer_reinstall", path = dir),
            t!("finish.reinstall_button"),
        ),
        _ => (
            t!("finish.offer_install", path = dir),
            t!("finish.install_button"),
        ),
    }
}

/// Proposition d'installation (Accueil, assistant) : seulement sous Windows, quand
/// cet exécutable n'est pas la copie installée.
pub fn install_offer(m: &mut Manager, ui: &mut egui::Ui) {
    let Some((layout, situation)) = install::cached_situation() else {
        return;
    };
    if !situation.offers_install() {
        return;
    }
    let (text, button) = offer_texts(&layout, &situation);
    widgets::card(ui, |ui| {
        card_title(ui, icons::DRIVE, t!("finish.install_title"));
        widgets::hint(ui, text);
        ui.add_space(6.0);
        if widgets::primary(ui, format!("{}  {button}", icons::DRIVE)).clicked() {
            start_install(m);
        }
    });
    ui.add_space(12.0);
}

/// Carte « Installation » des Options : état, installer ou mettre à jour
/// (Windows), désinstaller (tous les OS).
pub fn installation(m: &mut Manager, ui: &mut egui::Ui) {
    widgets::card(ui, |ui| {
        card_title(ui, icons::DRIVE, t!("finish.install_title"));
        if let Some((layout, situation)) = install::cached_situation() {
            let dir = layout.dir.display().to_string();
            match &situation {
                Situation::Installed => {
                    widgets::hint(
                        ui,
                        t!(
                            "finish.status_installed",
                            version = install::VERSION,
                            path = dir
                        ),
                    );
                }
                Situation::Newer { installed } => {
                    widgets::hint(
                        ui,
                        t!(
                            "finish.status_newer",
                            installed = installed,
                            current = install::VERSION,
                            path = dir
                        ),
                    );
                }
                offer => {
                    let (text, button) = offer_texts(&layout, offer);
                    widgets::hint(ui, text);
                    ui.add_space(6.0);
                    if widgets::primary(ui, format!("{}  {button}", icons::DRIVE)).clicked() {
                        start_install(m);
                    }
                }
            }
            widgets::row_separator(ui);
        } else if let Ok(exe) = std::env::current_exe() {
            widgets::hint(ui, t!("finish.status_running_from", path = exe.display()));
            widgets::row_separator(ui);
        }
        let clicked = widgets::setting_row(
            ui,
            icons::DELETE,
            t!("finish.uninstall"),
            Some(t!("finish.uninstall_hint")),
            |ui| ui.button(t!("finish.uninstall_button")).clicked(),
        );
        if clicked {
            m.confirm = Some(Confirm::Uninstall);
        }
    });
}

/// Désinstallation confirmée : lancée dans un autre processus (celui-ci sera fermé).
pub fn start_uninstall(m: &mut Manager) {
    match install::spawn_self(&["--uninstall"]) {
        Ok(()) => m.show_notice(t!("finish.uninstalling")),
        Err(e) => m.error = Some(format!("{e:#}")),
    }
}

/// Autorisations de macOS : ce que Tidy demande et où les gérer. Rien ailleurs.
pub fn permissions(m: &mut Manager, ui: &mut egui::Ui) {
    #[cfg(target_os = "macos")]
    {
        widgets::card(ui, |ui| {
            card_title(ui, icons::SHIELD, t!("finish.permissions_title"));
            widgets::hint(ui, t!("finish.permissions_intro"));
            ui.add_space(6.0);
            for (icon, text) in [
                (icons::FOLDER, t!("finish.permission_folders")),
                (icons::BELL, t!("finish.permission_notifications")),
                (icons::KEYBOARD, t!("finish.permission_none")),
            ] {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icon).color(tidy_ui::theme::ACCENT_2));
                    ui.add(egui::Label::new(text).wrap());
                });
            }
            ui.add_space(6.0);
            if ui
                .button(format!(
                    "{}  {}",
                    icons::SETTINGS,
                    t!("finish.open_privacy")
                ))
                .clicked()
                && let Err(e) = std::process::Command::new("/usr/bin/open")
                    .arg(PRIVACY_URL)
                    .spawn()
            {
                m.error = Some(format!("{e:#}"));
            }
        });
        ui.add_space(12.0);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (m, ui);
}

/// À propos : version, dossiers des données et des journaux, licences.
pub fn about(m: &mut Manager, ui: &mut egui::Ui) {
    let root = m.store.root().to_owned();
    let logs = m.store.logs_dir();
    widgets::card(ui, |ui| {
        card_title(ui, icons::INFO, t!("finish.about_title"));
        widgets::hint(ui, t!("finish.about_intro"));
        ui.add_space(6.0);
        egui::Grid::new("à-propos")
            .num_columns(2)
            .spacing([24.0, 8.0])
            .show(ui, |ui| {
                ui.label(t!("diagnostic.version"));
                ui.monospace(install::VERSION);
                ui.end_row();
                ui.label(t!("diagnostic.data_dir"));
                path_label(ui, &root);
                ui.end_row();
                ui.label(t!("diagnostic.logs_dir"));
                path_label(ui, &logs);
                ui.end_row();
                ui.label(t!("finish.license"));
                ui.label("MIT");
                ui.end_row();
            });
        ui.add_space(6.0);
        widgets::hint(ui, t!("finish.data_hint"));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let platform = tidy_platform::current();
            if ui
                .button(format!(
                    "{}  {}",
                    icons::FOLDER_OPEN,
                    t!("diagnostic.open_data")
                ))
                .clicked()
                && let Err(e) = platform.open(&root)
            {
                m.error = Some(format!("{e:#}"));
            }
            if ui
                .button(format!(
                    "{}  {}",
                    icons::DOCUMENT,
                    t!("diagnostic.open_logs")
                ))
                .clicked()
            {
                let _ = std::fs::create_dir_all(&logs);
                if let Err(e) = platform.open(&logs) {
                    m.error = Some(format!("{e:#}"));
                }
            }
        });
        ui.add_space(6.0);
        egui::CollapsingHeader::new(t!("finish.third_party"))
            .id_salt("licences-tierces")
            .show(ui, |ui| {
                egui::Grid::new("licences")
                    .num_columns(2)
                    .spacing([24.0, 4.0])
                    .show(ui, |ui| {
                        for (name, license) in THIRD_PARTY {
                            ui.label(name);
                            ui.label(widgets::dim(license));
                            ui.end_row();
                        }
                    });
                ui.add_space(4.0);
                widgets::hint(ui, t!("finish.third_party_hint"));
            });
    });
}

/// Chemin en police fixe, coupé s'il est trop long (entier au survol).
fn path_label(ui: &mut egui::Ui, path: &std::path::Path) {
    let text = path.display().to_string();
    ui.add(egui::Label::new(egui::RichText::new(&text).monospace()).truncate())
        .on_hover_text(text);
}
