//! Widgets posés sur le bureau (lots H et I) : une fenêtre par instance de
//! `DesktopState::widgets`, au niveau des boîtes, avec un cadre commun (déplacer,
//! geler, masquer avec le bureau, réglages, supprimer). Le contenu vient du registre de
//! `tidy-widgets`.
//!
//! Le processus du bureau reste le seul écrivain de `desktop.json` : l'hôte rend des
//! [`DesktopCommand`] (position, réglages, suppression), appliquées par `Desktop`.

use tidy_core::screen::Screen;
use tidy_core::{DesktopCommand, DesktopState, Store};

/// Hôte des widgets du processus du bureau.
pub struct WidgetHost {
    store: Store,
}

impl WidgetHost {
    pub fn new(store: &Store) -> Self {
        Self {
            store: store.clone(),
        }
    }

    /// Dessine une fenêtre par widget de `state` (créée au premier passage, oubliée
    /// quand le widget disparaît). `hidden` : bureau masqué (sauf widgets exclus).
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        state: &DesktopState,
        screens: &[Screen],
        hidden: bool,
        out: &mut Vec<DesktopCommand>,
    ) {
        let _ = (&self.store, ctx, state, screens, hidden, out);
    }
}
