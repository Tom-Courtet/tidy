//! Fichiers vus par le Shell de l'Explorateur : icônes et miniatures, menu
//! contextuel natif, glisser-déposer vers une autre application.
//!
//! Tout passe par les objets du Shell (`IShellItem…`), comme dans l'Explorateur :
//! mêmes icônes, mêmes entrées de menu (y compris celles des logiciels installés),
//! même glisser-déposer. Rien n'est supprimé ici : l'entrée « Supprimer » du menu natif
//! est celle de l'Explorateur (corbeille, sauf Maj+Suppr choisi par l'utilisateur).

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, SIZE, WPARAM};
use windows::Win32::System::Com::{IBindCtx, IDataObject};
use windows::Win32::System::Ole::{
    DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, IDropSource, OleInitialize, OleUninitialize,
};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_DataObject, BHID_SFUIObject, CMF_EXTENDEDVERBS, CMF_NORMAL, CMIC_MASK_CONTROL_DOWN,
    CMIC_MASK_PTINVOKE, CMIC_MASK_SHIFT_DOWN, CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX,
    GCS_VERBW, IContextMenu, IContextMenu2, IContextMenu3, IShellItemArray, IShellItemImageFactory,
    SHCreateDataObject, SHCreateItemFromParsingName, SHCreateShellItemArrayFromIDLists,
    SHDoDragDrop, SHParseDisplayName, SIIGBF_ICONONLY, SIIGBF_RESIZETOFIT,
};
use windows::Win32::UI::WindowsAndMessaging::HMENU;
use windows::core::{Interface, PCSTR, PCWSTR, PSTR};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HBITMAP, ReleaseDC, ScreenToClient,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetCursorPos, PostMessageW, SW_SHOWNORMAL, SetForegroundWindow,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_DRAWITEM, WM_INITMENUPOPUP, WM_LBUTTONUP,
    WM_MEASUREITEM, WM_MENUCHAR, WM_NULL,
};

use super::icons::{Pidl, com_init};
use super::{wide, window};
use crate::IconImage;
use crate::convert::{bgra_to_rgba, icon_side, shell_path};

/// `CMIC_MASK_UNICODE` (shobjidl.h) : les champs `…W` de la commande sont remplis.
const CMIC_MASK_UNICODE: u32 = 0x0000_4000;
/// `HRESULT_FROM_WIN32(ERROR_CANCELLED)` : l'utilisateur a annulé (confirmation…).
const CANCELLED: i32 = 0x8007_04C7_u32 as i32;
/// Identifiants des commandes du Shell dans notre menu : de 1 à 0x7FFF.
const FIRST_COMMAND: u32 = 1;
const LAST_COMMAND: u32 = 0x7FFF;
/// Identifiant de la sous-classe qui relaie les messages du menu (unique ici).
const MENU_SUBCLASS_ID: usize = 0x7469_6D6E; // « timn »

/// Session OLE sur ce thread (presse-papiers des commandes « Copier » et « Couper »,
/// glisser-déposer), refermée à la fin si on l'a ouverte.
struct Ole(bool);

impl Ole {
    fn init() -> Result<Self> {
        // SAFETY: appel sans pointeur. Échec si le thread est déjà en mode COM
        // « multithread » : OLE y est impossible, on le signale.
        unsafe { OleInitialize(None) }.context("OLE indisponible sur ce thread")?;
        Ok(Self(true))
    }
}

impl Drop for Ole {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: équilibre l'OleInitialize réussi de `init`, sur le même thread.
            unsafe { OleUninitialize() };
        }
    }
}

/// Élément d'identification absolu de chaque chemin.
fn pidls(paths: &[PathBuf]) -> Result<Vec<Pidl>> {
    paths
        .iter()
        .map(|path| {
            let name = wide(&shell_path(&path.to_string_lossy()));
            let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
            // SAFETY: nom terminé par un zéro, vivant pendant l'appel ; le Shell alloue
            // `pidl`, confié aussitôt à un `Pidl` qui le libère.
            unsafe {
                SHParseDisplayName(PCWSTR(name.as_ptr()), None::<&IBindCtx>, &mut pidl, 0, None)
            }
            .with_context(|| format!("{} inconnu du Shell", path.display()))?;
            if pidl.is_null() {
                bail!("{} inconnu du Shell", path.display());
            }
            Ok(Pidl(pidl))
        })
        .collect()
}

fn raw_pidls(pidls: &[Pidl]) -> Vec<*const ITEMIDLIST> {
    pidls.iter().map(|p| p.0 as *const ITEMIDLIST).collect()
}

fn item_array(pidls: &[Pidl]) -> Result<IShellItemArray> {
    let raw = raw_pidls(pidls);
    // SAFETY: éléments absolus valides (gardés en vie par `pidls`) ; le tableau du Shell
    // en fait sa propre copie.
    Ok(unsafe { SHCreateShellItemArrayFromIDLists(&raw)? })
}

