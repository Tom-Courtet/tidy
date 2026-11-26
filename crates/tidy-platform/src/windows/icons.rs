//! Positions des icônes du bureau de l'Explorateur, par l'API du Shell (COM).
//!
//! Méthode documentée par Raymond Chen (« Manipulating the positions of desktop
//! icons ») : `IShellWindows::FindWindowSW(SWC_DESKTOP)` donne le navigateur du bureau,
//! puis sa vue active (`IShellView` → `IFolderView`). On y lit, pour chaque icône, son
//! **chemin complet** (nom d'analyse) et sa position, et on la déplace avec
//! `SelectAndPositionItems`. Pas d'injection, pas de lecture de la mémoire de
//! l'Explorateur : l'Explorateur fait lui-même le travail, à notre demande.
//!
//! Chaque appel ouvre sa propre session COM sur le thread courant (mode « appartement »,
//! comme le thread de l'interface) ; si le thread est déjà initialisé autrement, on
//! s'en accommode.

use std::path::PathBuf;

use anyhow::{Context, Result};
use tidy_core::icon_stash::{DesktopIcons, IconPos};
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    IServiceProvider,
};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Shell::Common::{ITEMIDLIST, STRRET};
use windows::Win32::UI::Shell::{
    CSIDL_DESKTOP, FWF_AUTOARRANGE, FWF_SNAPTOGRID, IFolderView, IFolderView2, IShellBrowser,
    IShellFolder, IShellWindows, SHGDN_FORPARSING, SID_STopLevelBrowser, SVGIO_ALLVIEW,
    SVSI_POSITIONITEM, SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellWindows, StrRetToStrW,
};
use windows::core::{Interface, PWSTR};

/// Icônes du bureau de l'Explorateur.
pub struct ExplorerIcons;

/// Initialise COM sur ce thread (sans erreur s'il l'est déjà, même autrement).
pub(super) fn com_init() {
    // SAFETY: appel sans pointeur ; un thread déjà initialisé renvoie S_FALSE ou
    // RPC_E_CHANGED_MODE, que l'on ignore volontairement.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
}

/// Vue des icônes du bureau.
fn folder_view() -> Result<IFolderView> {
    com_init();
    // SAFETY: appels COM documentés, dans l'ordre de la méthode de Raymond Chen ; les
    // VARIANT sont initialisés (VT_I4 pour l'emplacement, vide pour la racine).
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)
            .context("fenêtres du Shell inaccessibles")?;
        let mut location = VARIANT::default();
        (*location.Anonymous.Anonymous).vt = VT_I4;
        (*location.Anonymous.Anonymous).Anonymous.lVal = CSIDL_DESKTOP as i32;
        let root = VARIANT::default();
        let mut hwnd = 0i32;
        let dispatch = windows
            .FindWindowSW(&location, &root, SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH)
            .context("bureau du Shell introuvable")?;
        let provider: IServiceProvider = dispatch.cast()?;
        let browser: IShellBrowser = provider.QueryService(&SID_STopLevelBrowser)?;
        let view = browser.QueryActiveShellView()?;
        Ok(view.cast()?)
    }
}

/// Élément d'identification rendu par le Shell, libéré à la fin.
pub(super) struct Pidl(pub(super) *mut ITEMIDLIST);

impl Drop for Pidl {
    fn drop(&mut self) {
        // SAFETY: pointeur alloué par le Shell (IFolderView::Item), libéré une fois.
        unsafe { CoTaskMemFree(Some(self.0 as *const _)) };
    }
}

/// Chemin complet (nom d'analyse) d'un élément de la vue.
fn parsing_name(folder: &IShellFolder, pidl: &Pidl) -> Result<String> {
    let mut strret = STRRET::default();
    let mut text = PWSTR::null();
    // SAFETY: `pidl` appartient à `folder` (même vue) ; StrRetToStrW alloue `text`,
    // libéré juste après sa copie.
    unsafe {
        folder.GetDisplayNameOf(pidl.0, SHGDN_FORPARSING, &mut strret)?;
        StrRetToStrW(&mut strret, Some(pidl.0), &mut text)?;
        let out = text.to_string();
        CoTaskMemFree(Some(text.0 as *const _));
        Ok(out?)
    }
}

