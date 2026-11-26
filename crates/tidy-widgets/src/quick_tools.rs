//! Outils rapides : grille réglable de tuiles vers des outils du système.
//!
//! Capture → Snappy (`snappy --region`, ADR 005) s'il est installé, sinon un message
//! l'explique. Les autres tuiles ouvrent l'outil équivalent de l'OS
//! ([`WidgetAction::Open`] ou [`WidgetAction::Launch`]) ; un outil sans équivalent est
//! masqué.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use egui::{Align2, CornerRadius, FontId, Rect, Sense, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};
use tidy_ui::{icons, t, theme, widgets};

use crate::{Widget, WidgetAction, WidgetCx, WidgetKind};

pub fn kind() -> WidgetKind {
    WidgetKind {
        id: "quick_tools",
        name_key: "widgets.tools_name",
        icon: icons::TOOLS,
        default_size: [250.0, 150.0],
        create: |v| {
            Box::new(QuickTools::new(
                serde_json::from_value(v.clone()).unwrap_or_default(),
            ))
        },
    }
}

/// Un outil du catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Capture,
    Calculator,
    Notepad,
    TaskManager,
    ControlPanel,
    Settings,
    RecycleBin,
    Lock,
    Terminal,
    Files,
}

impl Tool {
    pub const ALL: [Tool; 10] = [
        Tool::Capture,
        Tool::Calculator,
        Tool::Notepad,
        Tool::TaskManager,
        Tool::ControlPanel,
        Tool::Settings,
        Tool::RecycleBin,
        Tool::Lock,
        Tool::Terminal,
        Tool::Files,
    ];

    pub fn icon(self) -> &'static str {
        match self {
            Tool::Capture => icons::CAMERA,
            Tool::Calculator => icons::GRID,
            Tool::Notepad => icons::EDIT,
            Tool::TaskManager => icons::DIAGNOSTIC,
            Tool::ControlPanel => icons::OPTIONS,
            Tool::Settings => icons::SETTINGS,
            Tool::RecycleBin => icons::DELETE,
            Tool::Lock => icons::LOCK,
            Tool::Terminal => icons::EXECUTABLE,
            Tool::Files => icons::FOLDER_OPEN,
        }
    }

    pub fn label(self) -> &'static str {
        if cfg!(target_os = "macos") {
            match self {
                Tool::Notepad => return t!("widgets.tool_notepad_mac"),
                Tool::TaskManager => return t!("widgets.tool_task_manager_mac"),
                Tool::Settings => return t!("widgets.tool_settings_mac"),
                Tool::Terminal => return t!("widgets.tool_terminal_mac"),
                Tool::Files => return t!("widgets.tool_files_mac"),
                _ => {}
            }
        }
        match self {
            Tool::Capture => t!("widgets.tool_capture"),
            Tool::Calculator => t!("widgets.tool_calculator"),
            Tool::Notepad => t!("widgets.tool_notepad"),
            Tool::TaskManager => t!("widgets.tool_task_manager"),
            Tool::ControlPanel => t!("widgets.tool_control_panel"),
            Tool::Settings => t!("widgets.tool_settings"),
            Tool::RecycleBin => t!("widgets.tool_recycle_bin"),
            Tool::Lock => t!("widgets.tool_lock"),
            Tool::Terminal => t!("widgets.tool_terminal"),
            Tool::Files => t!("widgets.tool_files"),
        }
    }

    /// Ce que fait la tuile sur cet OS (`None` : pas d'équivalent, tuile masquée).
    /// La capture passe par [`find_snappy`].
    pub fn action(self) -> Option<WidgetAction> {
        let open = |s: &str| Some(WidgetAction::Open(s.to_owned()));
        let launch = |program: &str, args: &[&str]| {
            Some(WidgetAction::Launch {
                program: program.to_owned(),
                args: args.iter().map(|a| (*a).to_owned()).collect(),
            })
        };
        if cfg!(windows) {
            match self {
                Tool::Capture => None,
                Tool::Calculator => open("calc.exe"),
                Tool::Notepad => open("notepad.exe"),
                Tool::TaskManager => open("taskmgr.exe"),
                Tool::ControlPanel => open("control.exe"),
                Tool::Settings => open("ms-settings:"),
                Tool::RecycleBin => open("shell:RecycleBinFolder"),
                Tool::Lock => launch("rundll32.exe", &["user32.dll,LockWorkStation"]),
                Tool::Terminal => open("cmd.exe"),
                Tool::Files => open("explorer.exe"),
            }
        } else if cfg!(target_os = "macos") {
            let home = std::env::var("HOME").unwrap_or_default();
            match self {
                Tool::Capture | Tool::ControlPanel => None,
                Tool::Calculator => open("/System/Applications/Calculator.app"),
                Tool::Notepad => open("/System/Applications/TextEdit.app"),
                Tool::TaskManager => open("/System/Applications/Utilities/Activity Monitor.app"),
                Tool::Settings => open("x-apple.systempreferences:"),
                Tool::RecycleBin => open(&format!("{home}/.Trash")),
                // Met l'écran en veille : la session se verrouille si le mot de passe
                // est demandé au réveil (réglage par défaut).
                Tool::Lock => launch("/usr/bin/pmset", &["displaysleepnow"]),
                Tool::Terminal => open("/System/Applications/Utilities/Terminal.app"),
                Tool::Files => open(&home),
            }
        } else {
            None
        }
    }

    /// Disponible sur cet OS.
    pub fn available(self) -> bool {
        self == Tool::Capture || self.action().is_some()
    }
}