/// Fenêtre de Tidy de ce titre, sur ce thread.
fn owner(window_title: &str) -> Result<HWND> {
    window::find_titled(window_title)
        .map(HWND)
        .with_context(|| format!("fenêtre « {window_title} » introuvable"))
}

// --- Icônes ---------------------------------------------------------------------

/// HBITMAP rendu par le Shell, détruit à la fin.
struct Bitmap(HBITMAP);

impl Drop for Bitmap {
    fn drop(&mut self) {
        // SAFETY: bitmap créé pour nous par GetImage, sélectionné dans aucun contexte.
        unsafe { DeleteObject(self.0) };
    }
}

/// Pixels d'un bitmap, en RGBA non prémultiplié.
fn read_bitmap(bitmap: &Bitmap) -> Result<IconImage> {
    let mut info = BITMAP::default();
    // SAFETY: `info` est une BITMAP dont la taille est passée à l'API.
    let n = unsafe {
        GetObjectW(
            bitmap.0,
            std::mem::size_of::<BITMAP>() as i32,
            (&mut info as *mut BITMAP).cast(),
        )
    };
    if n == 0 || info.bmWidth <= 0 || info.bmHeight == 0 {
        bail!("bitmap illisible");
    }
    let (width, height) = (info.bmWidth, info.bmHeight.abs());
    let mut header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Hauteur négative : lignes de haut en bas.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bgra = vec![0u8; width as usize * height as usize * 4];
    // SAFETY: contexte de l'écran, rendu juste après ; `bgra` a la place des `height`
    // lignes de 32 bits demandées par l'en-tête.
    let lines = unsafe {
        let dc = GetDC(std::ptr::null_mut());
        let lines = GetDIBits(
            dc,
            bitmap.0,
            0,
            height as u32,
            bgra.as_mut_ptr().cast(),
            &mut header,
            DIB_RGB_COLORS,
        );
        ReleaseDC(std::ptr::null_mut(), dc);
        lines
    };
    if lines != height {
        bail!("pixels du bitmap illisibles");
    }
    Ok(IconImage {
        width: width as u32,
        height: height as u32,
        rgba: bgra_to_rgba(&bgra),
    })
}

/// Icône (ou miniature) d'un élément, telle que l'Explorateur la dessine.
pub fn file_icon(path: &Path, size_px: u32, thumbnail: bool) -> Result<IconImage> {
    com_init();
    let side = icon_side(size_px) as i32;
    let name = wide(&shell_path(&path.to_string_lossy()));
    // Miniature : celle du fichier si le système en a une, sinon l'icône, réduite pour
    // tenir dans le carré (les côtés peuvent différer).
    let flags = if thumbnail {
        SIIGBF_RESIZETOFIT
    } else {
        SIIGBF_ICONONLY
    };
    // SAFETY: nom terminé par un zéro, vivant pendant l'appel ; le bitmap rendu est
    // confié à `Bitmap` qui le détruit.
    let bitmap = unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None::<&IBindCtx>)?;
        let hbitmap = factory.GetImage(SIZE { cx: side, cy: side }, flags)?;
        Bitmap(hbitmap.0)
    };
    read_bitmap(&bitmap)
}

// --- Menu contextuel ------------------------------------------------------------

thread_local! {
    /// Menu du Shell ouvert sur ce thread : ses sous-menus (« Ouvrir avec »,
    /// « Envoyer vers ») se remplissent par des messages qu'il faut lui relayer.
    static OPEN_MENU: RefCell<Option<IContextMenu>> = const { RefCell::new(None) };
}

