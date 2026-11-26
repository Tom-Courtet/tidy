//! Installation sous Windows (lot K), sans droits d'administrateur : entrée
//! « Applications et fonctionnalités » (`HKCU\…\Uninstall\Tidy`), raccourci du menu
//! Démarrer (`IShellLinkW`), processus de Tidy en cours (`CreateToolhelp32Snapshot`),
//! boîte de message (`MessageBoxW`).

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::{HSTRING, Interface};
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MessageBoxW,
};

use super::{from_wide, registry, wide};
use crate::install::{RegValue, UNINSTALL_KEY};

/// Pas de fenêtre console pour les commandes lancées en arrière-plan.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn write_uninstall_entry(values: &[(&str, RegValue)]) -> Result<()> {
    // Clé refaite de zéro : pas de valeur d'une ancienne version qui traîne.
    registry::delete_tree(UNINSTALL_KEY)?;
    for (name, value) in values {
        match value {
            RegValue::Text(text) => registry::set_string(UNINSTALL_KEY, name, text)?,
            RegValue::Number(n) => registry::set_dword(UNINSTALL_KEY, name, *n)?,
        }
    }
    Ok(())
}

pub fn remove_uninstall_entry() -> Result<()> {
    registry::delete_tree(UNINSTALL_KEY)
}

pub fn uninstall_entry_value(name: &str) -> Option<String> {
    registry::get_string(UNINSTALL_KEY, name)
}

pub fn remove_notification_id() -> Result<()> {
    registry::delete_tree(&format!(
        r"Software\Classes\AppUserModelId\{}",
        super::notify::AUMID
    ))
}

pub fn create_shortcut(
    link: &Path,
    target: &Path,
    icon: Option<&Path>,
    description: &str,
) -> Result<()> {
    if let Some(dir) = link.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("dossier {}", dir.display()))?;
    }
    super::icons::com_init();
    // SAFETY: appels COM documentés (création d'un raccourci `.lnk`) ; chaque chaîne
    // est un HSTRING vivant pendant l'appel.
    unsafe {
        let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .context("raccourci : objet ShellLink indisponible")?;
        shell.SetPath(&HSTRING::from(target))?;
        if let Some(dir) = target.parent() {
            shell.SetWorkingDirectory(&HSTRING::from(dir))?;
        }
        shell.SetDescription(&HSTRING::from(description))?;
        if let Some(icon) = icon {
            shell.SetIconLocation(&HSTRING::from(icon), 0)?;
        }
        let file: IPersistFile = shell.cast()?;
        file.Save(&HSTRING::from(link), true)
            .with_context(|| format!("raccourci {}", link.display()))?;
    }
    Ok(())
}

/// Chemin complet de l'exécutable du processus `pid`, s'il est lisible.
fn image_path(pid: u32) -> Option<PathBuf> {
    // SAFETY: simple ouverture en lecture limitée ; la poignée est fermée plus bas.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` et sa longueur, mise à jour par l'API ; poignée ouverte ci-dessus.
    let ok = unsafe {
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len)
    };
    // SAFETY: poignée ouverte ci-dessus, fermée une seule fois.
    unsafe { CloseHandle(handle) };
    (ok != 0).then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
}

/// Même chemin, sans tenir compte de la casse (système de fichiers de Windows).
fn same(a: &Path, b: &Path) -> bool {
    a.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
}

pub fn processes_of(exes: &[PathBuf]) -> Vec<u32> {
    let me = std::process::id();
    let names: Vec<String> = exes
        .iter()
        .filter_map(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_lowercase())
        .collect();
    // SAFETY: instantané des processus, fermé à la fin.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    // SAFETY: structure de données simple, valide remplie de zéros ; `dwSize` est
    // renseigné comme l'exige l'API.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut found = Vec::new();
    // SAFETY: instantané valide, `entry` initialisée ci-dessus.
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while more {
        let pid = entry.th32ProcessID;
        let name = from_wide(&entry.szExeFile).to_lowercase();
        if pid != me
            && names.contains(&name)
            && image_path(pid).is_some_and(|path| exes.iter().any(|exe| same(&path, exe)))
        {
            found.push(pid);
        }
        // SAFETY: même instantané, même structure.
        more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    // SAFETY: instantané ouvert ci-dessus.
    unsafe { CloseHandle(snapshot) };
    found
}

pub fn message_box(title: &str, text: &str, error: bool) {
    let title = wide(title);
    let text = wide(text);
    let icon = if error {
        MB_ICONERROR
    } else {
        MB_ICONINFORMATION
    };
    // SAFETY: chaînes terminées par un zéro, vivantes pendant l'appel ; pas de
    // fenêtre parente.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_SETFOREGROUND | icon,
        )
    };
}

pub fn delete_after_exit(files: &[PathBuf], dir: Option<&Path>) -> Result<()> {
    // Quelques secondes d'attente (ping), le temps que ce processus se termine et
    // libère son exécutable ; `rmdir` sans `/s` ne retire qu'un dossier vide.
    let mut script = String::from("ping -n 4 127.0.0.1 >nul");
    for file in files {
        script.push_str(&format!(" & del /f /q \"{}\"", file.display()));
    }
    if let Some(dir) = dir {
        script.push_str(&format!(" & rmdir \"{}\"", dir.display()));
    }
    std::process::Command::new("cmd.exe")
        .raw_arg(format!("/d /c \"{script}\""))
        .creation_flags(CREATE_NO_WINDOW)
        .current_dir(std::env::temp_dir())
        .spawn()
        .context("suppression différée")?;
    Ok(())
}
