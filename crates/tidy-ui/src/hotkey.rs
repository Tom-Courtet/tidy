//! Saisie et affichage des raccourcis globaux (repris de Snappy et Soundy).
//!
//! Les raccourcis sont écrits au format de la crate `global-hotkey` (`control+alt+KeyH`,
//! `alt+Space`) et affichés à la façon du système : `⌃⌥H` sous macOS, `Ctrl+Alt+H`
//! ailleurs.
//!
//! Particularités du clavier sous egui :
//! - ⌘C, ⌘X et ⌘V (Ctrl sous Windows) arrivent comme `Event::Copy`, `Cut` et `Paste`,
//!   pas comme des touches ;
//! - la touche Windows n'est pas dans `egui::Modifiers` : on la suit par ses appuis ;
//! - Windows n'envoie pas l'appui d'Impr. écran, seulement son relâchement ; ⌘V avec un
//!   presse-papiers vide ne produit pas non plus d'appui. Pour ces touches, un
//!   relâchement sans appui vaut un appui.
//!
//! Un raccourci déjà enregistré par l'agent n'arrive pas jusqu'à la fenêtre (le système
//! le donne à l'agent) : pour le réattribuer, le retirer d'abord.

use std::collections::HashSet;
use std::str::FromStr;

use egui::{Button, Event, Id, Key, Label, RichText, Stroke, Ui};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};

use crate::theme::{self, Typo};
use crate::{icons, t, widgets};

/// Modificateurs maintenus. `cmd` : ⌘ sous macOS, la touche Windows ailleurs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
}

impl Mods {
    /// `super_held` : touche Windows maintenue (absente de `egui::Modifiers`).
    pub fn from_egui(m: egui::Modifiers, super_held: bool) -> Self {
        Self {
            ctrl: m.ctrl,
            alt: m.alt,
            shift: m.shift,
            cmd: m.mac_cmd || super_held,
        }
    }

    pub fn any(self) -> bool {
        self.ctrl || self.alt || self.shift || self.cmd
    }

    fn global(self) -> Modifiers {
        let mut m = Modifiers::empty();
        for (on, flag) in [
            (self.ctrl, Modifiers::CONTROL),
            (self.alt, Modifiers::ALT),
            (self.shift, Modifiers::SHIFT),
            (self.cmd, Modifiers::META),
        ] {
            if on {
                m |= flag;
            }
        }
        m
    }

    /// Libellé des modificateurs seuls (⇧⌘, Ctrl+Maj+), pendant la saisie.
    pub fn label(self) -> String {
        mods_label(self.global(), cfg!(target_os = "macos"))
    }
}

