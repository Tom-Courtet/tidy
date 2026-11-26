//! Fonds d'écran sous macOS (lot J).
//!
//! - **Fonds fixes** : `NSWorkspace setDesktopImageURL:forScreen:options:error:`, un par
//!   écran ; l'ajustement passe par les options (mise à l'échelle, rognage, couleur
//!   de remplissage). AppKit demande le thread principal : depuis un autre fil (agent),
//!   l'appel est confié à la file principale et attendu ([`on_main`]).
//! - **Fonds animés** : la fenêtre se pose au niveau du fond d'écran
//!   (`kCGDesktopWindowLevel`, implémentation par défaut du trait).
//! - **Pause intelligente** : fenêtre d'une autre appli qui couvre tout un écran
//!   (`CGWindowListCopyWindowInfo`), session verrouillée (`CGSessionCopyCurrentDictionary`),
//!   batterie (`pmset -g batt`, relu au plus toutes les 30 s).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSColor, NSColorSpace, NSScreen, NSWorkspace, NSWorkspaceDesktopImageAllowClippingKey,
    NSWorkspaceDesktopImageFillColorKey, NSWorkspaceDesktopImageOptionKey,
    NSWorkspaceDesktopImageScalingKey,
};
use objc2_core_foundation::CFArray;
use objc2_core_graphics::{
    CGSessionCopyCurrentDictionary, CGWindowListCopyWindowInfo, CGWindowListOption,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSNumber, NSOperationQueue, NSString, NSURL, ns_string,
};
use tidy_core::geometry::Rect;
use tidy_core::screen::ScreenId;
use tidy_core::settings::wallpaper::{Fit, OriginalScreen, OriginalWallpaper};

use super::screens;
use crate::SystemActivity;

/// Délai maximal d'attente de la file principale.
const MAIN_TIMEOUT: Duration = Duration::from_secs(10);

/// Exécute `f` sur le thread principal (tout de suite si on y est) et rend son
/// résultat ; `None` si la file principale ne répond pas à temps.
fn on_main<R: Send + 'static>(f: impl FnOnce(MainThreadMarker) -> R + Send + 'static) -> Option<R> {
    if let Some(mtm) = MainThreadMarker::new() {
        return Some(f(mtm));
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let job = Mutex::new(Some(f));
    let block = RcBlock::new(move || {
        let Some(f) = job.lock().ok().and_then(|mut j| j.take()) else {
            return;
        };
        if let Some(mtm) = MainThreadMarker::new() {
            let _ = tx.send(f(mtm));
        }
    });
    // SAFETY: le bloc ne capture que des valeurs `Send` (la tâche et l'émetteur) ; il
    // s'exécute une fois sur la file principale.
    unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&block) };
    rx.recv_timeout(MAIN_TIMEOUT).ok()
}

/// Valeurs `NSImageScaling` d'AppKit.
const SCALE_AXES_INDEPENDENTLY: isize = 1;
const SCALE_NONE: isize = 2;
const SCALE_PROPORTIONALLY_UP_OR_DOWN: isize = 3;

/// Identifiant de Tidy de chaque `NSScreen` (même cadre que la liste des écrans).
fn screen_ids(mtm: MainThreadMarker) -> Vec<(ScreenId, Retained<NSScreen>)> {
    let known = screens::list();
    NSScreen::screens(mtm)
        .iter()
        .filter_map(|ns| {
            let frame = screens::from_appkit(ns.frame());
            let close = |a: Rect, b: Rect| {
                (a.x - b.x).abs() < 1.0
                    && (a.y - b.y).abs() < 1.0
                    && (a.width - b.width).abs() < 1.0
                    && (a.height - b.height).abs() < 1.0
            };
            let s = known.iter().find(|s| close(s.frame, frame))?;
            Some((s.id.clone(), ns))
        })
        .collect()
}

fn options(
    fit: Fit,
    background: [u8; 3],
) -> Retained<NSDictionary<NSWorkspaceDesktopImageOptionKey, AnyObject>> {
    let (scaling, clipping) = match fit {
        Fit::Fill => (SCALE_PROPORTIONALLY_UP_OR_DOWN, true),
        Fit::Fit => (SCALE_PROPORTIONALLY_UP_OR_DOWN, false),
        Fit::Stretch => (SCALE_AXES_INDEPENDENTLY, false),
        // macOS ne propose pas la mosaïque par cette API : image centrée.
        Fit::Center | Fit::Tile => (SCALE_NONE, false),
    };
    let [r, g, b] = background.map(|c| f64::from(c) / 255.0);
    let scaling = NSNumber::new_isize(scaling);
    let clipping = NSNumber::new_bool(clipping);
    let color = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0);
    // SAFETY: clés d'AppKit, toujours définies.
    let keys = unsafe {
        [
            NSWorkspaceDesktopImageScalingKey,
            NSWorkspaceDesktopImageAllowClippingKey,
            NSWorkspaceDesktopImageFillColorKey,
        ]
    };
    let values: [&AnyObject; 3] = [&scaling, &clipping, &color];
    NSDictionary::from_slices(&keys, &values)
}

