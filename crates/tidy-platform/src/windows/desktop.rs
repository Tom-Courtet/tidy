//! Bureau de l'Explorateur : ses icônes, le double-clic et le rectangle de sélection
//! sur une zone vide.
//!
//! Les icônes du bureau sont une liste (`SysListView32`, nommée « FolderView ») dans
//! `SHELLDLL_DefView`, elle-même dans `Progman` ou, selon la version de Windows et le
//! fond d'écran, dans une fenêtre `WorkerW`. Masquer cette liste masque toutes les
//! icônes, sans rien écrire sur le disque ni injecter de code dans l'Explorateur ; un
//! redémarrage de l'Explorateur ou de la session les rend toujours.
//!
//! Double-clic : un crochet souris bas niveau (`WH_MOUSE_LL`, sans DLL) repère deux
//! clics rapprochés sur le bureau. Clic sur une icône ou sur le vide ? Un clic sur le
//! vide désélectionne tout : un court instant après, la liste n'a plus d'élément
//! sélectionné. C'est le seul cas qui déclenche le rappel.
//!
//! Rectangle de sélection (« dessiner une boîte ») : le même crochet suit l'appui et
//! le relâchement du bouton gauche. Heuristique pour ne pas le confondre avec le
//! déplacement d'une icône :
//! 1. l'appui tombe sur le bureau de l'Explorateur, **icônes visibles** ;
//! 2. juste après l'appui (`PRESS_DELAY`), la liste n'a **aucun élément
//!    sélectionné** : un appui sur une icône la sélectionne aussitôt (et elle reste
//!    sélectionnée pendant qu'on la déplace) ; un appui sur le vide désélectionne tout,
//!    avant que le rectangle n'ait le temps d'attraper une icône ;
//! 3. au relâchement, le rectangle fait au moins `MIN_SELECTION_SIDE` points de large
//!    et de haut.
//!
//! Limites voulues (mieux vaut rater une proposition que proposer à tort) : un
//! rectangle tracé très vite (icône attrapée avant la vérification) ou avec Ctrl (la
//! sélection précédente reste) n'est pas signalé, ni un rectangle tracé icônes
//! masquées.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Result, bail};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetParent,
    GetSystemMetrics, HHOOK, IsWindowVisible, MSLLHOOKSTRUCT, SM_CXDOUBLECLK, SM_CYDOUBLECLK,
    SMTO_ABORTIFHUNG, SW_HIDE, SW_SHOW, SendMessageTimeoutW, SetWindowsHookExW, ShowWindow,
    UnhookWindowsHookEx, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WindowFromPoint,
};
use windows_sys::core::BOOL;

use super::{from_wide, screens, wide};
use crate::convert::{MIN_SELECTION_SIDE, selection_rect};
use crate::{Callback, RectCallback};

/// `LVM_GETSELECTEDCOUNT` (commctrl.h).
const LVM_GETSELECTEDCOUNT: u32 = 0x1032;
/// Délai avant de lire la sélection : le temps que l'Explorateur traite le clic.
const SELECTION_DELAY: Duration = Duration::from_millis(80);
/// Délai avant de lire la sélection après un appui (rectangle) : assez pour que
/// l'Explorateur ait traité l'appui, assez court pour que le rectangle n'ait pas encore
/// attrapé d'icône.
const PRESS_DELAY: Duration = Duration::from_millis(60);

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    // SAFETY: tampon local, taille passée à l'API ; `hwnd` peut être nul (échec sans
    // effet).
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return String::new();
    }
    from_wide(&buf[..n as usize])
}

fn child(parent: HWND, class: &str) -> HWND {
    let class = wide(class);
    // SAFETY: nom de classe terminé par un zéro ; parent nul = fenêtres de premier
    // niveau.
    unsafe {
        FindWindowExW(
            parent,
            std::ptr::null_mut(),
            class.as_ptr(),
            std::ptr::null(),
        )
    }
}

/// `SHELLDLL_DefView` du bureau : dans `Progman`, sinon dans une `WorkerW`.
fn def_view() -> HWND {
    let progman_class = wide("Progman");
    // SAFETY: nom de classe terminé par un zéro.
    let progman = unsafe { FindWindowW(progman_class.as_ptr(), std::ptr::null()) };
    let view = child(progman, "SHELLDLL_DefView");
    if !view.is_null() {
        return view;
    }
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        let class = wide("SHELLDLL_DefView");
        // SAFETY: nom terminé par un zéro ; `hwnd` vient d'EnumWindows.
        let view =
            unsafe { FindWindowExW(hwnd, std::ptr::null_mut(), class.as_ptr(), std::ptr::null()) };
        if view.is_null() {
            return 1;
        }
        // SAFETY: `data` est le `&mut HWND` passé à EnumWindows ci-dessous.
        unsafe { *(data as *mut HWND) = view };
        0
    }
    let mut found: HWND = std::ptr::null_mut();
    // SAFETY: le rappel ne vit que pendant l'appel, comme `found`.
    unsafe { EnumWindows(Some(visit), &mut found as *mut HWND as LPARAM) };
    found
}

