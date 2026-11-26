//! Widget Chat IA : conversation avec Claude, réponse en flux, invites prédéfinies
//! (traduire, résumer, rédiger, expliquer du code), copier une réponse, effacer.
//!
//! La clé de l'API est rangée dans le gestionnaire d'identifiants du système (jamais
//! dans les réglages du widget) ; sans clé, le widget explique où la trouver et propose
//! un champ pour la coller. La conversation reste en mémoire : rien n'est écrit sur le
//! disque, rien ne part sur le réseau avant le premier envoi.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use egui::{Align, Key, Layout, Modifiers, RichText, Ui};
use serde::{Deserialize, Serialize};
use tidy_integrations::ai::{
    AiError, AiProvider, ChatMessage, ChatRequest, Claude, DEFAULT_MODEL, Role, StopReason,
};
use tidy_integrations::secret::{self, SecretError, SecretStore};
use tidy_integrations::task::{Polled, Task, Waker};
use tidy_ui::{icons, t, theme, widgets};

use crate::online::{self, net_error_text};
use crate::{Widget, WidgetCx};

/// Échanges gardés dans la demande (les plus anciens sont oubliés).
const HISTORY: usize = 20;

/// Réglages enregistrés (la clé n'y est jamais).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiChatSettings {
    /// Modèle de l'API Claude.
    pub model: String,
}

impl Default for AiChatSettings {
    fn default() -> Self {
        Self {
            model: DEFAULT_MODEL.to_owned(),
        }
    }
}

/// Invite prédéfinie : une consigne ajoutée avant le texte de l'utilisateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Preset {
    Translate,
    Summarize,
    Write,
    ExplainCode,
}

impl Preset {
    const ALL: [Preset; 4] = [
        Preset::Translate,
        Preset::Summarize,
        Preset::Write,
        Preset::ExplainCode,
    ];

    fn label(self) -> &'static str {
        match self {
            Preset::Translate => t!("widgets2.ai_translate"),
            Preset::Summarize => t!("widgets2.ai_summarize"),
            Preset::Write => t!("widgets2.ai_write"),
            Preset::ExplainCode => t!("widgets2.ai_explain"),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Preset::Translate => icons::LANGUAGE,
            Preset::Summarize => icons::LIST,
            Preset::Write => icons::EDIT,
            Preset::ExplainCode => icons::EXECUTABLE,
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Preset::Translate => t!("widgets2.ai_translate_hint"),
            Preset::Summarize => t!("widgets2.ai_summarize_hint"),
            Preset::Write => t!("widgets2.ai_write_hint"),
            Preset::ExplainCode => t!("widgets2.ai_explain_hint"),
        }
    }

    /// Consigne envoyée au modèle.
    fn instruction(self) -> &'static str {
        match self {
            Preset::Translate => t!("widgets2.ai_translate_prompt"),
            Preset::Summarize => t!("widgets2.ai_summarize_prompt"),
            Preset::Write => t!("widgets2.ai_write_prompt"),
            Preset::ExplainCode => t!("widgets2.ai_explain_prompt"),
        }
    }
}

/// État d'une réponse.
#[derive(Debug, Clone, PartialEq)]
enum Status {
    Streaming,
    Done,
    Stopped,
    Refused,
    Truncated,
    Failed(AiError),
}

#[derive(Debug, Clone)]
struct Message {
    role: Role,
    /// Texte affiché (sans la consigne de l'invite).
    text: String,
    preset: Option<Preset>,
    status: Status,
}

/// Morceaux de la réponse, envoyés par le fil réseau.
enum Chunk {
    Text(String),
    End(Result<StopReason, AiError>),
}

struct Stream {
    rx: Receiver<Chunk>,
    cancel: Arc<AtomicBool>,
}

/// Présence de la clé dans le gestionnaire d'identifiants.
enum KeyState {
    Unknown,
    Checking(Task<Result<bool, SecretError>>),
    Present,
    Missing,
    Unavailable,
}

/// Crée le client d'API à partir de la clé (remplacé dans les tests).
type ProviderFactory = Arc<dyn Fn(String) -> Box<dyn AiProvider> + Send + Sync>;

