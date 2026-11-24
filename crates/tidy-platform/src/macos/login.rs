//! Lancement à l'ouverture de session : `SMAppService.mainApp` (macOS 13+), élément
//! d'ouverture de Réglages Système > Général > Ouverture. Repris de Snappy.
use anyhow::{Result, bail};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

use crate::LoginState as State;

/// Lancé depuis `Tidy.app` ?
fn in_bundle() -> bool {
    std::env::current_exe()
        .map(|exe| exe.to_string_lossy().contains(".app/Contents/MacOS/"))
        .unwrap_or(false)
}

fn not_bundled() -> State {
    State::Unavailable(
        "Disponible seulement pour Tidy.app (pas pour un exécutable lancé à la main).".into(),
    )
}

pub fn state() -> State {
    if !in_bundle() {
        return not_bundled();
    }
    // SAFETY : SMAppService existe depuis macOS 13 (Tidy demande macOS 14).
    let status = unsafe { SMAppService::mainAppService().status() };
    match status {
        SMAppServiceStatus::Enabled => State::On,
        SMAppServiceStatus::RequiresApproval => State::NeedsApproval,
        _ => State::Off,
    }
}

pub fn set(enabled: bool) -> Result<State> {
    if !in_bundle() {
        return Ok(not_bundled());
    }
    // SAFETY : voir `state`.
    let service = unsafe { SMAppService::mainAppService() };
    let result = if enabled {
        unsafe { service.registerAndReturnError() }
    } else {
        unsafe { service.unregisterAndReturnError() }
    };
    if let Err(e) = result {
        // Déjà dans l'état voulu : pas une erreur.
        let now = state();
        if (now == State::On) == enabled && now != State::NeedsApproval {
            return Ok(now);
        }
        bail!("{}", e.localizedDescription());
    }
    Ok(state())
}