/// Liste des icônes du bureau (`FolderView`).
fn folder_view() -> HWND {
    child(def_view(), "SysListView32")
}

pub fn icons_hidden() -> bool {
    let view = folder_view();
    // SAFETY: simple lecture de l'état d'une fenêtre (nulle : faux).
    !view.is_null() && unsafe { IsWindowVisible(view) } == 0
}

pub fn set_icons_hidden(hidden: bool) -> Result<()> {
    let view = folder_view();
    if view.is_null() {
        bail!("liste des icônes du bureau introuvable");
    }
    // SAFETY: fenêtre de l'Explorateur trouvée ci-dessus ; ShowWindow fonctionne d'un
    // processus à l'autre et ne fait que montrer ou cacher.
    unsafe { ShowWindow(view, if hidden { SW_HIDE } else { SW_SHOW }) };
    Ok(())
}

/// La fenêtre sous le clic fait-elle partie du bureau de l'Explorateur ?
fn is_desktop(hwnd: HWND) -> bool {
    match class_of(hwnd).as_str() {
        "SHELLDLL_DefView" | "Progman" | "WorkerW" => true,
        // SAFETY: lecture du parent d'une fenêtre existante.
        "SysListView32" => class_of(unsafe { GetParent(hwnd) }) == "SHELLDLL_DefView",
        _ => false,
    }
}

/// Aucune icône sélectionnée (ou icônes masquées) : le double-clic était sur le vide.
fn nothing_selected() -> bool {
    selected_count().is_none_or(|count| count == 0)
}

/// Nombre d'icônes sélectionnées ; `None` si les icônes sont masquées ou si
/// l'Explorateur ne répond pas.
fn selected_count() -> Option<usize> {
    let view = folder_view();
    // SAFETY: lecture de l'état d'une fenêtre (nulle : faux).
    if view.is_null() || unsafe { IsWindowVisible(view) } == 0 {
        return None;
    }
    let mut count = 0usize;
    // SAFETY: message sans pointeur (aucune mémoire à partager entre processus) ; délai
    // court si l'Explorateur ne répond pas.
    let ok = unsafe {
        SendMessageTimeoutW(
            view,
            LVM_GETSELECTEDCOUNT,
            0,
            0,
            SMTO_ABORTIFHUNG,
            200,
            &mut count,
        )
    };
    (ok != 0).then_some(count)
}

static ON_DOUBLE_CLICK: OnceLock<Callback> = OnceLock::new();
/// Dernier clic sur le bureau : (heure en ms, position).
static LAST_CLICK: Mutex<Option<(u32, POINT)>> = Mutex::new(None);

static ON_DRAG: OnceLock<RectCallback> = OnceLock::new();
/// Appui en cours sur le bureau, candidat au rectangle de sélection.
static PRESS: Mutex<Option<Press>> = Mutex::new(None);
/// Numéro du dernier appui (pour ignorer une vérification arrivée trop tard).
static PRESS_ID: AtomicU64 = AtomicU64::new(0);

struct Press {
    id: u64,
    /// Position de l'appui, en pixels physiques.
    start: POINT,
    /// Sélection vide juste après l'appui ? `None` : pas encore vérifié.
    on_empty: Option<bool>,
}

/// Deux clics forment-ils un double-clic (délai et distance du système) ?
fn is_double(
    first: (u32, POINT),
    second: (u32, POINT),
    max_ms: u32,
    max_dx: i32,
    max_dy: i32,
) -> bool {
    second.0.wrapping_sub(first.0) <= max_ms
        && (second.1.x - first.1.x).abs() <= max_dx / 2
        && (second.1.y - first.1.y).abs() <= max_dy / 2
}

/// Double-clic : deuxième clic proche du premier, puis vérification à part.
fn on_double_click_press(on_desktop: bool, click: (u32, POINT)) {
    if ON_DOUBLE_CLICK.get().is_none() {
        return;
    }
    let mut last = LAST_CLICK.lock().unwrap_or_else(|p| p.into_inner());
    if !on_desktop {
        *last = None;
        return;
    }
    // SAFETY: fonctions sans argument ni effet de bord.
    let (max_ms, dx, dy) = unsafe {
        (
            GetDoubleClickTime(),
            GetSystemMetrics(SM_CXDOUBLECLK),
            GetSystemMetrics(SM_CYDOUBLECLK),
        )
    };
    match *last {
        Some(first) if is_double(first, click, max_ms, dx, dy) => {
            *last = None;
            // Ne pas bloquer le crochet : la vérification se fait à part.
            std::thread::spawn(|| {
                std::thread::sleep(SELECTION_DELAY);
                if nothing_selected()
                    && let Some(callback) = ON_DOUBLE_CLICK.get()
                {
                    callback();
                }
            });
        }
        _ => *last = Some(click),
    }
}