pub struct AiChatWidget {
    s: AiChatSettings,
    store: Arc<dyn SecretStore>,
    factory: ProviderFactory,
    key: KeyState,
    messages: Vec<Message>,
    input: String,
    preset: Option<Preset>,
    stream: Option<Stream>,
    /// Saisie de la clé, et résultat du dernier enregistrement.
    key_input: String,
    key_task: Option<Task<Result<bool, SecretError>>>,
    key_note: Option<(bool, String)>,
}

/// Création depuis les réglages enregistrés (ni réseau ni trousseau ici).
pub fn create(settings: &serde_json::Value) -> Box<dyn Widget> {
    Box::new(AiChatWidget::new(
        serde_json::from_value(settings.clone()).unwrap_or_default(),
        Arc::new(secret::CLAUDE_API_KEY),
        Arc::new(|key| Box::new(Claude::new(key)) as Box<dyn AiProvider>),
    ))
}

impl AiChatWidget {
    fn new(s: AiChatSettings, store: Arc<dyn SecretStore>, factory: ProviderFactory) -> Self {
        Self {
            s,
            store,
            factory,
            key: KeyState::Unknown,
            messages: Vec::new(),
            input: String::new(),
            preset: None,
            stream: None,
            key_input: String::new(),
            key_task: None,
            key_note: None,
        }
    }

    fn model(&self) -> String {
        let m = self.s.model.trim();
        if m.is_empty() {
            DEFAULT_MODEL.to_owned()
        } else {
            m.to_owned()
        }
    }

    /// Vérifie la présence de la clé (une fois, hors du fil de l'interface : l'accès au
    /// trousseau peut attendre une autorisation).
    fn pump_key(&mut self, ui: &Ui) {
        match &self.key {
            KeyState::Unknown => {
                let store = self.store.clone();
                self.key =
                    KeyState::Checking(Task::spawn("cle-ia", online::waker(ui), move || {
                        store.get().map(|k| k.is_some())
                    }));
            }
            KeyState::Checking(task) => match task.poll() {
                Polled::Pending => {}
                Polled::Done(Ok(true)) => self.key = KeyState::Present,
                Polled::Done(Ok(false)) => self.key = KeyState::Missing,
                Polled::Done(Err(e)) => {
                    log::warn!("clé de l'API Claude : {e}");
                    self.key = KeyState::Unavailable;
                }
                Polled::Lost => self.key = KeyState::Unavailable,
            },
            _ => {}
        }
        if let Some(task) = &self.key_task {
            match task.poll() {
                Polled::Pending => {}
                Polled::Done(Ok(saved)) => {
                    self.key = if saved {
                        KeyState::Present
                    } else {
                        KeyState::Missing
                    };
                    self.key_note = Some((
                        true,
                        if saved {
                            t!("widgets2.ai_key_saved").to_owned()
                        } else {
                            t!("widgets2.ai_key_deleted").to_owned()
                        },
                    ));
                    self.key_task = None;
                }
                Polled::Done(Err(e)) => {
                    log::warn!("clé de l'API Claude : {e}");
                    self.key_note = Some((false, t!("widgets2.ai_key_store_error").to_owned()));
                    self.key_task = None;
                }
                Polled::Lost => {
                    self.key_note = Some((false, t!("widgets2.ai_key_store_error").to_owned()));
                    self.key_task = None;
                }
            }
        }
    }

    /// Relève les morceaux de la réponse en cours.
    fn pump_stream(&mut self) {
        let Some(stream) = &self.stream else {
            return;
        };
        let Some(last) = self.messages.last_mut() else {
            self.stream = None;
            return;
        };
        loop {
            match stream.rx.try_recv() {
                Ok(Chunk::Text(t)) => last.text.push_str(&t),
                Ok(Chunk::End(r)) => {
                    last.status = match r {
                        Ok(StopReason::Refusal) => Status::Refused,
                        Ok(StopReason::MaxTokens) => Status::Truncated,
                        Ok(_) => Status::Done,
                        Err(AiError::Cancelled) => Status::Stopped,
                        Err(e) => {
                            if e == AiError::NoKey {
                                self.key = KeyState::Missing;
                            }
                            if e == AiError::Auth {
                                self.key_note =
                                    Some((false, t!("widgets2.ai_err_auth").to_owned()));
                            }
                            Status::Failed(e)
                        }
                    };
                    self.stream = None;
                    return;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    last.status = Status::Failed(AiError::Interrupted);
                    self.stream = None;
                    return;
                }
            }
        }
    }