/// Toutes les icônes : (élément, chemin, position).
fn list(view: &IFolderView) -> Result<Vec<(Pidl, PathBuf, IconPos)>> {
    // SAFETY: méthodes de la vue obtenue ci-dessus ; chaque élément rendu est confié à
    // un `Pidl` qui le libère.
    unsafe {
        let folder: IShellFolder = view.GetFolder()?;
        let count = view.ItemCount(SVGIO_ALLVIEW)?;
        let mut out = Vec::with_capacity(count.max(0) as usize);
        for i in 0..count {
            let pidl = Pidl(view.Item(i)?);
            let pos = view.GetItemPosition(pidl.0)?;
            let path = parsing_name(&folder, &pidl)?;
            out.push((pidl, PathBuf::from(path), IconPos { x: pos.x, y: pos.y }));
        }
        Ok(out)
    }
}

fn same(a: &std::path::Path, b: &std::path::Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

impl DesktopIcons for ExplorerIcons {
    fn positions(&self) -> Result<Vec<(PathBuf, IconPos)>> {
        let view = folder_view()?;
        Ok(list(&view)?
            .into_iter()
            .map(|(_, path, pos)| (path, pos))
            .collect())
    }

    fn move_icons(&self, moves: &[(PathBuf, IconPos)]) -> Result<()> {
        let view = folder_view()?;
        let items = list(&view)?;
        let mut pidls: Vec<*const ITEMIDLIST> = Vec::new();
        let mut points: Vec<POINT> = Vec::new();
        for (path, pos) in moves {
            if let Some((pidl, _, _)) = items.iter().find(|(_, p, _)| same(p, path)) {
                pidls.push(pidl.0 as *const ITEMIDLIST);
                points.push(POINT { x: pos.x, y: pos.y });
            }
        }
        if pidls.is_empty() {
            return Ok(());
        }
        // SAFETY: `pidls` et `points` ont la même longueur et vivent pendant l'appel ;
        // les éléments appartiennent à cette vue (`items` les garde en vie).
        unsafe {
            view.SelectAndPositionItems(
                pidls.len() as u32,
                pidls.as_ptr(),
                Some(points.as_ptr()),
                SVSI_POSITIONITEM.0 as u32,
            )?;
        }
        Ok(())
    }

    fn auto_arrange(&self) -> Result<bool> {
        let view: IFolderView2 = folder_view()?.cast()?;
        // SAFETY: lecture des options de la vue.
        let flags = unsafe { view.GetCurrentFolderFlags()? };
        Ok(flags & FWF_AUTOARRANGE.0 as u32 != 0)
    }

    fn set_auto_arrange(&self, on: bool) -> Result<()> {
        set_flag(FWF_AUTOARRANGE.0 as u32, on)
    }

    fn snap_to_grid(&self) -> Result<bool> {
        let view: IFolderView2 = folder_view()?.cast()?;
        // SAFETY: lecture des options de la vue.
        let flags = unsafe { view.GetCurrentFolderFlags()? };
        Ok(flags & FWF_SNAPTOGRID.0 as u32 != 0)
    }

    fn set_snap_to_grid(&self, on: bool) -> Result<()> {
        set_flag(FWF_SNAPTOGRID.0 as u32, on)
    }
}

/// Change une seule option de la vue du bureau.
fn set_flag(bit: u32, on: bool) -> Result<()> {
    let view: IFolderView2 = folder_view()?.cast()?;
    // SAFETY: le masque limite le changement au seul bit demandé.
    unsafe { view.SetCurrentFolderFlags(bit, if on { bit } else { 0 })? };
    Ok(())
}