/// Appui : candidat au rectangle si le bureau est visé. La sélection est lue à part, un
/// court instant après (le crochet ne doit pas attendre l'Explorateur).
fn on_drag_press(on_desktop: bool, start: POINT) {
    if ON_DRAG.get().is_none() {
        return;
    }
    let mut press = PRESS.lock().unwrap_or_else(|p| p.into_inner());
    if !on_desktop {
        *press = None;
        return;
    }
    let id = PRESS_ID.fetch_add(1, Ordering::Relaxed) + 1;
    *press = Some(Press {
        id,
        start,
        on_empty: None,
    });
    std::thread::spawn(move || {
        std::thread::sleep(PRESS_DELAY);
        let on_empty = selected_count() == Some(0);
        let mut press = PRESS.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(p) = press.as_mut()
            && p.id == id
        {
            p.on_empty = Some(on_empty);
        }
    });
}

/// Relâchement : rectangle signalé si l'appui était sur le vide et assez loin.
fn on_drag_release(end: POINT) {
    let Some(press) = PRESS.lock().unwrap_or_else(|p| p.into_inner()).take() else {
        return;
    };
    if press.on_empty != Some(true) {
        return;
    }
    // Conversion en points (énumère les écrans) et rappel hors du crochet.
    std::thread::spawn(move || {
        let start = screens::to_points(press.start);
        let end = screens::to_points(end);
        if let Some(rect) = selection_rect(start, end, MIN_SELECTION_SIDE)
            && let Some(callback) = ON_DRAG.get()
        {
            callback(rect);
        }
    });
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let msg = wparam as u32;
    if code >= 0 && lparam != 0 && (msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP) {
        // SAFETY: pour WH_MOUSE_LL, `lparam` pointe sur une MSLLHOOKSTRUCT valide pendant
        // l'appel.
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if msg == WM_LBUTTONDOWN {
            // SAFETY: point reçu du système ; renvoie une fenêtre ou nul.
            let target = unsafe { WindowFromPoint(info.pt) };
            let on_desktop = is_desktop(target);
            on_double_click_press(on_desktop, (info.time, info.pt));
            on_drag_press(on_desktop, info.pt);
        } else if ON_DRAG.get().is_some() {
            on_drag_release(info.pt);
        }
    }
    // SAFETY: on passe le relais au crochet suivant, avec les mêmes arguments.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Crochet souris installé (`HHOOK`), 0 sinon ou une fois retiré.
static HOOK: AtomicUsize = AtomicUsize::new(0);

/// Retire le crochet souris (avant une attente longue du thread qui le porte, comme
/// l'arrêt de l'agent) : un crochet bas niveau qui ne répond plus ralentit la souris
/// de toute la session. Il n'est pas réinstallé.
pub fn unhook() {
    let hook = HOOK.swap(0, Ordering::SeqCst);
    if hook != 0 {
        // SAFETY: crochet rendu par `SetWindowsHookExW` dans ce processus ; le `swap`
        // garantit qu'il n'est retiré qu'une fois.
        let ok = unsafe { UnhookWindowsHookEx(hook as HHOOK) };
        if ok == 0 {
            log::warn!("crochet souris non retiré");
        }
    }
}

/// Installe le crochet souris, une fois par processus.
fn install_hook() -> bool {
    static INSTALLED: OnceLock<bool> = OnceLock::new();
    *INSTALLED.get_or_init(|| {
        // SAFETY: crochet global bas niveau, procédure dans cet exécutable ; il reste
        // installé jusqu'à la fin du processus. Le thread appelant pompe les messages
        // (boucle de l'agent ou du bureau), condition des crochets bas niveau.
        let hook = unsafe {
            SetWindowsHookExW(
                WH_MOUSE_LL,
                Some(mouse_hook),
                GetModuleHandleW(std::ptr::null()),
                0,
            )
        };
        if hook.is_null() {
            log::warn!("crochet souris refusé : ni double-clic ni rectangle sur le bureau");
        }
        HOOK.store(hook as usize, Ordering::SeqCst);
        !hook.is_null()
    })
}

pub fn watch_double_click(on_double_click: Callback) -> bool {
    if ON_DOUBLE_CLICK.set(on_double_click).is_err() {
        return true;
    }
    install_hook()
}

pub fn watch_drag(on_rect: RectCallback) -> bool {
    if ON_DRAG.set(on_rect).is_err() {
        return true;
    }
    install_hook()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_click_needs_time_and_distance() {
        let p = |x, y| POINT { x, y };
        assert!(is_double((1000, p(10, 10)), (1300, p(12, 11)), 500, 4, 4));
        assert!(!is_double((1000, p(10, 10)), (1600, p(10, 10)), 500, 4, 4));
        assert!(!is_double((1000, p(10, 10)), (1100, p(30, 10)), 500, 4, 4));
        // Compteur de millisecondes qui repasse par zéro.
        assert!(is_double(
            (u32::MAX - 100, p(0, 0)),
            (100, p(0, 0)),
            500,
            4,
            4
        ));
    }
}