/// Touche egui → touche de `global-hotkey`. `None` : touche qu'un raccourci global ne
/// peut pas utiliser (modificateur, touche multimédia…).
pub fn code_of(key: Key) -> Option<Code> {
    use Code as C;
    Some(match key {
        Key::A => C::KeyA,
        Key::B => C::KeyB,
        Key::C => C::KeyC,
        Key::D => C::KeyD,
        Key::E => C::KeyE,
        Key::F => C::KeyF,
        Key::G => C::KeyG,
        Key::H => C::KeyH,
        Key::I => C::KeyI,
        Key::J => C::KeyJ,
        Key::K => C::KeyK,
        Key::L => C::KeyL,
        Key::M => C::KeyM,
        Key::N => C::KeyN,
        Key::O => C::KeyO,
        Key::P => C::KeyP,
        Key::Q => C::KeyQ,
        Key::R => C::KeyR,
        Key::S => C::KeyS,
        Key::T => C::KeyT,
        Key::U => C::KeyU,
        Key::V => C::KeyV,
        Key::W => C::KeyW,
        Key::X => C::KeyX,
        Key::Y => C::KeyY,
        Key::Z => C::KeyZ,
        Key::Num0 => C::Digit0,
        Key::Num1 => C::Digit1,
        Key::Num2 => C::Digit2,
        Key::Num3 => C::Digit3,
        Key::Num4 => C::Digit4,
        Key::Num5 => C::Digit5,
        Key::Num6 => C::Digit6,
        Key::Num7 => C::Digit7,
        Key::Num8 => C::Digit8,
        Key::Num9 => C::Digit9,
        Key::F1 => C::F1,
        Key::F2 => C::F2,
        Key::F3 => C::F3,
        Key::F4 => C::F4,
        Key::F5 => C::F5,
        Key::F6 => C::F6,
        Key::F7 => C::F7,
        Key::F8 => C::F8,
        Key::F9 => C::F9,
        Key::F10 => C::F10,
        Key::F11 => C::F11,
        Key::F12 => C::F12,
        Key::F13 => C::F13,
        Key::F14 => C::F14,
        Key::F15 => C::F15,
        Key::F16 => C::F16,
        Key::F17 => C::F17,
        Key::F18 => C::F18,
        Key::F19 => C::F19,
        Key::F20 => C::F20,
        Key::F21 => C::F21,
        Key::F22 => C::F22,
        Key::F23 => C::F23,
        Key::F24 => C::F24,
        Key::Space => C::Space,
        Key::Enter => C::Enter,
        Key::Tab => C::Tab,
        Key::Backspace => C::Backspace,
        Key::Delete => C::Delete,
        Key::Insert => C::Insert,
        Key::Escape => C::Escape,
        Key::Home => C::Home,
        Key::End => C::End,
        Key::PageUp => C::PageUp,
        Key::PageDown => C::PageDown,
        Key::ArrowLeft => C::ArrowLeft,
        Key::ArrowRight => C::ArrowRight,
        Key::ArrowUp => C::ArrowUp,
        Key::ArrowDown => C::ArrowDown,
        Key::Minus => C::Minus,
        Key::Equals => C::Equal,
        Key::OpenBracket => C::BracketLeft,
        Key::CloseBracket => C::BracketRight,
        Key::Backslash => C::Backslash,
        Key::IntlBackslash => C::IntlBackslash,
        Key::Semicolon => C::Semicolon,
        Key::Quote => C::Quote,
        Key::Backtick => C::Backquote,
        Key::Comma => C::Comma,
        Key::Period => C::Period,
        Key::Slash => C::Slash,
        Key::PrintScreen => C::PrintScreen,
        Key::Pause => C::Pause,
        Key::ScrollLock => C::ScrollLock,
        _ => return None,
    })
}

/// Raccourci au format des réglages : `"control+alt+shift+super+KeyS"`.
pub fn config_text(mods: Mods, code: Code) -> String {
    let mut out = String::new();
    for (on, name) in [
        (mods.ctrl, "control+"),
        (mods.alt, "alt+"),
        (mods.shift, "shift+"),
        (mods.cmd, "super+"),
    ] {
        if on {
            out.push_str(name);
        }
    }
    out.push_str(&code.to_string());
    out
}

/// Raccourci lu depuis les réglages ; `None` s'il est vide ou illisible.
pub fn parse(text: &str) -> Option<HotKey> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    HotKey::from_str(text).ok()
}

