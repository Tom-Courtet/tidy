//! Page Recherche de la fenêtre principale (lot G) : raccourci et comportement du
//! lanceur, dossiers indexés, exclusions, onglets affichés, index et récents.
//!
//! Les réglages vont dans `config.json` ([`Manager::save`]) : l'agent prévient le
//! lanceur, qui réindexe si les dossiers ou les exclusions ont changé.

use serde_json::Value;
use tidy_search::{BUILTIN_EXCLUSIONS, DEFAULT_KEYS, Recents, Tab, default_folders};
use tidy_ui::{icons, t, widgets};

use super::Manager;
use super::pages::{card_title, page_header};
use super::shortcuts::{self, Slot};
use crate::ipc::Request;

pub fn show(m: &mut Manager, ui: &mut egui::Ui) {
    page_header(
        ui,
        icons::SEARCH,
        t!("search.page_title"),
        t!("search.page_intro"),
    );
    widgets::card(ui, |ui| launcher(m, ui));
    ui.add_space(12.0);
    widgets::card(ui, |ui| folders(m, ui));
    ui.add_space(12.0);
    widgets::card(ui, |ui| exclusions(m, ui));
    ui.add_space(12.0);
    widgets::card(ui, |ui| tabs(m, ui));
    ui.add_space(12.0);
    widgets::card(ui, |ui| index(m, ui));
}

/// Ligne avec un interrupteur ; enregistre au changement.
fn toggle_row(
    m: &mut Manager,
    ui: &mut egui::Ui,
    icon: &str,
    title: &str,
    hint: &str,
    value: impl Fn(&mut Manager) -> &mut bool,
) {
    let mut on = *value(m);
    let changed = widgets::setting_row(ui, icon, title, Some(hint), |ui| {
        widgets::toggle(ui, &mut on).changed()
    });
    if changed {
        *value(m) = on;
        m.save();
    }
}

fn launcher(m: &mut Manager, ui: &mut egui::Ui) {
    card_title(ui, icons::KEYBOARD, t!("search.page_launcher"));
    shortcuts::row(m, ui, Slot::Search);
    widgets::row_separator(ui);
    toggle_row(
        m,
        ui,
        icons::ZAP,
        t!("search.keep_in_background"),
        t!("search.keep_in_background_hint"),
        |m| &mut m.settings.search.keep_in_background,
    );
    widgets::row_separator(ui);
    toggle_row(
        m,
        ui,
        icons::CLOCK,
        t!("search.show_recents"),
        t!("search.show_recents_hint"),
        |m| &mut m.settings.search.show_recents,
    );
    if tidy_search::spotlight::available() {
        widgets::row_separator(ui);
        toggle_row(
            m,
            ui,
            icons::GLOBE,
            t!("search.more_results_setting"),
            t!("search.more_results_hint"),
            |m| &mut m.settings.search.more_results,
        );
    }
}

fn folder_label(key: &str) -> &'static str {
    match key {
        "desktop" => t!("search.folder_desktop"),
        "documents" => t!("search.folder_documents"),
        "downloads" => t!("search.folder_downloads"),
        "pictures" => t!("search.folder_pictures"),
        "music" => t!("search.folder_music"),
        "videos" => t!("search.folder_videos"),
        _ if cfg!(target_os = "macos") => t!("search.folder_programs_mac"),
        _ => t!("search.folder_programs"),
    }
}

fn folder_icon(key: &str) -> &'static str {
    match key {
        "desktop" => icons::MONITOR,
        "documents" => icons::DOCUMENT,
        "downloads" => icons::INBOX,
        "pictures" => icons::IMAGE,
        "music" => icons::MUSIC,
        "videos" => icons::VIDEO,
        _ => icons::APP,
    }
}

fn folders(m: &mut Manager, ui: &mut egui::Ui) {
    card_title(ui, icons::FOLDER_TREE, t!("search.folders"));
    widgets::hint(ui, t!("search.folders_hint"));
    ui.add_space(8.0);
    for (i, key) in DEFAULT_KEYS.into_iter().enumerate() {
        if i > 0 {
            widgets::row_separator(ui);
        }
        let paths = default_folders(key);
        let hint = if paths.is_empty() {
            t!("search.folder_missing").to_owned()
        } else {
            paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(" · ")
        };
        let mut on = !m.settings.search.disabled_defaults.iter().any(|k| k == key);
        let changed =
            widgets::setting_row(ui, folder_icon(key), folder_label(key), Some(&hint), |ui| {
                ui.add_enabled_ui(!paths.is_empty(), |ui| {
                    widgets::toggle(ui, &mut on).changed()
                })
                .inner
            });
        if changed {
            let disabled = &mut m.settings.search.disabled_defaults;
            disabled.retain(|k| k != key);
            if !on {
                disabled.push(key.to_owned());
            }
            m.save();
        }
    }
    let mut remove = None;
    for (i, path) in m.settings.search.folders.iter().enumerate() {
        widgets::row_separator(ui);
        let title = path.display().to_string();
        let hint = (!path.is_dir()).then(|| t!("search.folder_missing"));
        let clicked = widgets::setting_row(ui, icons::FOLDER, &title, hint, |ui| {
            widgets::icon_button(ui, icons::DELETE, t!("search.remove_folder")).clicked()
        });
        if clicked {
            remove = Some(i);
        }
    }
    if let Some(i) = remove {
        m.settings.search.folders.remove(i);
        m.save();
    }
    ui.add_space(10.0);
    if ui
        .button(format!(
            "{}  {}",
            icons::FOLDER_PLUS,
            t!("search.add_folder")
        ))
        .clicked()
        && let Some(path) = rfd::FileDialog::new().pick_folder()
    {
        let known = m.settings.search.folders.contains(&path)
            || DEFAULT_KEYS
                .iter()
                .filter(|k| !m.settings.search.disabled_defaults.iter().any(|d| d == *k))
                .any(|k| default_folders(k).iter().any(|d| path.starts_with(d)));
        if known {
            m.error = Some(t!("search.folder_already").into());
        } else {
            m.settings.search.folders.push(path);
            m.save();
        }
    }
}