fn fit_from(
    options: &NSDictionary<NSWorkspaceDesktopImageOptionKey, AnyObject>,
) -> (Option<Fit>, Option<[u8; 3]>) {
    // SAFETY: clés d'AppKit, toujours définies.
    let (scaling_key, clipping_key, color_key) = unsafe {
        (
            NSWorkspaceDesktopImageScalingKey,
            NSWorkspaceDesktopImageAllowClippingKey,
            NSWorkspaceDesktopImageFillColorKey,
        )
    };
    let number = |key| {
        options
            .objectForKey(key)
            .and_then(|o| o.downcast::<NSNumber>().ok())
    };
    let scaling = number(scaling_key).map(|n| n.integerValue());
    let clipping = number(clipping_key).is_some_and(|n| n.boolValue());
    let fit = scaling.map(|s| match s {
        SCALE_AXES_INDEPENDENTLY => Fit::Stretch,
        SCALE_NONE => Fit::Center,
        _ if clipping => Fit::Fill,
        _ => Fit::Fit,
    });
    let color = options
        .objectForKey(color_key)
        .and_then(|o| o.downcast::<NSColor>().ok())
        .and_then(|c| c.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()))
        .map(|c| {
            [c.redComponent(), c.greenComponent(), c.blueComponent()]
                .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        });
    (fit, color)
}

fn set_on(
    workspace: &NSWorkspace,
    screen: &NSScreen,
    image: &Path,
    fit: Fit,
    background: [u8; 3],
) -> Result<()> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&image.to_string_lossy()));
    let options = options(fit, background);
    // SAFETY: dictionnaire d'options aux types attendus par AppKit ; thread principal.
    unsafe { workspace.setDesktopImageURL_forScreen_options_error(&url, screen, &options) }
        .map_err(|e| anyhow::anyhow!("{}", e.localizedDescription()))
}

pub fn snapshot() -> Option<OriginalWallpaper> {
    on_main(|mtm| {
        let workspace = NSWorkspace::sharedWorkspace();
        let screens = screen_ids(mtm)
            .into_iter()
            .map(|(id, ns)| {
                let image = workspace
                    .desktopImageURLForScreen(&ns)
                    .and_then(|u| u.path())
                    .map(|p| PathBuf::from(p.to_string()));
                let (fit, background) = workspace
                    .desktopImageOptionsForScreen(&ns)
                    .map_or((None, None), |o| fit_from(&o));
                OriginalScreen {
                    screen: id.0,
                    image,
                    fit,
                    background,
                }
            })
            .collect();
        OriginalWallpaper {
            screens,
            ..Default::default()
        }
    })
}

pub fn set(screen: Option<&ScreenId>, image: &Path, fit: Fit, background: [u8; 3]) -> Result<()> {
    if !image.is_file() {
        bail!("image introuvable : {}", image.display());
    }
    let screen = screen.cloned();
    let image = image.to_owned();
    on_main(move |mtm| {
        let workspace = NSWorkspace::sharedWorkspace();
        let targets: Vec<_> = screen_ids(mtm)
            .into_iter()
            .filter(|(id, _)| screen.as_ref().is_none_or(|s| s == id))
            .collect();
        if targets.is_empty() {
            bail!("écran introuvable");
        }
        for (_, ns) in targets {
            set_on(&workspace, &ns, &image, fit, background)?;
        }
        Ok(())
    })
    .context("AppKit ne répond pas")?
}

pub fn restore(original: &OriginalWallpaper) -> Result<()> {
    let original = original.clone();
    on_main(move |mtm| {
        let workspace = NSWorkspace::sharedWorkspace();
        let screens = screen_ids(mtm);
        let first = original.screens.iter().find(|s| s.image.is_some());
        for (id, ns) in &screens {
            // L'écran d'origine, sinon (écran branché depuis) le premier fond relevé.
            let Some(s) = original.screens.iter().find(|s| s.screen == id.0).or(first) else {
                continue;
            };
            let Some(image) = &s.image else {
                continue;
            };
            let fit = s.fit.unwrap_or_default();
            let background = s.background.unwrap_or([0, 0, 0]);
            if let Err(e) = set_on(&workspace, ns, image, fit, background) {
                log::warn!("fond d'origine de {} : {e:#}", id.0);
            }
        }
        Ok(())
    })
    .context("AppKit ne répond pas")?
}