/// Sous-classe posée le temps du menu : relaie au Shell les messages de ses sous-menus.
unsafe extern "system" fn relay_menu_messages(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: usize,
    lparam: isize,
    _id: usize,
    _data: usize,
) -> isize {
    if matches!(
        msg,
        WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR
    ) && let Some(menu) = OPEN_MENU.with(|m| m.borrow().clone())
    {
        if let Ok(menu3) = menu.cast::<IContextMenu3>() {
            let mut result = windows::Win32::Foundation::LRESULT(0);
            // SAFETY: message reçu du système, relayé tel quel au menu du Shell.
            if unsafe {
                menu3.HandleMenuMsg2(msg, WPARAM(wparam), LPARAM(lparam), Some(&mut result))
            }
            .is_ok()
            {
                return result.0;
            }
        } else if let Ok(menu2) = menu.cast::<IContextMenu2>() {
            // SAFETY: idem.
            if unsafe { menu2.HandleMenuMsg(msg, WPARAM(wparam), LPARAM(lparam)) }.is_ok() {
                return 0;
            }
        }
    }
    // SAFETY: on rend la main à la procédure d'origine, avec les mêmes arguments.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Menu Windows créé pour l'occasion, détruit à la fin.
struct PopupMenu(windows_sys::Win32::UI::WindowsAndMessaging::HMENU);

impl Drop for PopupMenu {
    fn drop(&mut self) {
        // SAFETY: menu créé par CreatePopupMenu, détruit une fois.
        unsafe { DestroyMenu(self.0) };
    }
}

/// Sous-classe de relais retirée à la fin, même en cas d'erreur.
struct Relay(windows_sys::Win32::Foundation::HWND);

impl Drop for Relay {
    fn drop(&mut self) {
        OPEN_MENU.with(|m| m.borrow_mut().take());
        // SAFETY: retire la sous-classe posée par `show_menu` sur la même fenêtre.
        unsafe { RemoveWindowSubclass(self.0, Some(relay_menu_messages), MENU_SUBCLASS_ID) };
    }
}

fn key_down(key: u16) -> bool {
    // SAFETY: simple lecture de l'état du clavier.
    unsafe { GetKeyState(i32::from(key)) < 0 }
}

/// Menu contextuel de l'Explorateur pour ces éléments, sous la souris.
pub fn show_menu(window_title: &str, paths: &[PathBuf]) -> Result<bool> {
    if paths.is_empty() {
        return Ok(false);
    }
    let _ole = Ole::init()?;
    let hwnd = owner(window_title)?;
    let pidls = pidls(paths)?;
    // Éléments d'un même dossier (cas des boîtes : le bureau) : objet d'interface du
    // dossier parent, comme l'Explorateur.
    // SAFETY: tableau d'éléments valide ; renvoie un menu COM ou une erreur.
    let menu: IContextMenu =
        unsafe { item_array(&pidls)?.BindToHandler(None::<&IBindCtx>, &BHID_SFUIObject)? };
    // SAFETY: création d'un menu vide, confié à `PopupMenu`.
    let popup = PopupMenu(unsafe { CreatePopupMenu() });
    if popup.0.is_null() {
        bail!("menu impossible à créer");
    }
    let mut flags = CMF_NORMAL;
    if key_down(VK_SHIFT) {
        // Maj : entrées étendues (« Copier en tant que chemin d'accès »…), comme
        // l'Explorateur.
        flags |= CMF_EXTENDEDVERBS;
    }
    // SAFETY: menu vide valide ; le Shell y ajoute ses entrées entre nos bornes.
    unsafe { menu.QueryContextMenu(HMENU(popup.0), 0, FIRST_COMMAND, LAST_COMMAND, flags) }
        .ok()
        .context("entrées du menu du Shell indisponibles")?;

    let mut cursor = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
    // SAFETY: sortie locale.
    unsafe { GetCursorPos(&mut cursor) };
    OPEN_MENU.with(|m| *m.borrow_mut() = Some(menu.clone()));
    // SAFETY: fenêtre de ce thread ; la sous-classe est retirée par `Relay`.
    unsafe { SetWindowSubclass(hwnd.0, Some(relay_menu_messages), MENU_SUBCLASS_ID, 0) };
    let relay = Relay(hwnd.0);
    // SAFETY: fenêtre et menu valides ; la fenêtre au premier plan ferme le menu quand
    // on clique ailleurs (KB135788), le message vide qui suit le confirme.
    let command = unsafe {
        SetForegroundWindow(hwnd.0);
        let command = TrackPopupMenuEx(
            popup.0,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            cursor.x,
            cursor.y,
            hwnd.0,
            std::ptr::null(),
        );
        PostMessageW(hwnd.0, WM_NULL, 0, 0);
        command
    };
    drop(relay);
    if command <= 0 {
        // Menu fermé sans choix.
        return Ok(true);
    }
    let offset = (command as u32 - FIRST_COMMAND) as usize;
    let mut mask = CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE;
    // État des touches au moment du choix, pas à l'ouverture du menu. Jamais Maj pour
    // « Supprimer » : Maj transmise rendrait la suppression définitive ; Tidy ne passe
    // que par la corbeille.
    let delete = verb_of(&menu, offset).is_some_and(|v| is_delete_verb(&v));
    if key_down(VK_SHIFT) && !delete {
        mask |= CMIC_MASK_SHIFT_DOWN;
    }
    if key_down(VK_CONTROL) {
        mask |= CMIC_MASK_CONTROL_DOWN;
    }
    let invoke = CMINVOKECOMMANDINFOEX {
        cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
        fMask: mask,
        hwnd,
        // MAKEINTRESOURCE : numéro de la commande à la place d'un verbe.
        lpVerb: PCSTR(offset as *const u8),
        lpVerbW: PCWSTR(offset as *const u16),
        nShow: SW_SHOWNORMAL,
        ptInvoke: POINT {
            x: cursor.x,
            y: cursor.y,
        },
        ..Default::default()
    };
    // SAFETY: structure complète, `cbSize` annonce la version étendue ; les verbes sont
    // des numéros (pas des pointeurs) comme le permet MAKEINTRESOURCE.
    let result = unsafe {
        menu.InvokeCommand((&invoke as *const CMINVOKECOMMANDINFOEX).cast::<CMINVOKECOMMANDINFO>())
    };
    match result {
        Err(e) if e.code().0 == CANCELLED => Ok(true),
        Err(e) => Err(e).context("commande du menu du Shell"),
        Ok(()) => Ok(true),
    }
}

/// Verbe (nom stable, comme « delete ») de la commande n° `offset` du menu, s'il en a.
fn verb_of(menu: &IContextMenu, offset: usize) -> Option<String> {
    let mut buf = [0u16; 64];
    // SAFETY: tampon local de `buf.len()` caractères UTF-16 ; avec GCS_VERBW, le Shell
    // y écrit une chaîne terminée par un zéro (le type PSTR est celui de l'API).
    unsafe {
        menu.GetCommandString(
            offset,
            GCS_VERBW,
            None,
            PSTR(buf.as_mut_ptr().cast()),
            buf.len() as u32,
        )
    }
    .ok()?;
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// Verbe de suppression du Shell (corbeille, ou définitive avec Maj).
fn is_delete_verb(verb: &str) -> bool {
    verb.eq_ignore_ascii_case("delete")
}

// --- Glisser-déposer ------------------------------------------------------------

/// Objet de données du Shell pour ces éléments (comme un glisser depuis
/// l'Explorateur) : fichiers, chemins, image de glisser.
fn data_object(pidls: &[Pidl]) -> Result<IDataObject> {
    // SAFETY: tableau d'éléments valide.
    let from_array = item_array(pidls)
        .and_then(|array| Ok(unsafe { array.BindToHandler(None::<&IBindCtx>, &BHID_DataObject)? }));
    match from_array {
        Ok(data) => Ok(data),
        Err(e) => {
            // Éléments de dossiers différents : objet de données relatif au bureau
            // (racine du Shell), qui accepte des éléments absolus.
            log::debug!("objet de données par tableau refusé ({e:#}), repli");
            let raw = raw_pidls(pidls);
            // SAFETY: éléments absolus valides pendant l'appel ; dossier nul = racine.
            Ok(unsafe { SHCreateDataObject(None, Some(&raw), None::<&IDataObject>)? })
        }
    }
}

/// Signale à la fenêtre que le bouton est relâché : la boucle du glisser-déposer a
/// gardé le relâchement pour elle, sans lui, egui croirait le bouton toujours enfoncé.
fn release_button(hwnd: windows_sys::Win32::Foundation::HWND) {
    let mut p = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
    // SAFETY: sorties locales ; fenêtre de ce thread.
    unsafe {
        GetCursorPos(&mut p);
        ScreenToClient(hwnd, &mut p);
    }
    // MAKELPARAM : coordonnées signées sur 16 bits.
    let lparam = ((p.y as i16 as u16 as u32) << 16 | p.x as i16 as u16 as u32) as isize;
    // SAFETY: message posté (pas d'attente) à une fenêtre de ce processus.
    unsafe { PostMessageW(hwnd, WM_LBUTTONUP, 0, lparam) };
}

/// Glisser-déposer de ces fichiers vers une autre fenêtre (Explorateur, appli). Rend
/// la main quand l'utilisateur lâche (ou annule avec Échap).
pub fn start_drag(window_title: &str, paths: &[PathBuf]) -> Result<bool> {
    if paths.is_empty() {
        return Ok(false);
    }
    let _ole = Ole::init()?;
    let hwnd = owner(window_title)?;
    let pidls = pidls(paths)?;
    let data = data_object(&pidls)?;
    // La cible choisit : déplacer (même disque), copier ou créer un raccourci. Tidy ne
    // supprime jamais rien après coup (un « déplacement optimisé » est fait par la
    // cible elle-même).
    let allowed = DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK;
    // SAFETY: objet de données valide ; source nulle = source par défaut du Shell
    // (bouton relâché → dépôt, Échap → annulation), image de glisser fournie par le
    // Shell. Boucle modale sur ce thread.
    let result = unsafe { SHDoDragDrop(Some(hwnd), &data, None::<&IDropSource>, allowed) };
    release_button(hwnd.0);
    let effect = result.context("glisser-déposer du Shell")?;
    log::debug!("glisser-déposer terminé (effet {})", effect.0);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_verb_is_recognised() {
        assert!(is_delete_verb("delete"));
        assert!(is_delete_verb("Delete"));
        assert!(!is_delete_verb("rename"));
        assert!(!is_delete_verb("cut"));
    }
}
