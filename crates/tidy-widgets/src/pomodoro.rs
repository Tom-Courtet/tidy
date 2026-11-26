//! iPomodoro : minuteur de concentration. Périodes de travail et pauses (courte, puis
//! longue après un nombre de cycles), durées réglables, enchaînement automatique ou
//! non, notification (avec ou sans son) en fin de période, mode concentration qui
//! masque le bureau pendant le travail.
//!
//! La logique ([`Cycle`]) est pure et testée. Un petit fil réveille le bureau à la fin
//! de chaque période, même quand toutes ses fenêtres sont masquées : la notification
//! part à l'heure.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use egui::{Align2, CornerRadius, FontId, Rect, Ui, ViewportCommand, ViewportId, pos2, vec2};
use serde::{Deserialize, Serialize};
use tidy_ui::{icons, t, theme, widgets};

use crate::{Widget, WidgetAction, WidgetCx, WidgetKind};

pub fn kind() -> WidgetKind {
    WidgetKind {
        id: "pomodoro",
        name_key: "widgets.pomodoro_name",
        icon: icons::TIMER,
        default_size: [230.0, 160.0],
        create: |v| {
            Box::new(Pomodoro::new(
                serde_json::from_value(v.clone()).unwrap_or_default(),
            ))
        },
    }
}

/// Réglages d'un pomodoro (durées en minutes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PomodoroSettings {
    pub work_min: u32,
    pub short_min: u32,
    pub long_min: u32,
    /// Périodes de travail avant une pause longue.
    pub cycles: u32,
    /// Enchaîner la période suivante sans attendre un clic.
    pub auto_continue: bool,
    pub notify: bool,
    pub sound: bool,
    /// Masquer le bureau pendant le travail.
    pub focus_mode: bool,
    /// Grande taille.
    pub large: bool,
}

impl Default for PomodoroSettings {
    fn default() -> Self {
        Self {
            work_min: 25,
            short_min: 5,
            long_min: 15,
            cycles: 4,
            auto_continue: false,
            notify: true,
            sound: true,
            focus_mode: false,
            large: false,
        }
    }
}

impl PomodoroSettings {
    /// Durée d'une période (au moins une minute, au plus trois heures).
    pub fn duration(&self, phase: Phase) -> Duration {
        let min = match phase {
            Phase::Work => self.work_min,
            Phase::ShortBreak => self.short_min,
            Phase::LongBreak => self.long_min,
        };
        Duration::from_secs(u64::from(min.clamp(1, 180)) * 60)
    }

    fn cycles(&self) -> u32 {
        self.cycles.clamp(1, 12)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Work,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Work => t!("widgets.pomodoro_work"),
            Phase::ShortBreak => t!("widgets.pomodoro_short"),
            Phase::LongBreak => t!("widgets.pomodoro_long"),
        }
    }
}

/// État du minuteur.
#[derive(Debug, Clone, PartialEq)]
pub struct Cycle {
    pub phase: Phase,
    /// Périodes de travail terminées dans la série en cours.
    pub done: u32,
    /// Fin prévue, quand le minuteur tourne.
    deadline: Option<Instant>,
    /// Temps restant, quand il est arrêté.
    left: Duration,
}

impl Cycle {
    pub fn new(s: &PomodoroSettings) -> Self {
        Self {
            phase: Phase::Work,
            done: 0,
            deadline: None,
            left: s.duration(Phase::Work),
        }
    }

    pub fn running(&self) -> bool {
        self.deadline.is_some()
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        match self.deadline {
            Some(d) => d.saturating_duration_since(now),
            None => self.left,
        }
    }

    pub fn start(&mut self, now: Instant) {
        if self.deadline.is_none() {
            self.deadline = Some(now + self.left);
        }
    }

    pub fn pause(&mut self, now: Instant) {
        if let Some(d) = self.deadline.take() {
            self.left = d.saturating_duration_since(now);
        }
    }

    /// Revient au début d'une série, arrêté.
    pub fn reset(&mut self, s: &PomodoroSettings) {
        *self = Self::new(s);
    }