/// Les deux textes désignent-ils le même raccourci (`alt+control+KeyH` et
/// `control+alt+KeyH`) ?
pub fn same(a: &str, b: &str) -> bool {
    match (parse(a), parse(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Libellé d'un raccourci des réglages, à la façon du système. Un texte illisible est
/// rendu tel quel.
pub fn label(text: &str) -> String {
    match parse(text) {
        Some(h) => label_with(&h, cfg!(target_os = "macos")),
        None => text.to_owned(),
    }
}

fn mods_label(mods: Modifiers, mac: bool) -> String {
    let order: [(Modifiers, &str); 4] = if mac {
        [
            (Modifiers::CONTROL, "⌃"),
            (Modifiers::ALT, "⌥"),
            (Modifiers::SHIFT, "⇧"),
            (Modifiers::META, "⌘"),
        ]
    } else {
        [
            (Modifiers::CONTROL, "Ctrl"),
            (Modifiers::ALT, "Alt"),
            (Modifiers::SHIFT, t!("hotkey.shift")),
            (Modifiers::META, "Win"),
        ]
    };
    let mut out = String::new();
    for (m, name) in order {
        if mods.contains(m) {
            out.push_str(name);
            if !mac {
                out.push('+');
            }
        }
    }
    out
}

/// Libellé d'un raccourci : `⌃⌥H` (macOS) ou `Ctrl+Alt+H`.
fn label_with(hotkey: &HotKey, mac: bool) -> String {
    let mut out = mods_label(hotkey.mods, mac);
    out.push_str(&key_label(hotkey.key, mac));
    out
}

/// Nom affiché d'une touche.
fn key_label(code: Code, mac: bool) -> String {
    let os = |m: &'static str, other: &'static str| if mac { m } else { other };
    let name = match code {
        Code::Space => t!("hotkey.key_space"),
        Code::Escape => t!("hotkey.key_escape"),
        Code::Enter => os("↩", t!("hotkey.key_enter")),
        Code::Backspace => os("⌫", t!("hotkey.key_backspace")),
        Code::Delete => os("⌦", t!("hotkey.key_delete")),
        Code::Tab => os("⇥", "Tab"),
        Code::Insert => t!("hotkey.key_insert"),
        Code::Home => t!("hotkey.key_home"),
        Code::End => t!("hotkey.key_end"),
        Code::PageUp => t!("hotkey.key_page_up"),
        Code::PageDown => t!("hotkey.key_page_down"),
        Code::PrintScreen => t!("hotkey.key_print_screen"),
        Code::Pause => "Pause",
        Code::ScrollLock => t!("hotkey.key_scroll_lock"),
        Code::ArrowLeft => "←",
        Code::ArrowRight => "→",
        Code::ArrowUp => "↑",
        Code::ArrowDown => "↓",
        Code::Minus => "-",
        Code::Equal => "=",
        Code::BracketLeft => "[",
        Code::BracketRight => "]",
        Code::Backslash => "\\",
        Code::IntlBackslash => "<",
        Code::Semicolon => ";",
        Code::Quote => "'",
        Code::Backquote => "`",
        Code::Comma => ",",
        Code::Period => ".",
        Code::Slash => "/",
        _ => {
            let key = code.to_string();
            let short = key
                .strip_prefix("Digit")
                .or_else(|| key.strip_prefix("Key"))
                .map(str::to_owned)
                .or_else(|| key.strip_prefix("Numpad").map(|k| format!("Num{k}")));
            return short.unwrap_or(key);
        }
    };
    name.to_owned()
}

/// Touches utilisables seules (ou avec ⇧ seul) : elles ne tapent pas de texte.
fn standalone(code: Code) -> bool {
    use Code as C;
    matches!(
        code,
        C::F1
            | C::F2
            | C::F3
            | C::F4
            | C::F5
            | C::F6
            | C::F7
            | C::F8
            | C::F9
            | C::F10
            | C::F11
            | C::F12
            | C::F13
            | C::F14
            | C::F15
            | C::F16
            | C::F17
            | C::F18
            | C::F19
            | C::F20
            | C::F21
            | C::F22
            | C::F23
            | C::F24
            | C::PrintScreen
    )
}

/// Raccourci refusé, quel que soit son usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    /// Sans modificateur, la touche ne taperait plus rien ailleurs.
    NoModifier,
    /// ⇧ seul : la touche ne taperait plus sa majuscule.
    ShiftOnly,
    /// Illisible pour `global-hotkey`.
    Unreadable,
}

impl Invalid {
    /// Explication, dans la langue de l'interface.
    pub fn message(self) -> String {
        let mods = if cfg!(target_os = "macos") {
            t!("hotkey.modifiers_mac")
        } else {
            t!("hotkey.modifiers_other")
        };
        match self {
            Invalid::NoModifier => t!("hotkey.no_modifier", mods = mods),
            Invalid::ShiftOnly => t!("hotkey.shift_only", mods = mods),
            Invalid::Unreadable => t!("hotkey.unreadable").to_owned(),
        }
    }
}

/// Vérifie qu'un raccourci est utilisable comme raccourci global.
pub fn validate(text: &str) -> Result<(), Invalid> {
    let hotkey = parse(text).ok_or(Invalid::Unreadable)?;
    if !standalone(hotkey.key) {
        if hotkey.mods.is_empty() {
            return Err(Invalid::NoModifier);
        }
        if hotkey.mods == Modifiers::SHIFT {
            return Err(Invalid::ShiftOnly);
        }
    }
    Ok(())
}

/// Premier propriétaire (parmi `others`) qui utilise déjà ce raccourci. Les textes
/// vides ou illisibles sont ignorés.
pub fn conflict<'a, T>(
    text: &str,
    others: impl IntoIterator<Item = (T, Option<&'a str>)>,
) -> Option<T> {
    let wanted = parse(text)?;
    others
        .into_iter()
        .find(|(_, other)| other.and_then(parse) == Some(wanted))
        .map(|(owner, _)| owner)
}

/// Raccourci que le système garde pour lui (ou presque toujours pris).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reserved {
    Spotlight,
    Screenshot,
    AppSwitcher,
    Spaces,
    WindowsKey,
    WindowsSystem,
}

fn reserved(hotkey: &HotKey, mac: bool) -> Option<Reserved> {
    let m = hotkey.mods;
    let k = hotkey.key;
    let cmd = Modifiers::META;
    if mac {
        if k == Code::Space && (m == cmd || m == cmd | Modifiers::ALT) {
            return Some(Reserved::Spotlight);
        }
        if m == cmd | Modifiers::SHIFT && matches!(k, Code::Digit3 | Code::Digit4 | Code::Digit5) {
            return Some(Reserved::Screenshot);
        }
        if m == cmd && k == Code::Tab {
            return Some(Reserved::AppSwitcher);
        }
        if m == Modifiers::CONTROL
            && matches!(
                k,
                Code::ArrowLeft | Code::ArrowRight | Code::ArrowUp | Code::ArrowDown
            )
        {
            return Some(Reserved::Spaces);
        }
        None
    } else {
        if m.contains(cmd) {
            return Some(Reserved::WindowsKey);
        }
        let system = (m == Modifiers::ALT && matches!(k, Code::Tab | Code::F4 | Code::Escape))
            || (m == Modifiers::CONTROL && k == Code::Escape)
            || (m == Modifiers::CONTROL | Modifiers::SHIFT && k == Code::Escape)
            || (m == Modifiers::CONTROL | Modifiers::ALT && k == Code::Delete);
        system.then_some(Reserved::WindowsSystem)
    }
}

/// Avertissement si le système réserve déjà ce raccourci (⌘Espace, ⇧⌘3-5 sous macOS,
/// touche Windows…). Ce n'est pas un refus : le raccourci peut ne pas marcher.
pub fn system_warning(text: &str) -> Option<&'static str> {
    let hotkey = parse(text)?;
    Some(match reserved(&hotkey, cfg!(target_os = "macos"))? {
        Reserved::Spotlight => t!("hotkey.reserved_spotlight"),
        Reserved::Screenshot => t!("hotkey.reserved_screenshot"),
        Reserved::AppSwitcher => t!("hotkey.reserved_app_switcher"),
        Reserved::Spaces => t!("hotkey.reserved_spaces"),
        Reserved::WindowsKey => t!("hotkey.reserved_windows_key"),
        Reserved::WindowsSystem => t!("hotkey.reserved_windows_system"),
    })
}

