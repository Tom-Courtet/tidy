//! Implémentation macOS (AppKit et CoreGraphics via objc2).

mod click_through;
mod login;
mod screens;
mod window;

use std::path::Path;

use anyhow::{Context, Result};
use objc2_app_kit::NSEvent;
use objc2_foundation::NSLocale;
use raw_window_handle::HasWindowHandle;
use tidy_core::geometry::{Point, Rect};
use tidy_core::screen::Screen;

use crate::{DesktopPlatform, LoginState, SurfaceLevel};

pub struct MacPlatform;

impl DesktopPlatform for MacPlatform {
    fn screens(&self) -> Vec<Screen> {
        screens::list()
    }

    fn mouse_location(&self) -> Point {
        let p = NSEvent::mouseLocation();
        Point {
            x: p.x as f32,
            y: (screens::main_height() - p.y) as f32,
        }
    }

    fn make_desktop_surface(&self, window: &dyn HasWindowHandle, frame: Rect, level: SurfaceLevel) {
        window::make_surface(window, frame, level);
    }

    fn make_desktop_surface_titled(&self, title: &str, frame: Rect, level: SurfaceLevel) -> bool {
        window::make_surface_titled(title, frame, level)
    }

    fn set_interactive_regions(&self, title: &str, regions: Option<Vec<Rect>>) {
        click_through::set_regions(title, regions);
    }

    fn open(&self, path: &Path) -> Result<()> {
        run_open(&[path.as_os_str()])
    }

    fn reveal(&self, path: &Path) -> Result<()> {
        run_open(&["-R".as_ref(), path.as_os_str()])
    }

    fn preferred_languages(&self) -> Vec<String> {
        NSLocale::preferredLanguages()
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn login_item(&self) -> LoginState {
        login::state()
    }

    fn set_login_item(&self, enabled: bool) -> Result<LoginState> {
        login::set(enabled)
    }
}

/// `open` de macOS : app par défaut, ou Finder avec `-R`.
fn run_open(args: &[&std::ffi::OsStr]) -> Result<()> {
    let status = std::process::Command::new("/usr/bin/open")
        .args(args)
        .status()
        .context("lancement de open")?;
    anyhow::ensure!(status.success(), "open a échoué ({status})");
    Ok(())
}