// --- Pause intelligente -----------------------------------------------------------

/// Une fenêtre d'une autre appli couvre-t-elle tout un écran ?
fn fullscreen_window() -> bool {
    let option =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(list) = CGWindowListCopyWindowInfo(option, 0) else {
        return false;
    };
    let frames: Vec<Rect> = screens::list().into_iter().map(|s| s.frame).collect();
    let own = std::process::id() as i64;
    // SAFETY: CFArray et NSArray sont interchangeables (« toll-free bridging ») ; les
    // éléments sont des dictionnaires de clés textuelles ; `list` vit jusqu'à la fin.
    let windows: &NSArray<NSDictionary<NSString, AnyObject>> =
        unsafe { &*(&*list as *const CFArray).cast() };
    windows.iter().any(|w| {
        let number = |key: &NSString| {
            w.objectForKey(key)
                .and_then(|o| o.downcast::<NSNumber>().ok())
        };
        let layer = number(ns_string!("kCGWindowLayer")).map(|n| n.integerValue());
        let pid = number(ns_string!("kCGWindowOwnerPID")).map(|n| n.longLongValue());
        if layer != Some(0) || pid == Some(own) {
            return false;
        }
        let Some(bounds) = w
            .objectForKey(ns_string!("kCGWindowBounds"))
            .and_then(|o| o.downcast::<NSDictionary>().ok())
        else {
            return false;
        };
        // SAFETY: le dictionnaire des bornes a des clés textuelles et des nombres.
        let bounds: &NSDictionary<NSString, AnyObject> =
            unsafe { &*(&*bounds as *const NSDictionary).cast() };
        let value = |key: &NSString| {
            bounds
                .objectForKey(key)
                .and_then(|o| o.downcast::<NSNumber>().ok())
                .map_or(0.0, |n| n.doubleValue() as f32)
        };
        let (x, y) = (value(ns_string!("X")), value(ns_string!("Y")));
        let (width, height) = (value(ns_string!("Width")), value(ns_string!("Height")));
        frames.iter().any(|f| {
            x <= f.x + 1.0
                && y <= f.y + 1.0
                && x + width >= f.right() - 1.0
                && y + height >= f.bottom() - 1.0
        })
    })
}

fn locked() -> bool {
    let Some(dict) = CGSessionCopyCurrentDictionary() else {
        return false;
    };
    // SAFETY: CFDictionary et NSDictionary sont interchangeables ; clés textuelles.
    let dict: &NSDictionary<NSString, AnyObject> =
        unsafe { &*(&*dict as *const objc2_core_foundation::CFDictionary).cast() };
    dict.objectForKey(ns_string!("CGSSessionScreenIsLocked"))
        .and_then(|o| o.downcast::<NSNumber>().ok())
        .is_some_and(|n| n.boolValue())
}

/// Lit la sortie de `pmset -g batt` : (sur batterie, charge en %).
pub(crate) fn parse_pmset(text: &str) -> Battery {
    let on_battery = text.contains("'Battery Power'");
    let percent = text
        .split(|c: char| c.is_whitespace() || c == ';')
        .find_map(|word| word.strip_suffix('%')?.parse::<u8>().ok());
    (on_battery, percent)
}

/// (sur batterie, charge en %).
type Battery = (bool, Option<u8>);

static BATTERY: Mutex<Option<(Instant, Battery)>> = Mutex::new(None);
const BATTERY_EVERY: Duration = Duration::from_secs(30);

fn battery() -> Battery {
    let mut cache = BATTERY.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((at, value)) = *cache
        && at.elapsed() < BATTERY_EVERY
    {
        return value;
    }
    let value = std::process::Command::new("/usr/bin/pmset")
        .args(["-g", "batt"])
        .output()
        .ok()
        .map(|o| parse_pmset(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or((false, None));
    *cache = Some((Instant::now(), value));
    value
}

pub fn activity() -> SystemActivity {
    let (on_battery, battery_percent) = battery();
    SystemActivity {
        fullscreen: fullscreen_window(),
        on_battery,
        battery_percent,
        locked: locked(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmset_output_is_read() {
        let battery = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=123)\t83%; discharging; 4:12 remaining present: true\n";
        assert_eq!(parse_pmset(battery), (true, Some(83)));
        let ac = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t100%; charged; 0:00 remaining present: true\n";
        assert_eq!(parse_pmset(ac), (false, Some(100)));
        assert_eq!(parse_pmset("Now drawing from 'AC Power'\n"), (false, None));
    }

    /// Lecture seule : ne change rien au fond d'écran.
    #[test]
    fn activity_can_be_read() {
        let _ = activity();
    }
}