/// Résultat d'un enregistrement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorded {
    /// Combinaison tapée, au format des réglages.
    Combo(String),
    /// Échap sans modificateur.
    Cancel,
    /// Touche sans équivalent pour un raccourci global.
    Unsupported,
}

/// Lit le clavier pendant l'enregistrement d'un raccourci.
#[derive(Debug, Clone, Default)]
pub struct Recorder {
    /// Touche Windows maintenue.
    super_held: bool,
    /// Touches vues enfoncées depuis le début de l'enregistrement.
    pressed: HashSet<Key>,
}

impl Recorder {
    /// Modificateurs maintenus en ce moment, pour l'affichage.
    pub fn live(&self, m: egui::Modifiers) -> Mods {
        Mods::from_egui(m, self.super_held)
    }

    /// Oublie la touche Windows quand la fenêtre perd le clavier (son relâchement
    /// arrivera ailleurs).
    pub fn focus_lost(&mut self) {
        self.super_held = false;
    }

    /// Lit les événements d'une frame ; `now` : modificateurs maintenus (pour `Copy`,
    /// `Cut` et `Paste`, qui n'en portent pas).
    pub fn read(&mut self, events: &[Event], now: egui::Modifiers) -> Option<Recorded> {
        for event in events {
            match event {
                Event::Key {
                    key,
                    physical_key,
                    pressed,
                    repeat,
                    modifiers,
                } => {
                    // macOS enregistre la position de la touche (clavier US) ;
                    // Windows, la touche telle qu'elle est gravée.
                    let key = if cfg!(target_os = "macos") {
                        physical_key.unwrap_or(*key)
                    } else {
                        *key
                    };
                    if matches!(key, Key::SuperLeft | Key::SuperRight) {
                        self.super_held = *pressed;
                        continue;
                    }
                    if is_modifier(key) || *repeat {
                        continue;
                    }
                    let mods = Mods::from_egui(*modifiers, self.super_held);
                    if *pressed {
                        self.pressed.insert(key);
                    } else if self.pressed.contains(&key) || !release_counts(key, mods) {
                        continue;
                    }
                    if key == Key::Escape && !mods.any() {
                        return Some(Recorded::Cancel);
                    }
                    return Some(match code_of(key) {
                        Some(code) => Recorded::Combo(config_text(mods, code)),
                        None => Recorded::Unsupported,
                    });
                }
                Event::Copy | Event::Cut | Event::Paste(_) => {
                    let key = match event {
                        Event::Copy => Key::C,
                        Event::Cut => Key::X,
                        _ => Key::V,
                    };
                    self.pressed.insert(key);
                    let mut mods = Mods::from_egui(now, self.super_held);
                    if cfg!(target_os = "macos") {
                        mods.cmd = true;
                    } else {
                        mods.ctrl = true;
                    }
                    let code = code_of(key)?;
                    return Some(Recorded::Combo(config_text(mods, code)));
                }
                _ => {}
            }
        }
        None
    }
}

