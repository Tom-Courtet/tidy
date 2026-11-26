//! Registre de l'utilisateur (`HKEY_CURRENT_USER`), sans droits d'administrateur :
//!
//! - démarrage avec la session : valeur `Tidy` de `…\CurrentVersion\Run` (repris de
//!   Snappy) ;
//! - menu contextuel du bureau : verbe `TidyCreateBox` de
//!   `Software\Classes\DesktopBackground\Shell`. Sous Windows 11, il apparaît dans
//!   « Afficher plus d'options » (le nouveau menu demande une extension empaquetée) ;
//! - installation (lot K, voir `super::install`) : valeurs numériques, lecture et
//!   suppression de clés.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WRITE, REG_DWORD,
    REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW,
    RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};

use super::{from_wide, wide};
use crate::LoginState;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Tidy";
const MENU: &str = r"Software\Classes\DesktopBackground\Shell\TidyCreateBox";

/// Clé ouverte (ou créée), fermée à la fin de `f`.
fn with_key<T>(path: &str, access: u32, create: bool, f: impl FnOnce(HKEY) -> T) -> Result<T> {
    let mut key: HKEY = std::ptr::null_mut();
    let path_w = wide(path);
    // SAFETY: chemin terminé par un zéro ; `key` reçoit la clé ouverte.
    let status = unsafe {
        if create {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path_w.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                access,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        } else {
            RegOpenKeyExW(HKEY_CURRENT_USER, path_w.as_ptr(), 0, access, &mut key)
        }
    };
    if status != ERROR_SUCCESS {
        bail!("clé {path} inaccessible (erreur {status})");
    }
    let out = f(key);
    // SAFETY: clé ouverte ci-dessus.
    unsafe { RegCloseKey(key) };
    Ok(out)
}

/// Écrit une chaîne (`name` vide : valeur par défaut de la clé).
pub(super) fn set_string(path: &str, name: &str, value: &str) -> Result<()> {
    let name_w = wide(name);
    let value_w = wide(value);
    let bytes = u32::try_from(value_w.len() * 2).context("valeur trop longue")?;
    let status = with_key(path, KEY_WRITE, true, |key| {
        // SAFETY: chaînes UTF-16 terminées par un zéro ; `bytes` est la taille de
        // `value_w` en octets.
        unsafe {
            RegSetValueExW(
                key,
                if name.is_empty() {
                    std::ptr::null()
                } else {
                    name_w.as_ptr()
                },
                0,
                REG_SZ,
                value_w.as_ptr().cast(),
                bytes,
            )
        }
    })?;
    if status != ERROR_SUCCESS {
        bail!("écriture de {path}\\{name} refusée (erreur {status})");
    }
    Ok(())
}

/// Écrit un nombre (`REG_DWORD`).
pub(super) fn set_dword(path: &str, name: &str, value: u32) -> Result<()> {
    let name_w = wide(name);
    let bytes = value.to_le_bytes();
    let status = with_key(path, KEY_WRITE, true, |key| {
        // SAFETY: nom terminé par un zéro ; 4 octets pour un REG_DWORD.
        unsafe {
            RegSetValueExW(
                key,
                name_w.as_ptr(),
                0,
                REG_DWORD,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        }
    })?;
    if status != ERROR_SUCCESS {
        bail!("écriture de {path}\\{name} refusée (erreur {status})");
    }
    Ok(())
}

/// Lit une chaîne ; `None` si la clé ou la valeur n'existe pas.
pub(super) fn get_string(path: &str, name: &str) -> Option<String> {
    let path_w = wide(path);
    let name_w = wide(name);
    let mut buf = vec![0u16; 2048];
    let mut bytes = (buf.len() * 2) as u32;
    // SAFETY: chaînes terminées par un zéro ; `bytes` est la taille de `buf` en
    // octets, mise à jour par l'API, qui termine la chaîne par un zéro.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            path_w.as_ptr(),
            name_w.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    (status == ERROR_SUCCESS).then(|| from_wide(&buf))
}

/// Supprime une clé et tout ce qu'elle contient (sans erreur si elle n'existe pas).
pub(super) fn delete_tree(path: &str) -> Result<()> {
    let path_w = wide(path);
    // SAFETY: chemin terminé par un zéro.
    let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, path_w.as_ptr()) };
    if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
        bail!("suppression de la clé {path} refusée (erreur {status})");
    }
    Ok(())
}

fn exe() -> Result<PathBuf> {
    std::env::current_exe().context("chemin de l'exécutable")
}

/// Icône à montrer pour `exe` : `tidy.ico` posé à côté par l'installation, sinon
/// la première icône de l'exécutable.
pub(super) fn icon_location(exe: &Path) -> String {
    let ico = exe.with_file_name("tidy.ico");
    if ico.is_file() {
        ico.display().to_string()
    } else {
        format!("\"{}\",0", exe.display())
    }
}

pub fn set_desktop_menu(label: Option<&str>) -> Result<()> {
    match label {
        Some(label) => set_desktop_menu_for(&exe()?, label),
        None => delete_tree(MENU).context("retrait du menu du bureau"),
    }
}

/// Entrée « Créer une boîte » du menu du bureau, vers l'exécutable `exe`.
pub fn set_desktop_menu_for(exe: &Path, label: &str) -> Result<()> {
    set_string(MENU, "MUIVerb", label)?;
    set_string(MENU, "Icon", &icon_location(exe))?;
    set_string(
        &format!(r"{MENU}\command"),
        "",
        &format!("\"{}\" --create-box", exe.display()),
    )
}

pub fn login_state() -> LoginState {
    let name = wide(RUN_VALUE);
    let found = with_key(RUN, KEY_QUERY_VALUE, false, |key| {
        // SAFETY: on ne demande que l'existence de la valeur.
        let status = unsafe {
            RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        status == ERROR_SUCCESS
    });
    match found {
        Ok(true) => LoginState::On,
        Ok(false) => LoginState::Off,
        Err(e) => LoginState::Unavailable(format!("{e:#}")),
    }
}

pub fn set_login(enabled: bool) -> Result<LoginState> {
    if enabled {
        set_login_for(&exe()?)?;
    } else {
        let name = wide(RUN_VALUE);
        let status = with_key(RUN, KEY_SET_VALUE, false, |key| {
            // SAFETY: nom terminé par un zéro.
            unsafe { RegDeleteValueW(key, name.as_ptr()) }
        })?;
        if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
            bail!("désinscription refusée (erreur {status})");
        }
    }
    Ok(login_state())
}

/// Démarrage avec la session vers l'exécutable `exe`.
pub fn set_login_for(exe: &Path) -> Result<()> {
    set_string(
        RUN,
        RUN_VALUE,
        &format!("\"{}\" --background", exe.display()),
    )
}