/// Réglages des outils rapides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsSettings {
    /// Outils affichés, dans l'ordre.
    pub tools: Vec<Tool>,
    pub columns: u32,
    pub labels: bool,
    /// Chemin de Snappy choisi à la main (sinon cherché aux endroits habituels).
    pub snappy_path: String,
}

impl Default for ToolsSettings {
    fn default() -> Self {
        Self {
            tools: vec![
                Tool::Capture,
                Tool::Calculator,
                Tool::Notepad,
                Tool::TaskManager,
                Tool::Settings,
                Tool::RecycleBin,
                Tool::Lock,
                Tool::Terminal,
            ],
            columns: 4,
            labels: false,
            snappy_path: String::new(),
        }
    }
}

/// Exécutable de Snappy, s'il est installé : chemin réglé, à côté de Tidy, dans le
/// `PATH`, aux emplacements habituels, puis (Windows) d'après son lancement avec la
/// session.
pub fn find_snappy(custom: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        "snappy.exe"
    } else {
        "snappy"
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    if !custom.trim().is_empty() {
        candidates.push(PathBuf::from(custom.trim()));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        candidates.push(dir.join(exe));
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join(exe)));
    }
    if cfg!(windows) {
        for var in ["LOCALAPPDATA", "ProgramFiles"] {
            if let Some(base) = std::env::var_os(var) {
                let base = PathBuf::from(base);
                candidates.push(base.join("Programs").join("Snappy").join(exe));
                candidates.push(base.join("Snappy").join(exe));
            }
        }
    } else {
        candidates.push(PathBuf::from(
            "/Applications/Snappy.app/Contents/MacOS/snappy",
        ));
        if let Some(home) = std::env::var_os("HOME") {
            candidates
                .push(PathBuf::from(home).join("Applications/Snappy.app/Contents/MacOS/snappy"));
        }
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .or_else(snappy_from_login)
}

/// Windows : chemin de Snappy d'après sa valeur `Run` (lancement avec la session).
fn snappy_from_login() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// Pas de fenêtre de console pour `reg.exe`.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("reg.exe")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "Snappy",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        parse_run_value(&String::from_utf8_lossy(&out.stdout)).filter(|p| p.is_file())
    }
    #[cfg(not(windows))]
    None
}

/// Programme d'une ligne de `reg query` (`Snappy  REG_SZ  "C:\…\snappy.exe" --background`).
pub fn parse_run_value(output: &str) -> Option<PathBuf> {
    let line = output.lines().find(|l| l.contains("REG_SZ"))?;
    let value = line.split_once("REG_SZ")?.1.trim();
    let program = match value.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => value.split(" --").next()?.trim(),
    };
    (!program.is_empty()).then(|| PathBuf::from(program))
}

/// Taille d'une tuile, selon qu'elle porte un libellé.
fn tile_size(labels: bool) -> egui::Vec2 {
    if labels {
        vec2(80.0, 58.0)
    } else {
        vec2(44.0, 44.0)
    }
}

const GAP: f32 = 6.0;

pub struct QuickTools {
    s: ToolsSettings,
    /// Réponse du fil qui cherche et lance Snappy (`false` : introuvable).
    capture: Option<Receiver<bool>>,
    /// Message « Snappy n'est pas installé », jusqu'à quand.
    missing_until: Option<Instant>,
}