fn is_modifier(key: Key) -> bool {
    matches!(
        key,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::AltLeft
            | Key::AltRight
    )
}

/// Touches dont le système peut ne transmettre que le relâchement (voir le haut du
/// fichier).
fn release_counts(key: Key, mods: Mods) -> bool {
    let command = if cfg!(target_os = "macos") {
        mods.cmd
    } else {
        mods.ctrl
    };
    key == Key::PrintScreen || (command && matches!(key, Key::C | Key::X | Key::V))
}

/// Ce que l'utilisateur a fait dans un [`field`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldChange {
    /// Nouveau raccourci, déjà vérifié.
    Set(String),
    /// Raccourci retiré.
    Cleared,
}

/// État d'un champ, gardé dans la mémoire d'egui.
#[derive(Debug, Clone, Default)]
struct FieldState {
    recorder: Recorder,
    error: Option<String>,
}

/// Largeur du champ de raccourci (touche et messages).
const FIELD_WIDTH: f32 = 250.0;

/// Identifiant du champ en cours d'enregistrement (un seul à la fois).
fn recording_id() -> Id {
    Id::unique("tidy-raccourci-en-saisie")
}

/// Champ de raccourci : la combinaison actuelle en « touche » cliquable, à cliquer
/// pour taper la nouvelle (Échap annule), et un bouton « Retirer ». `check` vérifie la
/// combinaison tapée (conflit avec un autre raccourci de Tidy) : `Err(message)` la
/// refuse et laisse la saisie ouverte. Avertit si le système réserve le raccourci.
pub fn field(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    value: Option<&str>,
    check: impl FnOnce(&str) -> Result<(), String>,
) -> Option<FieldChange> {
    let id = ui.make_persistent_id(("raccourci", id_salt));
    let recording = ui.data(|d| d.get_temp::<Id>(recording_id())) == Some(id);
    let mut state: FieldState = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let mut change = None;
    let mut stop = false;

    if recording {
        let (events, now, focused) = ui.input(|i| (i.events.clone(), i.modifiers, i.focused));
        if !focused {
            state.recorder.focus_lost();
        }
        match state.recorder.read(&events, now) {
            Some(Recorded::Cancel) => stop = true,
            Some(Recorded::Unsupported) => {
                state.error = Some(t!("hotkey.unsupported").to_owned());
            }
            Some(Recorded::Combo(text)) => match validate(&text).map_err(Invalid::message) {
                Ok(()) => match check(&text) {
                    Ok(()) => {
                        change = Some(FieldChange::Set(text));
                        stop = true;
                    }
                    Err(e) => state.error = Some(e),
                },
                Err(e) => state.error = Some(e),
            },
            None => {}
        }
        // Les touches tapées ne doivent rien déclencher d'autre (Tab, Entrée…).
        ui.input_mut(|i| {
            i.events.retain(|e| {
                !matches!(
                    e,
                    Event::Key { .. } | Event::Text(_) | Event::Copy | Event::Cut | Event::Paste(_)
                )
            })
        });
    }
    let recording = recording && !stop;

    // Bloc de largeur fixe, aligné à droite : la touche (et « Retirer ») en haut, le
    // message (erreur, aide, avertissement) dessous.
    let width = FIELD_WIDTH.min(ui.available_width());
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Max),
        |ui| {
            ui.set_width(width);
            ui.allocate_ui_with_layout(
                egui::vec2(width, 28.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    // De droite à gauche : « Retirer », puis la touche.
                    if !recording
                        && value.is_some()
                        && ui
                            .small_button(icons::CLOSE)
                            .on_hover_text(t!("hotkey.remove"))
                            .clicked()
                    {
                        change = Some(FieldChange::Cleared);
                    }
                    let (text, color) = if recording {
                        let live = state.recorder.live(ui.input(|i| i.modifiers));
                        if live.any() {
                            (format!("{}…", live.label()), theme::ACCENT_2)
                        } else {
                            (t!("hotkey.press").to_owned(), theme::ACCENT_2)
                        }
                    } else {
                        match value {
                            Some(v) => (label(v), theme::TEXT),
                            None => (t!("hotkey.none").to_owned(), theme::TEXT_DIM),
                        }
                    };
                    let stroke = if recording {
                        Stroke::new(1.5, theme::ACCENT_2)
                    } else {
                        Stroke::new(1.0, theme::BORDER)
                    };
                    let button = Button::new(RichText::new(text).bold().color(color))
                        .fill(theme::SURFACE_HI)
                        .stroke(stroke)
                        .min_size(egui::vec2(130.0, 28.0));
                    let response = ui.add(button).on_hover_text(if recording {
                        t!("hotkey.record_hint")
                    } else {
                        t!("hotkey.change")
                    });
                    if response.clicked() {
                        if recording {
                            stop = true;
                        } else {
                            state = FieldState::default();
                            ui.data_mut(|d| d.insert_temp(recording_id(), id));
                        }
                        // Sinon Entrée ou Espace recliqueraient le bouton.
                        response.surrender_focus();
                    }
                },
            );

            let recording = recording && !stop;
            if stop {
                ui.data_mut(|d| d.remove::<Id>(recording_id()));
                state.error = None;
            }
            let message =
                |text: String, color| RichText::new(text).color(color).size(theme::SIZE_S);
            if recording {
                ui.ctx().request_repaint();
                match &state.error {
                    Some(error) => {
                        ui.add(Label::new(message(error.clone(), theme::DANGER)).wrap());
                    }
                    None => {
                        ui.label(widgets::dim(t!("hotkey.record_hint")));
                    }
                }
            } else if let Some(warning) = value.and_then(system_warning) {
                ui.add(
                    Label::new(message(
                        format!("{}  {warning}", icons::WARNING),
                        theme::WARNING,
                    ))
                    .wrap(),
                );
            }
        },
    );
    ui.data_mut(|d| d.insert_temp(id, state));
    change
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, pressed: bool, modifiers: egui::Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers,
        }
    }

    /// ⌘ sous macOS, Ctrl ailleurs, tels qu'egui-winit les remplit.
    fn command() -> egui::Modifiers {
        if cfg!(target_os = "macos") {
            egui::Modifiers::MAC_CMD | egui::Modifiers::COMMAND
        } else {
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND
        }
    }

    #[test]
    fn every_mapped_key_is_readable_by_global_hotkey() {
        let mut seen = HashSet::new();
        for &k in Key::ALL {
            let Some(code) = code_of(k) else { continue };
            assert!(seen.insert(code), "{code:?} deux fois");
            let text = config_text(
                Mods {
                    ctrl: true,
                    ..Mods::default()
                },
                code,
            );
            let hotkey = HotKey::from_str(&text).unwrap_or_else(|e| panic!("{text} : {e}"));
            assert_eq!(hotkey.key, code, "{text}");
            assert_eq!(hotkey.mods, Modifiers::CONTROL, "{text}");
        }
    }

    #[test]
    fn labels_follow_the_platform() {
        let hk = parse("control+alt+KeyH").unwrap();
        assert_eq!(label_with(&hk, true), "⌃⌥H");
        assert_eq!(label_with(&hk, false), "Ctrl+Alt+H");
        let hk = parse("super+shift+Digit3").unwrap();
        assert_eq!(label_with(&hk, true), "⇧⌘3");
        let hk = parse("alt+Backquote").unwrap();
        assert_eq!(label_with(&hk, false), "Alt+`");
        let hk = parse("F13").unwrap();
        assert_eq!(label_with(&hk, false), "F13");
        // Illisible : rendu tel quel.
        assert_eq!(label("pas+un+raccourci"), "pas+un+raccourci");
    }

    #[test]
    fn same_hotkey_written_differently() {
        assert!(same("alt+control+KeyH", "control+alt+KeyH"));
        assert!(same("cmd+Space", "super+Space"));
        assert!(!same("control+KeyH", "alt+KeyH"));
        assert!(!same("", ""));
    }

    #[test]
    fn modifiers_are_required_except_for_function_keys() {
        assert_eq!(validate("KeyA"), Err(Invalid::NoModifier));
        assert_eq!(validate("Space"), Err(Invalid::NoModifier));
        assert_eq!(validate("shift+KeyA"), Err(Invalid::ShiftOnly));
        assert_eq!(validate("pas un raccourci"), Err(Invalid::Unreadable));
        assert_eq!(validate("F5"), Ok(()));
        assert_eq!(validate("shift+PrintScreen"), Ok(()));
        assert_eq!(validate("control+alt+KeyH"), Ok(()));
        assert!(!Invalid::NoModifier.message().is_empty());
    }

    #[test]
    fn conflicts_are_found_whatever_the_spelling() {
        let others = [
            ("recherche", Some("alt+Space")),
            ("bureau", None),
            ("boîte", Some("alt+control+KeyH")),
        ];
        assert_eq!(conflict("control+alt+KeyH", others), Some("boîte"));
        assert_eq!(conflict("control+alt+KeyJ", others), None);
        assert_eq!(conflict("", others), None);
    }

    #[test]
    fn system_shortcuts_are_flagged() {
        let mac = |t: &str| reserved(&parse(t).unwrap(), true);
        let win = |t: &str| reserved(&parse(t).unwrap(), false);
        assert_eq!(mac("super+Space"), Some(Reserved::Spotlight));
        assert_eq!(mac("super+shift+Digit4"), Some(Reserved::Screenshot));
        assert_eq!(mac("super+Tab"), Some(Reserved::AppSwitcher));
        assert_eq!(mac("control+ArrowLeft"), Some(Reserved::Spaces));
        assert_eq!(mac("alt+Space"), None);
        assert_eq!(mac("control+alt+KeyH"), None);
        assert_eq!(win("super+KeyE"), Some(Reserved::WindowsKey));
        assert_eq!(win("alt+F4"), Some(Reserved::WindowsSystem));
        assert_eq!(win("control+shift+Escape"), Some(Reserved::WindowsSystem));
        assert_eq!(win("control+alt+KeyH"), None);
        assert_eq!(win("super+shift+Digit3"), Some(Reserved::WindowsKey));
    }

    #[test]
    fn typed_combination_is_recorded() {
        let mut r = Recorder::default();
        let mods = egui::Modifiers {
            shift: true,
            ..command()
        };
        let events = [
            key(Key::ShiftLeft, true, egui::Modifiers::SHIFT),
            key(Key::Num2, true, mods),
        ];
        let expected = if cfg!(target_os = "macos") {
            "shift+super+Digit2"
        } else {
            "control+shift+Digit2"
        };
        assert_eq!(
            r.read(&events, mods),
            Some(Recorded::Combo(expected.into()))
        );
    }

    #[test]
    fn escape_cancels_but_not_with_modifiers() {
        let mut r = Recorder::default();
        let none = egui::Modifiers::NONE;
        assert_eq!(
            r.read(&[key(Key::Escape, true, none)], none),
            Some(Recorded::Cancel)
        );
        let ctrl = egui::Modifiers::CTRL;
        assert_eq!(
            r.read(&[key(Key::Escape, true, ctrl)], ctrl),
            Some(Recorded::Combo("control+Escape".into()))
        );
    }

    #[test]
    fn clipboard_shortcuts_are_recorded() {
        let mut r = Recorder::default();
        let Some(Recorded::Combo(text)) = r.read(&[Event::Copy], command()) else {
            panic!("⌘C doit être enregistré");
        };
        assert!(text.ends_with("KeyC"), "{text}");
        // Son relâchement ne compte pas une deuxième fois.
        assert_eq!(r.read(&[key(Key::C, false, command())], command()), None);
    }

    #[test]
    fn windows_key_is_tracked_from_its_presses() {
        let mut r = Recorder::default();
        let none = egui::Modifiers::NONE;
        let events = [key(Key::SuperLeft, true, none), key(Key::F16, true, none)];
        assert_eq!(
            r.read(&events, none),
            Some(Recorded::Combo("super+F16".into()))
        );
        assert!(r.live(none).cmd);
        r.focus_lost();
        assert!(!r.live(none).cmd);
    }

    #[test]
    fn unsupported_keys_are_reported() {
        let mut r = Recorder::default();
        let ctrl = egui::Modifiers::CTRL;
        assert_eq!(
            r.read(&[key(Key::BrowserBack, true, ctrl)], ctrl),
            Some(Recorded::Unsupported)
        );
    }

    /// Le champ : un clic ouvre la saisie, la combinaison tapée est rendue, Échap
    /// annule.
    #[test]
    fn field_records_then_returns_the_combination() {
        let ctx = egui::Context::default();
        let run = |events: Vec<Event>, click: bool| {
            let mut events = events;
            if click {
                let pos = egui::pos2(200.0, 14.0);
                events.push(Event::PointerMoved(pos));
                events.push(Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                });
                events.push(Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 400.0),
                )),
                events,
                ..Default::default()
            };
            let mut out = None;
            let full = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        out = field(ui, "essai", Some("alt+Space"), |t| {
                            if same(t, "control+alt+KeyJ") {
                                Err("pris".into())
                            } else {
                                Ok(())
                            }
                        });
                    });
            });
            full.drop_without_applying_deltas();
            out
        };
        assert_eq!(run(vec![], false), None);
        assert_eq!(run(vec![], true), None, "le clic ouvre la saisie");
        let ctrl_alt = egui::Modifiers::CTRL | egui::Modifiers::ALT;
        // Raccourci déjà pris : refusé, la saisie reste ouverte.
        assert_eq!(run(vec![key(Key::J, true, ctrl_alt)], false), None);
        assert_eq!(
            run(vec![key(Key::K, true, ctrl_alt)], false),
            Some(FieldChange::Set("control+alt+KeyK".into()))
        );
        // Saisie fermée : une touche ne change plus rien.
        assert_eq!(run(vec![key(Key::L, true, ctrl_alt)], false), None);
    }
}