    /// Conversation envoyée : les échanges réussis (ou arrêtés avec du texte), puis la
    /// nouvelle question.
    fn request(&self, question: String) -> ChatRequest {
        let mut pairs: Vec<(String, String)> = Vec::new();
        let mut pending: Option<String> = None;
        for m in &self.messages {
            match m.role {
                Role::User => pending = Some(with_preset(m.preset, &m.text)),
                Role::Assistant => {
                    let usable =
                        matches!(m.status, Status::Done | Status::Stopped | Status::Truncated)
                            && !m.text.trim().is_empty();
                    if let (Some(q), true) = (pending.take(), usable) {
                        pairs.push((q, m.text.clone()));
                    }
                }
            }
        }
        let skip = pairs.len().saturating_sub(HISTORY);
        let mut messages = Vec::new();
        for (q, a) in pairs.into_iter().skip(skip) {
            messages.push(ChatMessage {
                role: Role::User,
                text: q,
            });
            messages.push(ChatMessage {
                role: Role::Assistant,
                text: a,
            });
        }
        messages.push(ChatMessage {
            role: Role::User,
            text: question,
        });
        ChatRequest {
            model: self.model(),
            system: t!("widgets2.ai_system").to_owned(),
            messages,
        }
    }

    fn send(&mut self, waker: Waker) {
        let text = self.input.trim().to_owned();
        if text.is_empty() || self.stream.is_some() {
            return;
        }
        let request = self.request(with_preset(self.preset, &text));
        self.messages.push(Message {
            role: Role::User,
            text,
            preset: self.preset,
            status: Status::Done,
        });
        self.messages.push(Message {
            role: Role::Assistant,
            text: String::new(),
            preset: None,
            status: Status::Streaming,
        });
        self.input.clear();
        self.preset = None;

        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let store = self.store.clone();
        let factory = self.factory.clone();
        let spawned = std::thread::Builder::new()
            .name("chat-ia".into())
            .spawn(move || {
                let result = match store.get() {
                    Ok(Some(key)) => {
                        let provider = factory(key);
                        let mut on_text = |t: &str| {
                            let _ = tx.send(Chunk::Text(t.to_owned()));
                            waker();
                        };
                        provider.stream(&request, &stop, &mut on_text)
                    }
                    Ok(None) => Err(AiError::NoKey),
                    Err(e) => {
                        log::warn!("clé de l'API Claude : {e}");
                        Err(AiError::NoKey)
                    }
                };
                let _ = tx.send(Chunk::End(result));
                waker();
            });
        if let Err(e) = spawned {
            log::error!("fil du chat impossible à lancer : {e}");
        }
        self.stream = Some(Stream { rx, cancel });
    }

    fn stop(&mut self) {
        if let Some(s) = &self.stream {
            s.cancel.store(true, Ordering::Relaxed);
        }
        self.stream = None;
        if let Some(last) = self.messages.last_mut()
            && last.status == Status::Streaming
        {
            last.status = Status::Stopped;
        }
    }

    fn clear(&mut self) {
        self.stop();
        self.messages.clear();
    }