    /// Période qui suit la période en cours.
    pub fn next_phase(&self, s: &PomodoroSettings) -> Phase {
        match self.phase {
            Phase::Work if (self.done + 1).is_multiple_of(s.cycles()) => Phase::LongBreak,
            Phase::Work => Phase::ShortBreak,
            _ => Phase::Work,
        }
    }

    /// Passe à la période suivante : elle démarre tout de suite si `run`.
    fn advance(&mut self, s: &PomodoroSettings, now: Instant, run: bool) {
        let next = self.next_phase(s);
        match self.phase {
            Phase::Work => self.done += 1,
            Phase::LongBreak => self.done = 0,
            Phase::ShortBreak => {}
        }
        self.phase = next;
        self.left = s.duration(next);
        self.deadline = run.then(|| now + self.left);
    }

    /// Passer la période en cours (sans notification). Le minuteur garde son état.
    pub fn skip(&mut self, s: &PomodoroSettings, now: Instant) {
        let run = self.running();
        self.advance(s, now, run);
    }

    /// Fin de période atteinte ? Rend la période terminée ; la suivante démarre seule
    /// si l'enchaînement automatique est activé.
    pub fn tick(&mut self, s: &PomodoroSettings, now: Instant) -> Option<Phase> {
        let deadline = self.deadline?;
        if now < deadline {
            return None;
        }
        let finished = self.phase;
        self.advance(s, now, s.auto_continue);
        Some(finished)
    }

    /// Fraction écoulée de la période en cours (0 à 1).
    pub fn progress(&self, s: &PomodoroSettings, now: Instant) -> f32 {
        let total = s.duration(self.phase).as_secs_f32();
        (1.0 - self.remaining(now).as_secs_f32() / total).clamp(0.0, 1.0)
    }
}