impl QuickTools {
    pub fn new(s: ToolsSettings) -> Self {
        Self {
            s,
            capture: None,
            missing_until: None,
        }
    }

    fn shown(&self) -> Vec<Tool> {
        self.s
            .tools
            .iter()
            .copied()
            .filter(|t| t.available())
            .collect()
    }

    fn columns(&self, n: usize) -> usize {
        (self.s.columns.clamp(1, 8) as usize).min(n.max(1))
    }

    /// Lance Snappy sur un fil (recherche sur le disque, éventuellement `reg.exe`).
    fn capture(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let custom = self.s.snappy_path.clone();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("tidy-capture".into())
            .spawn(move || {
                let launched = match find_snappy(&custom) {
                    Some(snappy) => std::process::Command::new(&snappy)
                        .arg("--region")
                        .spawn()
                        .inspect_err(|e| log::warn!("lancement de {} : {e}", snappy.display()))
                        .is_ok(),
                    None => false,
                };
                let _ = tx.send(launched);
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => self.capture = Some(rx),
            Err(e) => log::warn!("fil de capture : {e}"),
        }
    }
}

impl Widget for QuickTools {
    fn ui(&mut self, ui: &mut Ui, cx: &mut WidgetCx) {
        if let Some(rx) = &self.capture
            && let Ok(launched) = rx.try_recv()
        {
            self.capture = None;
            if !launched {
                self.missing_until = Some(Instant::now() + Duration::from_secs(8));
            }
        }
        if self
            .missing_until
            .is_some_and(|until| Instant::now() >= until)
        {
            self.missing_until = None;
        }
        let rect = ui.max_rect();
        if self
            .missing_until
            .is_some_and(|until| Instant::now() < until)
        {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(6.0)));
            let text = if cfg!(target_os = "macos") {
                t!("widgets.tools_snappy_missing_mac")
            } else {
                t!("widgets.tools_snappy_missing")
            };
            child.add(egui::Label::new(widgets::dim(text)).wrap());
            if child.small_button(t!("common.close")).clicked() {
                self.missing_until = None;
            }
            return;
        }
        let tools = self.shown();
        if tools.is_empty() {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                t!("widgets.tools_empty"),
                FontId::proportional(theme::SIZE_S),
                theme::TEXT_DIM,
            );
            return;
        }
        let cols = self.columns(tools.len());
        let rows = tools.len().div_ceil(cols);
        let tile = tile_size(self.s.labels);
        let grid = vec2(
            cols as f32 * tile.x + (cols - 1) as f32 * GAP,
            rows as f32 * tile.y + (rows - 1) as f32 * GAP,
        );
        let origin = rect.center() - grid / 2.0;
        for (i, tool) in tools.into_iter().enumerate() {
            let (col, row) = ((i % cols) as f32, (i / cols) as f32);
            let r = Rect::from_min_size(
                origin + vec2(col * (tile.x + GAP), row * (tile.y + GAP)),
                tile,
            );
            let response = ui
                .interact(r, ui.scope_id().with(("outil", i)), Sense::click())
                .on_hover_text(tool.label())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let painter = ui.painter();
            let fill = if response.hovered() {
                theme::ACCENT
            } else {
                theme::SURFACE_HI
            };
            painter.rect_filled(r, CornerRadius::same(10), fill);
            let icon_y = if self.s.labels {
                r.top() + 22.0
            } else {
                r.center().y
            };
            painter.text(
                pos2(r.center().x, icon_y),
                Align2::CENTER_CENTER,
                tool.icon(),
                FontId::proportional(theme::SIZE_ICON),
                theme::on(fill),
            );
            if self.s.labels {
                painter.text(
                    pos2(r.center().x, r.bottom() - 6.0),
                    Align2::CENTER_BOTTOM,
                    widgets::truncate(tool.label(), 13),
                    FontId::proportional(theme::SIZE_XS),
                    theme::on(fill),
                );
            }
            if response.clicked() {
                match tool.action() {
                    Some(action) => cx.actions.push(action),
                    None if tool == Tool::Capture => self.capture(ui.ctx()),
                    None => {}
                }
            }
        }
    }

    fn settings_ui(&mut self, ui: &mut Ui, _cx: &mut WidgetCx) -> bool {
        let mut changed = false;
        let s = &mut self.s;
        changed |= widgets::setting_row(ui, icons::GRID, t!("widgets.tools_columns"), None, |ui| {
            ui.add(egui::DragValue::new(&mut s.columns).range(1..=8))
                .changed()
        });
        changed |= widgets::setting_row(ui, icons::TITLE, t!("widgets.tools_labels"), None, |ui| {
            widgets::toggle(ui, &mut s.labels).changed()
        });
        ui.add_space(6.0);
        widgets::section_title(ui, t!("widgets.tools_list"));
        for tool in Tool::ALL.into_iter().filter(|t| t.available()) {
            let mut on = s.tools.contains(&tool);
            let toggled = widgets::setting_row(ui, tool.icon(), tool.label(), None, |ui| {
                widgets::toggle(ui, &mut on).changed()
            });
            if toggled {
                if on {
                    // Garder l'ordre du catalogue.
                    s.tools.push(tool);
                    s.tools
                        .sort_by_key(|t| Tool::ALL.iter().position(|x| x == t));
                } else {
                    s.tools.retain(|t| *t != tool);
                }
                changed = true;
            }
        }
        ui.add_space(6.0);
        ui.label(t!("widgets.tools_snappy_path"));
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut s.snappy_path)
                    .hint_text(t!("widgets.tools_snappy_path_hint"))
                    .desired_width(f32::INFINITY),
            )
            .changed();
        changed
    }

    fn save(&self) -> serde_json::Value {
        serde_json::to_value(&self.s).unwrap_or_default()
    }

    fn repaint_after(&self) -> Option<Duration> {
        // Message passé : plus de redessin (une durée nulle redessinerait tout le
        // bureau à chaque image).
        let now = Instant::now();
        self.missing_until
            .filter(|until| *until > now)
            .map(|until| until - now)
    }

    fn preferred_size(&self) -> Option<[f32; 2]> {
        let n = self.shown().len().max(1);
        let cols = self.columns(n);
        let rows = n.div_ceil(cols);
        let tile = tile_size(self.s.labels);
        let w = cols as f32 * tile.x + (cols - 1) as f32 * GAP + 16.0;
        let h = rows as f32 * tile.y + (rows - 1) as f32 * GAP + 16.0;
        // Assez large pour le message « Snappy n'est pas installé ».
        Some([w.max(150.0), h.max(60.0)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_message_stops_repainting_once_over() {
        let mut w = QuickTools::new(ToolsSettings::default());
        assert_eq!(w.repaint_after(), None);
        w.missing_until = Some(Instant::now() + Duration::from_secs(8));
        assert!(w.repaint_after().is_some_and(|d| d > Duration::ZERO));
        w.missing_until = Instant::now().checked_sub(Duration::from_secs(1));
        assert_eq!(w.repaint_after(), None);
    }

    #[test]
    fn run_value_is_parsed() {
        let out = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    Snappy    REG_SZ    \"C:\\Outils\\snappy.exe\" --background\r\n\r\n";
        assert_eq!(
            parse_run_value(out),
            Some(PathBuf::from("C:\\Outils\\snappy.exe"))
        );
        let bare = "    Snappy    REG_SZ    C:\\Outils\\snappy.exe --background";
        assert_eq!(
            parse_run_value(bare),
            Some(PathBuf::from("C:\\Outils\\snappy.exe"))
        );
        assert_eq!(parse_run_value("ERREUR : valeur introuvable"), None);
    }

    #[test]
    fn custom_snappy_path_wins() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("snappy-maison");
        std::fs::write(&fake, "").unwrap();
        assert_eq!(find_snappy(fake.to_str().unwrap()), Some(fake));
    }

    #[test]
    fn grid_and_availability() {
        let mut w = QuickTools::new(ToolsSettings::default());
        // La capture est toujours proposée (Snappy, ou un message).
        assert!(Tool::Capture.available());
        let n = w.shown().len();
        assert!(n >= 1);
        w.s.columns = 2;
        let [width, height] = w.preferred_size().unwrap();
        assert!(
            height > width || n <= 2,
            "deux colonnes : plus haute que large"
        );
        w.s.tools.clear();
        assert!(w.shown().is_empty());
        let back: ToolsSettings = serde_json::from_value(w.save()).unwrap();
        assert_eq!(back, w.s);
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn system_tools_have_an_action() {
        for tool in [Tool::Calculator, Tool::Notepad, Tool::Lock, Tool::Terminal] {
            assert!(tool.action().is_some(), "{tool:?}");
        }
    }
}