    /// Champ de la clé (dans le widget sans clé, et dans les réglages).
    fn key_field(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.key_input)
                    .password(true)
                    .hint_text("sk-ant-…")
                    .desired_width(ui.available_width() - 110.0),
            );
            let busy = self.key_task.is_some();
            ui.add_enabled_ui(!busy && !self.key_input.trim().is_empty(), |ui| {
                if widgets::primary(ui, t!("widgets2.ai_key_save")).clicked() {
                    let key = self.key_input.trim().to_owned();
                    if secret::looks_like_claude_key(&key) {
                        let store = self.store.clone();
                        self.key_task = Some(Task::spawn("cle-ia", online::waker(ui), move || {
                            store.set(&key).map(|()| true)
                        }));
                        self.key_input.clear();
                        self.key_note = None;
                    } else {
                        self.key_note = Some((false, t!("widgets2.ai_key_bad").to_owned()));
                    }
                }
            });
        });
        if let Some((ok, note)) = &self.key_note {
            let color = if *ok { theme::ACCENT_2 } else { theme::WARNING };
            ui.label(RichText::new(note).color(color).size(theme::SIZE_S));
        }
    }

    fn message_ui(ui: &mut Ui, m: &Message, copy: &mut Option<String>) {
        let width = ui.available_width();
        match m.role {
            Role::User => {
                ui.with_layout(Layout::top_down(Align::Max), |ui| {
                    if let Some(p) = m.preset {
                        ui.label(widgets::dim(format!("{}  {}", p.icon(), p.label())));
                    }
                    egui::Frame::new()
                        .fill(theme::ACCENT)
                        .corner_radius(12)
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_max_width(width * 0.85);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&m.text).color(theme::on(theme::ACCENT)),
                                )
                                .wrap(),
                            );
                        });
                });
            }
            Role::Assistant => {
                egui::Frame::new()
                    .fill(theme::SURFACE_HI)
                    .corner_radius(12)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_max_width(width * 0.92);
                        if m.text.is_empty() && m.status == Status::Streaming {
                            ui.add(egui::Spinner::new().size(14.0));
                        } else {
                            ui.add(egui::Label::new(&m.text).selectable(true).wrap());
                        }
                    });
                let note = match &m.status {
                    Status::Streaming | Status::Done => None,
                    Status::Stopped => Some(t!("widgets2.ai_stopped").to_owned()),
                    Status::Refused => Some(t!("widgets2.ai_refused").to_owned()),
                    Status::Truncated => Some(t!("widgets2.ai_truncated").to_owned()),
                    Status::Failed(e) => Some(ai_error_text(e)),
                };
                if let Some(note) = note {
                    let color = if matches!(m.status, Status::Failed(_)) {
                        theme::WARNING
                    } else {
                        theme::TEXT_DIM
                    };
                    ui.label(RichText::new(note).color(color).size(theme::SIZE_S));
                }
                if m.status != Status::Streaming
                    && !m.text.is_empty()
                    && ui
                        .small_button(format!("{}  {}", icons::COPY, t!("widgets2.ai_copy")))
                        .clicked()
                {
                    *copy = Some(m.text.clone());
                }
            }
        }
    }
}

/// Texte envoyé : la consigne de l'invite, puis le texte de l'utilisateur.
fn with_preset(preset: Option<Preset>, text: &str) -> String {
    match preset {
        Some(p) => format!("{}\n\n{text}", p.instruction()),
        None => text.to_owned(),
    }
}

fn ai_error_text(e: &AiError) -> String {
    match e {
        AiError::NoKey => t!("widgets2.ai_err_no_key").to_owned(),
        AiError::Auth => t!("widgets2.ai_err_auth").to_owned(),
        AiError::Permission => t!("widgets2.ai_err_permission").to_owned(),
        AiError::NotFound(_) => t!("widgets2.ai_err_model").to_owned(),
        AiError::RateLimited => t!("widgets2.ai_err_rate").to_owned(),
        AiError::Overloaded => t!("widgets2.ai_err_overloaded").to_owned(),
        AiError::BadRequest(m) if !m.is_empty() => t!("widgets2.ai_err_request", message = m),
        AiError::BadRequest(_) | AiError::Api(_) => t!("widgets2.ai_err_other").to_owned(),
        AiError::Server(_) => t!("widgets2.ai_err_server").to_owned(),
        AiError::Net(e) => net_error_text(e),
        AiError::Interrupted => t!("widgets2.ai_err_interrupted").to_owned(),
        AiError::Cancelled => t!("widgets2.ai_stopped").to_owned(),
    }
}

