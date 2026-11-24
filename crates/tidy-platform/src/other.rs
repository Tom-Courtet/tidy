//! Autres OS (Linux…) : implémentation vide, qui compile et répond poliment.

use std::path::Path;

use anyhow::{Result, bail};
use raw_window_handle::HasWindowHandle;
use tidy_core::geometry::{Point, Rect};
use tidy_core::screen::{Screen, ScreenId};

use crate::{DesktopPlatform, LoginState, SurfaceLevel};

pub struct OtherPlatform;

const NOT_YET: &str = "pas encore disponible sur cet OS";

impl DesktopPlatform for OtherPlatform {
    fn screens(&self) -> Vec<Screen> {
        let frame = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        vec![Screen {
            id: ScreenId("principal".into()),
            name: "Écran".into(),
            frame,
            visible_frame: frame,
            scale: 1.0,
            primary: true,
        }]
    }

    fn mouse_location(&self) -> Point {
        Point::default()
    }

    fn make_desktop_surface(&self, _: &dyn HasWindowHandle, _: Rect, _: SurfaceLevel) {
        log::warn!("surface du bureau : {NOT_YET}");
    }

    fn make_desktop_surface_titled(&self, _: &str, _: Rect, _: SurfaceLevel) -> bool {
        true
    }

    fn desktop_icons_hidden(&self) -> bool {
        false
    }

    fn set_desktop_icons_hidden(&self, _: bool) -> Result<()> {
        bail!("icônes du bureau : {NOT_YET}")
    }

    fn watch_desktop_double_click(&self, _: crate::Callback) -> bool {
        false
    }

    fn set_desktop_menu(&self, _: Option<&str>) -> Result<()> {
        Ok(())
    }

    fn open(&self, _: &Path) -> Result<()> {
        bail!("ouvrir : {NOT_YET}")
    }

    fn reveal(&self, _: &Path) -> Result<()> {
        bail!("afficher : {NOT_YET}")
    }

    fn preferred_languages(&self) -> Vec<String> {
        std::env::var("LANG").into_iter().collect()
    }

    fn login_item(&self) -> LoginState {
        LoginState::Unavailable(NOT_YET.into())
    }

    fn set_login_item(&self, _: bool) -> Result<LoginState> {
        Ok(self.login_item())
    }
}