/// Temps restant affiché (`24:59`), arrondi à la seconde supérieure.
pub fn format_remaining(d: Duration) -> String {
    let secs = d.as_millis().div_ceil(1000) as u64;
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

pub struct Pomodoro {
    s: PomodoroSettings,
    cycle: Cycle,
    /// Bureau masqué par le mode concentration.
    focus_active: bool,
    /// Génération du réveil en cours : un réveil dépassé ne fait rien.
    wake: Arc<AtomicU64>,
    /// Fin de période pour laquelle un réveil est prévu.
    wake_for: Option<Instant>,
}

impl Pomodoro {
    pub fn new(s: PomodoroSettings) -> Self {
        Self {
            cycle: Cycle::new(&s),
            s,
            focus_active: false,
            wake: Arc::default(),
            wake_for: None,
        }
    }

    /// Prévoit un réveil du bureau à la fin de la période (fenêtres masquées ou non).
    fn schedule_wake(&mut self, ctx: &egui::Context) {
        let deadline = self.cycle.deadline;
        if deadline == self.wake_for {
            return;
        }
        self.wake_for = deadline;
        let generation = self.wake.fetch_add(1, Ordering::SeqCst) + 1;
        let Some(deadline) = deadline else {
            return;
        };
        let wake = Arc::clone(&self.wake);
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("tidy-pomodoro".into())
            .spawn(move || {
                std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                if wake.load(Ordering::SeqCst) == generation {
                    ctx.send_viewport_cmd_to(
                        ViewportId::ROOT,
                        ViewportCommand::RequestPaintWhileHidden,
                    );
                    ctx.request_repaint();
                }
            });
        if let Err(e) = spawned {
            log::warn!("réveil du pomodoro : {e}");
        }
    }

    fn finished(&mut self, phase: Phase, cx: &mut WidgetCx) {
        if !self.s.notify {
            return;
        }
        let next = self.cycle.phase;
        let minutes = self.s.duration(next).as_secs() / 60;
        let (title, body) = match phase {
            Phase::Work => (
                t!("widgets.pomodoro_done_work"),
                t!(
                    "widgets.pomodoro_done_work_body",
                    pause = next.label(),
                    minutes = minutes
                ),
            ),
            _ => (
                t!("widgets.pomodoro_done_break"),
                t!("widgets.pomodoro_done_break_body", minutes = minutes),
            ),
        };
        cx.actions.push(WidgetAction::Alert {
            title: title.to_owned(),
            body,
            sound: self.s.sound,
        });
    }

    /// Mode concentration : bureau masqué pendant le travail, rendu après.
    fn sync_focus(&mut self, cx: &mut WidgetCx) {
        let want = self.s.focus_mode && self.cycle.running() && self.cycle.phase == Phase::Work;
        if want != self.focus_active {
            self.focus_active = want;
            cx.actions.push(WidgetAction::HideDesktop(want));
        }
    }
}

impl Widget for Pomodoro {
    fn ui(&mut self, ui: &mut Ui, cx: &mut WidgetCx) {
        let now = Instant::now();
        if let Some(finished) = self.cycle.tick(&self.s, now) {
            self.finished(finished, cx);
        }
        let rect = ui.max_rect();
        let k = if self.s.large { 1.4 } else { 1.0 };
        let painter = ui.painter().clone();
        let color = match self.cycle.phase {
            Phase::Work => theme::ACCENT_2,
            _ => theme::ACCENT,
        };

        // Période et cycles.
        let top = rect.top() + 4.0;
        painter.text(
            pos2(rect.left() + 8.0, top),
            Align2::LEFT_TOP,
            self.cycle.phase.label(),
            FontId::proportional(theme::SIZE_S * k),
            color,
        );
        let cycles = self.s.cycles();
        let dot = 6.0 * k;
        for i in 0..cycles {
            let x = rect.right() - 8.0 - (cycles - i) as f32 * (dot + 4.0) + dot / 2.0;
            let c = pos2(x, top + 7.0 * k);
            if i < self.cycle.done {
                painter.circle_filled(c, dot / 2.0, theme::ACCENT_2);
            } else {
                painter.circle_stroke(c, dot / 2.0, egui::Stroke::new(1.0, theme::TEXT_DIM));
            }
        }

        // Temps restant et avancement.
        let remaining = self.cycle.remaining(now);
        let time_y = top + 22.0 * k;
        painter.text(
            pos2(rect.center().x, time_y),
            Align2::CENTER_TOP,
            format_remaining(remaining),
            theme::display(34.0 * k),
            theme::TEXT,
        );
        let bar_y = time_y + 48.0 * k;
        let bar = Rect::from_min_size(
            pos2(rect.left() + 12.0, bar_y),
            vec2(rect.width() - 24.0, 4.0),
        );
        painter.rect_filled(bar, CornerRadius::same(2), theme::SURFACE_HI);
        let mut filled = bar;
        filled.set_width(bar.width() * self.cycle.progress(&self.s, now));
        painter.rect_filled(filled, CornerRadius::same(2), color);

        // Boutons, centrés sous la barre.
        let size = theme::SIZE_L * k;
        let cell = size + 18.0;
        let row_y = bar_y + 12.0 + cell / 2.0;
        let running = self.cycle.running();
        let (play, play_tip) = if running {
            (icons::PAUSE, t!("widgets.pomodoro_pause"))
        } else {
            (icons::PLAY, t!("widgets.pomodoro_start"))
        };
        let buttons = [
            (play, play_tip),
            (icons::RESTORE, t!("widgets.pomodoro_reset")),
            (icons::NEXT, t!("widgets.pomodoro_skip")),
        ];
        let left = rect.center().x - cell * buttons.len() as f32 / 2.0;
        let mut clicked = None;
        for (i, (icon, tip)) in buttons.into_iter().enumerate() {
            let r = Rect::from_center_size(
                pos2(left + cell * (i as f32 + 0.5), row_y),
                vec2(cell, cell),
            );
            let response = ui
                .interact(r, ui.scope_id().with(("pomodoro", i)), egui::Sense::click())
                .on_hover_text(tip)
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.hovered() {
                painter.rect_filled(r.shrink(2.0), CornerRadius::same(8), theme::SURFACE_HI);
            }
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                icon,
                FontId::proportional(size),
                if i == 0 { color } else { theme::TEXT },
            );
            if response.clicked() {
                clicked = Some(i);
            }
        }
        match clicked {
            Some(0) if running => self.cycle.pause(now),
            Some(0) => self.cycle.start(now),
            Some(1) => self.cycle.reset(&self.s),
            Some(2) => self.cycle.skip(&self.s, now),
            _ => {}
        }

        self.sync_focus(cx);
        let ctx = ui.ctx().clone();
        self.schedule_wake(&ctx);
    }

    fn settings_ui(&mut self, ui: &mut Ui, _cx: &mut WidgetCx) -> bool {
        let mut changed = false;
        let s = &mut self.s;
        for (value, label, max) in [
            (&mut s.work_min, t!("widgets.pomodoro_work_min"), 180),
            (&mut s.short_min, t!("widgets.pomodoro_short_min"), 180),
            (&mut s.long_min, t!("widgets.pomodoro_long_min"), 180),
            (&mut s.cycles, t!("widgets.pomodoro_cycles"), 12),
        ] {
            changed |= widgets::setting_row(ui, "", label, None, |ui| {
                ui.add(egui::DragValue::new(value).range(1..=max)).changed()
            });
        }
        ui.add_space(4.0);
        for (value, label, hint) in [
            (
                &mut s.auto_continue,
                t!("widgets.pomodoro_auto"),
                Some(t!("widgets.pomodoro_auto_hint")),
            ),
            (&mut s.notify, t!("widgets.pomodoro_notify"), None),
            (&mut s.sound, t!("widgets.pomodoro_sound"), None),
            (
                &mut s.focus_mode,
                t!("widgets.pomodoro_focus"),
                Some(t!("widgets.pomodoro_focus_hint")),
            ),
            (&mut s.large, t!("widgets.pomodoro_large"), None),
        ] {
            changed |= widgets::setting_row(ui, "", label, hint, |ui| {
                widgets::toggle(ui, value).changed()
            });
        }
        // Durées changées : un minuteur arrêté reprend la nouvelle durée.
        if changed && !self.cycle.running() {
            self.cycle.left = self.s.duration(self.cycle.phase);
        }
        changed
    }

    fn save(&self) -> serde_json::Value {
        serde_json::to_value(&self.s).unwrap_or_default()
    }

    fn repaint_after(&self) -> Option<Duration> {
        let now = Instant::now();
        self.cycle.running().then(|| {
            let ms = self.cycle.remaining(now).as_millis() % 1000;
            Duration::from_millis(ms as u64 + 10)
        })
    }

    fn preferred_size(&self) -> Option<[f32; 2]> {
        Some(if self.s.large {
            [300.0, 190.0]
        } else {
            [220.0, 140.0]
        })
    }

    fn keep_visible(&self) -> bool {
        self.focus_active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quick() -> PomodoroSettings {
        PomodoroSettings {
            work_min: 25,
            short_min: 5,
            long_min: 15,
            cycles: 2,
            ..Default::default()
        }
    }

    #[test]
    fn full_series_with_a_long_break() {
        let s = quick();
        let t0 = Instant::now();
        let mut c = Cycle::new(&s);
        assert!(!c.running());
        assert_eq!(c.remaining(t0), Duration::from_secs(25 * 60));
        assert_eq!(c.tick(&s, t0), None, "arrêté : rien ne se passe");
        c.start(t0);
        assert_eq!(c.tick(&s, t0 + Duration::from_secs(60)), None);
        assert_eq!(
            c.remaining(t0 + Duration::from_secs(60)),
            Duration::from_secs(24 * 60)
        );
        // Fin du premier travail : pause courte, arrêtée (pas d'enchaînement).
        let t1 = t0 + Duration::from_secs(25 * 60);
        assert_eq!(c.tick(&s, t1), Some(Phase::Work));
        assert_eq!(
            (c.phase, c.done, c.running()),
            (Phase::ShortBreak, 1, false)
        );
        assert_eq!(c.remaining(t1), Duration::from_secs(5 * 60));
        c.start(t1);
        let t2 = t1 + Duration::from_secs(5 * 60);
        assert_eq!(c.tick(&s, t2), Some(Phase::ShortBreak));
        assert_eq!(c.phase, Phase::Work);
        c.start(t2);
        // Deuxième travail (cycles = 2) : pause longue.
        let t3 = t2 + Duration::from_secs(25 * 60);
        assert_eq!(c.tick(&s, t3), Some(Phase::Work));
        assert_eq!((c.phase, c.done), (Phase::LongBreak, 2));
        c.start(t3);
        assert_eq!(
            c.tick(&s, t3 + Duration::from_secs(15 * 60)),
            Some(Phase::LongBreak)
        );
        assert_eq!((c.phase, c.done), (Phase::Work, 0), "nouvelle série");
    }

    #[test]
    fn auto_continue_starts_the_next_period() {
        let s = PomodoroSettings {
            auto_continue: true,
            ..quick()
        };
        let t0 = Instant::now();
        let mut c = Cycle::new(&s);
        c.start(t0);
        let t1 = t0 + Duration::from_secs(25 * 60 + 2);
        assert_eq!(c.tick(&s, t1), Some(Phase::Work));
        assert!(c.running());
        assert_eq!(c.remaining(t1), Duration::from_secs(5 * 60));
    }

    #[test]
    fn pause_skip_and_reset() {
        let s = quick();
        let t0 = Instant::now();
        let mut c = Cycle::new(&s);
        c.start(t0);
        c.pause(t0 + Duration::from_secs(100));
        assert!(!c.running());
        // En pause, le temps ne passe pas.
        assert_eq!(
            c.remaining(t0 + Duration::from_secs(10_000)),
            Duration::from_secs(25 * 60 - 100)
        );
        assert!((c.progress(&s, t0) - 100.0 / 1500.0).abs() < 1e-4);
        c.skip(&s, t0);
        assert_eq!(
            (c.phase, c.done, c.running()),
            (Phase::ShortBreak, 1, false)
        );
        c.start(t0);
        c.skip(&s, t0);
        assert_eq!(c.phase, Phase::Work);
        assert!(c.running(), "passer garde le minuteur en marche");
        c.reset(&s);
        assert_eq!(c, Cycle::new(&s));
    }

    #[test]
    fn durations_are_clamped() {
        let s = PomodoroSettings {
            work_min: 0,
            long_min: 999,
            cycles: 0,
            ..Default::default()
        };
        assert_eq!(s.duration(Phase::Work), Duration::from_secs(60));
        assert_eq!(s.duration(Phase::LongBreak), Duration::from_secs(180 * 60));
        let c = Cycle::new(&s);
        assert_eq!(c.next_phase(&s), Phase::LongBreak, "un cycle par série");
    }

    #[test]
    fn remaining_is_rounded_up() {
        assert_eq!(format_remaining(Duration::from_secs(25 * 60)), "25:00");
        assert_eq!(format_remaining(Duration::from_millis(59_001)), "01:00");
        assert_eq!(format_remaining(Duration::from_millis(400)), "00:01");
        assert_eq!(format_remaining(Duration::ZERO), "00:00");
    }

    #[test]
    fn focus_mode_hides_then_shows_the_desktop() {
        let mut p = Pomodoro::new(PomodoroSettings {
            focus_mode: true,
            ..quick()
        });
        let mut cx = WidgetCx::new(std::env::temp_dir());
        p.sync_focus(&mut cx);
        assert!(cx.actions.is_empty(), "arrêté : rien");
        p.cycle.start(Instant::now());
        p.sync_focus(&mut cx);
        assert_eq!(cx.actions, [WidgetAction::HideDesktop(true)]);
        assert!(p.keep_visible());
        p.cycle.pause(Instant::now());
        p.sync_focus(&mut cx);
        assert_eq!(cx.actions[1], WidgetAction::HideDesktop(false));
        assert!(!p.keep_visible());
    }

    #[test]
    fn end_of_work_sends_a_notification() {
        let mut p = Pomodoro::new(quick());
        let mut cx = WidgetCx::new(std::env::temp_dir());
        let t0 = Instant::now();
        p.cycle.start(t0);
        let finished = p
            .cycle
            .tick(&p.s, t0 + Duration::from_secs(25 * 60))
            .unwrap();
        p.finished(finished, &mut cx);
        assert!(matches!(
            &cx.actions[..],
            [WidgetAction::Alert { sound: true, .. }]
        ));
    }
}