fn exclusions(m: &mut Manager, ui: &mut egui::Ui) {
    card_title(ui, icons::FILTER, t!("search.exclusions"));
    widgets::hint(
        ui,
        t!(
            "search.exclusions_hint",
            list = BUILTIN_EXCLUSIONS.join(", ")
        ),
    );
    ui.add_space(8.0);
    let mut remove = None;
    if !m.settings.search.exclusions.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for (i, pattern) in m.settings.search.exclusions.iter().enumerate() {
                if widgets::chip(ui, pattern) {
                    remove = Some(i);
                }
            }
        });
        ui.add_space(8.0);
    }
    if let Some(i) = remove {
        m.settings.search.exclusions.remove(i);
        m.save();
    }
    let id = egui::Id::unique("nouvelle-exclusion");
    let mut draft: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    let mut add = false;
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut draft)
                .hint_text(t!("search.exclusion_placeholder"))
                .desired_width(320.0),
        );
        add |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        add |= ui.button(t!("search.exclusion_add")).clicked();
    });
    let pattern = draft.trim().to_owned();
    if add && !pattern.is_empty() {
        if !m.settings.search.exclusions.contains(&pattern) {
            m.settings.search.exclusions.push(pattern);
            m.save();
        }
        draft.clear();
    }
    ui.data_mut(|d| d.insert_temp(id, draft));
    ui.add_space(10.0);
    toggle_row(
        m,
        ui,
        icons::EYE,
        t!("search.index_hidden"),
        t!("search.index_hidden_hint"),
        |m| &mut m.settings.search.index_hidden,
    );
}

fn tabs(m: &mut Manager, ui: &mut egui::Ui) {
    card_title(ui, icons::TABS, t!("search.tabs"));
    widgets::hint(ui, t!("search.tabs_hint"));
    ui.add_space(8.0);
    let mut toggled = None;
    ui.horizontal_wrapped(|ui| {
        for tab in Tab::ALL.into_iter().filter(|t| *t != Tab::All) {
            let shown = !m.settings.search.hidden_tabs.iter().any(|h| h == tab.key());
            if widgets::choice(ui, shown, crate::search::tab_label(tab)).clicked() {
                toggled = Some(tab);
            }
        }
    });
    if let Some(tab) = toggled {
        let hidden = &mut m.settings.search.hidden_tabs;
        if hidden.iter().any(|h| h == tab.key()) {
            hidden.retain(|h| h != tab.key());
        } else {
            hidden.push(tab.key().to_owned());
        }
        m.save();
    }
}

fn index(m: &mut Manager, ui: &mut egui::Ui) {
    card_title(ui, icons::REFRESH, t!("search.index"));
    widgets::hint(ui, t!("search.index_hint"));
    if let Ok(meta) = std::fs::metadata(tidy_search::cache_path(m.store.root())) {
        let mb = meta.len() as f64 / 1_048_576.0;
        ui.label(widgets::dim(t!(
            "search.index_size",
            size = format!("{mb:.1}")
        )));
    }
    let recents = m.store.load::<Recents>().items.len();
    ui.label(widgets::dim(t!("search.recents_count", count = recents)));
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if widgets::primary(ui, format!("{}  {}", icons::REFRESH, t!("search.rebuild"))).clicked()
            && m.request(module_request("reindex"))
        {
            m.show_notice(t!("search.rebuild_done"));
        }
        let clear = ui.add_enabled(
            recents > 0,
            egui::Button::new(format!("{}  {}", icons::DELETE, t!("search.clear_recents"))),
        );
        if clear.clicked() {
            match m.store.save(&Recents::default()) {
                Ok(()) => {
                    // Lanceur fermé : rien à prévenir, l'erreur éventuelle est sans gravité.
                    let _ = crate::ipc::request(&m.store, &module_request("recents_cleared"));
                    m.show_notice(t!("search.recents_cleared"));
                }
                Err(e) => m.error = Some(format!("{e:#}")),
            }
        }
    });
}

fn module_request(action: &str) -> Request {
    Request::Module {
        module: "search".into(),
        action: action.into(),
        data: Value::Null,
    }
}