impl Widget for AiChatWidget {
    fn ui(&mut self, ui: &mut Ui, _cx: &mut WidgetCx) {
        self.pump_key(ui);
        self.pump_stream();
        let waker = online::waker(ui);

        // Invites et « Effacer ».
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for p in Preset::ALL {
                let selected = self.preset == Some(p);
                if widgets::choice(ui, selected, format!("{}  {}", p.icon(), p.label())).clicked() {
                    self.preset = if selected { None } else { Some(p) };
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !self.messages.is_empty()
                    && widgets::icon_button(ui, icons::DELETE, t!("widgets2.ai_clear")).clicked()
                {
                    self.clear();
                }
            });
        });

        // Zone de saisie en bas, conversation au-dessus.
        let input_height = 86.0;
        let conversation_height = (ui.available_height() - input_height).max(80.0);
        let mut copy = None;
        egui::ScrollArea::vertical()
            .id_salt("conversation")
            .max_height(conversation_height)
            .min_scrolled_height(conversation_height)
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.messages.is_empty() {
                    match self.key {
                        KeyState::Missing => {
                            widgets::notice(
                                ui,
                                theme::ACCENT,
                                icons::KEY,
                                t!("widgets2.ai_no_key"),
                            );
                            ui.add_space(4.0);
                            self.key_field(ui);
                        }
                        KeyState::Unavailable => {
                            widgets::notice(
                                ui,
                                theme::WARNING,
                                icons::WARNING,
                                t!("widgets2.ai_key_store_error"),
                            );
                        }
                        _ => {
                            ui.add_space(8.0);
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new(icons::AI).size(28.0).color(theme::ACCENT));
                                widgets::hint(ui, t!("widgets2.ai_empty"));
                            });
                        }
                    }
                }
                for m in &self.messages {
                    Self::message_ui(ui, m, &mut copy);
                }
                if self
                    .messages
                    .last()
                    .is_some_and(|m| m.status == Status::Failed(AiError::NoKey))
                {
                    self.key_field(ui);
                }
            });
        if let Some(text) = copy {
            ui.ctx().copy_text(text);
        }

        // Saisie : Entrée envoie, Maj+Entrée va à la ligne.
        let id = ui.make_persistent_id("saisie-chat");
        let focused = ui.memory(|m| m.has_focus(id));
        let enter = focused && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        let hint = self
            .preset
            .map_or(t!("widgets2.ai_input_hint"), Preset::hint);
        ui.add(
            egui::TextEdit::multiline(&mut self.input)
                .id(id)
                .hint_text(hint)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            ui.label(widgets::dim(self.model()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.stream.is_some() {
                    if ui
                        .button(format!("{}  {}", icons::PAUSE, t!("widgets2.ai_stop")))
                        .clicked()
                    {
                        self.stop();
                    }
                } else {
                    let can_send = !self.input.trim().is_empty();
                    let clicked = ui
                        .add_enabled_ui(can_send, |ui| widgets::primary(ui, t!("widgets2.ai_send")))
                        .inner
                        .clicked();
                    if (clicked || enter) && can_send {
                        self.send(waker.clone());
                    }
                }
            });
        });
    }

    fn settings_ui(&mut self, ui: &mut Ui, cx: &mut WidgetCx) -> bool {
        self.pump_key(ui);
        let mut changed = false;
        widgets::section_title(ui, t!("widgets2.ai_key"));
        let status = match self.key {
            KeyState::Present => t!("widgets2.ai_key_present"),
            KeyState::Missing => t!("widgets2.ai_key_missing"),
            KeyState::Unavailable => t!("widgets2.ai_key_store_error"),
            KeyState::Unknown | KeyState::Checking(_) => t!("widgets2.loading"),
        };
        ui.label(widgets::dim(status));
        self.key_field(ui);
        if matches!(self.key, KeyState::Present)
            && self.key_task.is_none()
            && ui
                .button(format!(
                    "{}  {}",
                    icons::DELETE,
                    t!("widgets2.ai_key_delete")
                ))
                .clicked()
        {
            let store = self.store.clone();
            self.key_task = Some(Task::spawn("cle-ia", online::waker(ui), move || {
                store.delete().map(|()| false)
            }));
        }
        widgets::hint(ui, t!("widgets2.ai_key_help"));

        widgets::row_separator(ui);
        widgets::setting_row(
            ui,
            "",
            t!("widgets2.ai_model"),
            Some(t!("widgets2.ai_model_hint")),
            |ui| {
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.s.model)
                            .hint_text(DEFAULT_MODEL)
                            .desired_width(150.0),
                    )
                    .changed()
                {
                    changed = true;
                }
            },
        );
        if changed {
            cx.changed = true;
        }
        changed
    }

    fn save(&self) -> serde_json::Value {
        serde_json::to_value(&self.s).unwrap_or_default()
    }

    fn repaint_after(&self) -> Option<Duration> {
        // Le fil réseau réveille la fenêtre à chaque morceau ; ceci n'est qu'un filet.
        self.stream.as_ref().map(|_| Duration::from_millis(250))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing::render;
    use tidy_integrations::secret::MemorySecret;

    /// Fournisseur sans réseau : rejoue un flux enregistré.
    struct Recorded(&'static str);

    impl AiProvider for Recorded {
        fn stream(
            &self,
            _: &ChatRequest,
            cancel: &AtomicBool,
            on_text: &mut dyn FnMut(&str),
        ) -> Result<StopReason, AiError> {
            tidy_integrations::ai::read_stream(self.0.as_bytes(), cancel, on_text)
        }
    }

    fn widget(sse: &'static str, key: Option<&str>) -> AiChatWidget {
        let store = Arc::new(MemorySecret::default());
        if let Some(k) = key {
            store.set(k).unwrap();
        }
        AiChatWidget::new(
            AiChatSettings::default(),
            store,
            Arc::new(move |_| Box::new(Recorded(sse)) as Box<dyn AiProvider>),
        )
    }

    const OK: &str = include_str!("../../tidy-integrations/tests/data/claude-stream-ok.sse");
    const ERROR: &str = include_str!("../../tidy-integrations/tests/data/claude-stream-error.sse");

    fn ask(w: &mut AiChatWidget, text: &str) {
        w.input = text.into();
        w.send(tidy_integrations::task::no_waker());
        for _ in 0..200 {
            w.pump_stream();
            if w.stream.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn answer_streams_into_the_conversation() {
        let mut w = widget(OK, Some("sk-ant-essai-0123456789abcdef"));
        w.preset = Some(Preset::Translate);
        ask(&mut w, "Bonjour le monde");
        assert_eq!(w.messages.len(), 2);
        assert_eq!(w.messages[0].text, "Bonjour le monde");
        assert_eq!(w.messages[0].preset, Some(Preset::Translate));
        assert_eq!(w.messages[1].status, Status::Done);
        assert!(w.messages[1].text.contains("Hello, world"));
        assert_eq!(w.preset, None);
        render(&mut w, 2);
    }

    #[test]
    fn without_key_the_answer_says_so() {
        let mut w = widget(OK, None);
        render(&mut w, 1);
        ask(&mut w, "Salut");
        assert_eq!(w.messages[1].status, Status::Failed(AiError::NoKey));
        render(&mut w, 2);
    }

    #[test]
    fn failed_exchanges_are_not_resent() {
        let mut w = widget(ERROR, Some("sk-ant-essai-0123456789abcdef"));
        ask(&mut w, "Première question");
        assert_eq!(w.messages[1].status, Status::Failed(AiError::Overloaded));
        assert_eq!(w.messages[1].text, "Le début de la");
        let req = w.request("Deuxième".into());
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].text, "Deuxième");
    }

    #[test]
    fn history_alternates_and_presets_add_their_instruction() {
        let mut w = widget(OK, Some("sk-ant-essai-0123456789abcdef"));
        w.preset = Some(Preset::Summarize);
        ask(&mut w, "Un long texte");
        let req = w.request("Et en une phrase ?".into());
        let roles: Vec<Role> = req.messages.iter().map(|m| m.role).collect();
        assert_eq!(roles, [Role::User, Role::Assistant, Role::User]);
        assert!(
            req.messages[0]
                .text
                .starts_with(Preset::Summarize.instruction())
        );
        assert!(req.messages[0].text.ends_with("Un long texte"));
        assert_eq!(req.model, DEFAULT_MODEL);
    }

    #[test]
    fn key_never_lands_in_the_settings() {
        let mut w = widget(OK, None);
        w.key_input = "sk-ant-essai-0123456789abcdef".into();
        render(&mut w, 1);
        assert!(!w.save().to_string().contains("sk-ant"));
    }

    #[test]
    fn clear_empties_the_conversation() {
        let mut w = widget(OK, Some("sk-ant-essai-0123456789abcdef"));
        ask(&mut w, "Salut");
        w.clear();
        assert!(w.messages.is_empty());
    }
}
