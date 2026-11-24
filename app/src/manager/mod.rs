//! Fenêtre principale (`tidy --manager`) : navigation latérale et pages de réglages,
//! dans la DA de Tappy (en-tête avec logo, cartes, choix lime).
//!
//! Vague 0 : Accueil, Options (langue, ouverture de session, mode du bureau avec
//! accord explicite pour le mode géré) et Diagnostic. Les autres pages sont posées et
//! annoncées ; elles se remplissent dans les vagues suivantes.
//!
//! Processus à part : sa mémoire est rendue au système dès qu'on ferme la fenêtre.

mod pages;

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use tidy_core::events::Event;
use tidy_core::{Settings, Store};
use tidy_ui::{icons, t, theme, widgets};

use crate::ipc::{self, Request, Response, Status};

/// Pages de la fenêtre, dans l'ordre de la navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Boxes,
    Organization,
    QuickActions,
    Layouts,
    Wallpapers,
    Widgets,
    Search,
    Options,
    Diagnostic,
}

impl Page {
    const ALL: [Page; 10] = [
        Page::Home,
        Page::Boxes,
        Page::Organization,
        Page::QuickActions,
        Page::Layouts,
        Page::Wallpapers,
        Page::Widgets,
        Page::Search,
        Page::Options,
        Page::Diagnostic,
    ];

    /// Page d'après son nom en argument (`tidy --manager options`).
    pub fn from_arg(arg: &str) -> Option<Page> {
        Some(match arg {
            "home" => Page::Home,
            "boxes" => Page::Boxes,
            "organization" => Page::Organization,
            "quick-actions" => Page::QuickActions,
            "layouts" => Page::Layouts,
            "wallpapers" => Page::Wallpapers,
            "widgets" => Page::Widgets,
            "search" => Page::Search,
            "options" => Page::Options,
            "diagnostic" => Page::Diagnostic,
            _ => return None,
        })
    }

    fn icon(self) -> &'static str {
        match self {
            Page::Home => icons::HOME,
            Page::Boxes => icons::BOX,
            Page::Organization => icons::AUTO_TIDY,
            Page::QuickActions => icons::QUICK_ACTIONS,
            Page::Layouts => icons::SNAPSHOT,
            Page::Wallpapers => icons::WALLPAPER,
            Page::Widgets => icons::WIDGETS,
            Page::Search => icons::SEARCH,
            Page::Options => icons::OPTIONS,
            Page::Diagnostic => icons::DIAGNOSTIC,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Page::Home => t!("manager.home"),
            Page::Boxes => t!("manager.boxes"),
            Page::Organization => t!("manager.organization"),
            Page::QuickActions => t!("manager.quick_actions"),
            Page::Layouts => t!("manager.layouts"),
            Page::Wallpapers => t!("manager.wallpapers"),
            Page::Widgets => t!("manager.widgets"),
            Page::Search => t!("manager.search"),
            Page::Options => t!("manager.options"),
            Page::Diagnostic => t!("manager.diagnostic"),
        }
    }
}

pub fn run(store: Store, page: Option<Page>) -> Result<()> {
    let settings: Settings = store.load();
    crate::lang::apply(settings.language);

    let (tx, events) = mpsc::channel();
    let ctx_cell: Arc<OnceLock<egui::Context>> = Arc::default();
    let wake = Arc::clone(&ctx_cell);
    if let Err(e) = ipc::subscribe(
        &store,
        move |event| {
            let _ = tx.send(event);
            if let Some(ctx) = wake.get() {
                ctx.request_repaint();
            }
        },
        || {},
    ) {
        log::warn!("pas d'agent ({e:#}) : fenêtre principale seule");
    }

    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Tidy")
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([720.0, 480.0]),
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "Tidy",
        native,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            let _ = ctx_cell.set(cc.egui_ctx.clone());
            Ok(Box::new(Manager::new(store, settings, events, page)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("fenêtre principale : {e}"))
}

pub struct Manager {
    pub store: Store,
    pub settings: Settings,
    pub page: Page,
    events: Receiver<Event>,
    pub status: Option<Status>,
    status_at: Option<Instant>,
    /// Modale d'accord pour le mode géré ouverte.
    pub asking_consent: bool,
    /// Dernière erreur à montrer.
    pub error: Option<String>,
}

/// Fréquence de mise à jour de l'état de l'agent tant que la fenêtre est ouverte.
const STATUS_EVERY: Duration = Duration::from_secs(2);

impl Manager {
    fn new(store: Store, settings: Settings, events: Receiver<Event>, page: Option<Page>) -> Self {
        Self {
            store,
            settings,
            page: page.unwrap_or(Page::Home),
            events,
            status: None,
            status_at: None,
            asking_consent: false,
            error: None,
        }
    }

    /// Enregistre les réglages et prévient l'agent.
    pub fn save(&mut self) {
        match self.store.save(&self.settings) {
            Ok(()) => {
                crate::lang::apply(self.settings.language);
                if ipc::request(&self.store, &Request::ReloadSettings).is_err() {
                    self.error = Some(t!("error.agent_unreachable").into());
                }
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    pub fn request(&mut self, req: Request) {
        match ipc::request(&self.store, &req) {
            Ok(Response::Error { message }) => self.error = Some(message),
            Ok(_) => self.status_at = None,
            Err(_) => self.error = Some(t!("error.agent_unreachable").into()),
        }
    }

    fn refresh_status(&mut self, ctx: &egui::Context) {
        if self.status_at.is_some_and(|t| t.elapsed() < STATUS_EVERY) {
            return;
        }
        self.status = match ipc::request(&self.store, &Request::Status) {
            Ok(Response::Status(s)) => Some(s),
            _ => None,
        };
        self.status_at = Some(Instant::now());
        ctx.request_repaint_after(STATUS_EVERY);
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::FocusManager => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Event::SettingsChanged => {
                    self.settings = self.store.load();
                    crate::lang::apply(self.settings.language);
                }
                Event::Shutdown => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                _ => {}
            }
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            widgets::app_header(ui, t!("manager.title"));
            ui.add_space(8.0);
            match &self.status {
                Some(_) => widgets::badge(ui, theme::ACCENT, t!("manager.agent_running")),
                None => widgets::badge(ui, theme::WARNING, t!("manager.agent_stopped")),
            };
            ui.label(widgets::dim(t!("manager.subtitle")));
        });
    }

    fn nav(&mut self, ui: &mut egui::Ui) {
        let selected = Page::ALL.iter().position(|p| *p == self.page).unwrap_or(0);
        let items = Page::ALL.map(|p| (p.icon(), p.label()));
        if let Some(i) = widgets::nav(ui, selected, items) {
            self.page = Page::ALL[i];
        }
    }
}

impl eframe::App for Manager {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_events(&ctx);
        self.refresh_status(&ctx);
        egui::Panel::top("en-tête")
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .inner_margin(egui::Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| self.header(ui));
        egui::Panel::left("navigation")
            .resizable(false)
            .exact_size(210.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(12)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER)),
            )
            .show(ui, |ui| self.nav(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(24))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| pages::show(self, ui));
            });
        pages::modals(self, &ctx);
    }
}
