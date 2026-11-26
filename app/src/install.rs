//! Installation et désinstallation (lot K).
//!
//! **Windows** : `tidy --install` copie l'exécutable dans
//! `%LOCALAPPDATA%\Programs\Tidy\tidy.exe` (sans droits d'administrateur), pose
//! l'icône `tidy.ico` à côté, crée le raccourci du menu Démarrer et l'entrée
//! « Applications et fonctionnalités », repointe le démarrage avec la session et le
//! menu du bureau vers le nouvel emplacement, puis relance Tidy installé. Relancé
//! avec un exécutable plus récent (ou de même version), il arrête Tidy puis remplace
//! l'ancien ; une version installée plus récente n'est remplacée qu'avec `--force`.
//! macOS : rien à installer (glisser `Tidy.app` dans Applications).
//!
//! `tidy --uninstall` (aussi lancé par « Applications et fonctionnalités ») arrête
//! tous les processus de Tidy (le bureau **verrouille** la boîte privée en partant),
//! rend les icônes rangées, les icônes de l'Explorateur ou du Finder et le fond
//! d'écran d'origine, retire tout ce que Tidy a posé dans le système, puis la copie
//! installée. Le dossier de données (coffre, dispositions, réglages) **reste**.
//!
//! La logique de décision (chemins, valeurs du registre, comparaison des versions)
//! est pure et testée ; les écritures passent par `tidy_platform::install`.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use tidy_core::settings::DesktopMode;
use tidy_core::{Settings, Store};
use tidy_platform::LoginState;
use tidy_platform::install::{self as sys, RegValue};
use tidy_ui::t;

use crate::ipc::{self, Request};

/// Nom de l'exécutable installé.
pub const EXE_NAME: &str = "tidy.exe";
/// Icône posée à côté (l'exécutable n'en contient pas).
pub const ICON_NAME: &str = "tidy.ico";
const SHORTCUT_NAME: &str = "Tidy.lnk";
/// Éditeur affiché dans « Applications et fonctionnalités ».
pub const PUBLISHER: &str = "Tidy";
/// Version de cet exécutable.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
static ICON: &[u8] = include_bytes!("../../assets/icon.ico");

/// Attente maximale de l'arrêt de Tidy (le bureau peut prendre jusqu'à 10 s pour
/// verrouiller la boîte privée et rechiffrer les fichiers ouverts).
const STOP_WAIT: Duration = Duration::from_secs(25);

/// Emplacements de l'installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// `%LOCALAPPDATA%\Programs\Tidy`.
    pub dir: PathBuf,
    pub exe: PathBuf,
    pub icon: PathBuf,
    /// `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Tidy.lnk`.
    pub shortcut: PathBuf,
}

impl Layout {
    /// D'après `%LOCALAPPDATA%` et `%APPDATA%`.
    pub fn new(local_app_data: &Path, roaming_app_data: &Path) -> Self {
        let dir = local_app_data.join("Programs").join("Tidy");
        Self {
            exe: dir.join(EXE_NAME),
            icon: dir.join(ICON_NAME),
            shortcut: roaming_app_data
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join(SHORTCUT_NAME),
            dir,
        }
    }

    /// Emplacements de cet utilisateur (Windows seulement).
    pub fn from_env() -> Option<Self> {
        if !cfg!(windows) {
            return None;
        }
        let local = std::env::var_os("LOCALAPPDATA")?;
        let roaming = std::env::var_os("APPDATA")?;
        Some(Self::new(Path::new(&local), Path::new(&roaming)))
    }
}

/// Valeurs de l'entrée « Applications et fonctionnalités ». `size_kb` : taille des
/// fichiers installés ; `date` : `AAAAMMJJ`.
pub fn uninstall_values(
    layout: &Layout,
    version: &str,
    size_kb: u32,
    date: &str,
) -> Vec<(&'static str, RegValue)> {
    let text = |s: String| RegValue::Text(s);
    let uninstall = format!("\"{}\" --uninstall", layout.exe.display());
    vec![
        ("DisplayName", text("Tidy".into())),
        ("DisplayVersion", text(version.into())),
        ("Publisher", text(PUBLISHER.into())),
        ("DisplayIcon", text(layout.icon.display().to_string())),
        ("InstallLocation", text(layout.dir.display().to_string())),
        ("UninstallString", text(uninstall.clone())),
        ("QuietUninstallString", text(uninstall)),
        ("InstallDate", text(date.into())),
        ("EstimatedSize", RegValue::Number(size_kb)),
        ("NoModify", RegValue::Number(1)),
        ("NoRepair", RegValue::Number(1)),
    ]
}

/// Compare deux versions `x.y.z` (un `v` en tête et un suffixe `-…` ou `+…` sont
/// ignorés ; un morceau illisible vaut 0).
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    fn parts(v: &str) -> [u64; 3] {
        let core = v.trim().trim_start_matches('v');
        let core = core.split(['-', '+']).next().unwrap_or_default();
        let mut out = [0; 3];
        for (slot, part) in out.iter_mut().zip(core.split('.')) {
            *slot = part.trim().parse().unwrap_or(0);
        }
        out
    }
    parts(a).cmp(&parts(b))
}

/// Où en est l'installation, vue depuis l'exécutable qui tourne.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Situation {
    /// Cet exécutable est la copie installée.
    Installed,
    /// Rien d'installé : l'installation est proposée.
    NotInstalled,
    /// Une version plus ancienne est installée : la mise à jour est proposée.
    Older { installed: String },
    /// Même version installée (ou version inconnue) : réinstaller la remplace.
    Same { installed: Option<String> },
    /// Une version plus récente est installée : pas de remplacement sans `--force`.
    Newer { installed: String },
}

impl Situation {
    /// L'installation (ou la mise à jour) est à proposer depuis cet exécutable.
    pub fn offers_install(&self) -> bool {
        matches!(
            self,
            Situation::NotInstalled | Situation::Older { .. } | Situation::Same { .. }
        )
    }
}

/// Choix « déjà installé / à installer / plus récent ».
pub fn situation(
    running_installed: bool,
    installed_exists: bool,
    installed_version: Option<&str>,
    current: &str,
) -> Situation {
    if running_installed {
        return Situation::Installed;
    }
    if !installed_exists {
        return Situation::NotInstalled;
    }
    match installed_version {
        None => Situation::Same { installed: None },
        Some(v) => match compare_versions(v, current) {
            Ordering::Less => Situation::Older {
                installed: v.into(),
            },
            Ordering::Equal => Situation::Same {
                installed: Some(v.into()),
            },
            Ordering::Greater => Situation::Newer {
                installed: v.into(),
            },
        },
    }
}

/// Même fichier ? (Chemins canoniques si possible ; casse ignorée sous Windows.)
pub fn same_path(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (a, b) = (canon(a), canon(b));
    if cfg!(windows) {
        a.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    } else {
        a == b
    }
}

/// Emplacements et situation de cet exécutable (Windows seulement).
pub fn current_situation() -> Option<(Layout, Situation)> {
    let layout = Layout::from_env()?;
    let current = std::env::current_exe().ok()?;
    let version = sys::uninstall_entry_value("DisplayVersion");
    let found = situation(
        same_path(&current, &layout.exe),
        layout.exe.is_file(),
        version.as_deref(),
        VERSION,
    );
    Some((layout, found))
}

/// Situation relue à un instant donné.
type Cached = (Instant, Option<(Layout, Situation)>);

/// [`current_situation`], relue au plus toutes les 5 s (fenêtre principale).
pub fn cached_situation() -> Option<(Layout, Situation)> {
    static CACHE: Mutex<Option<Cached>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    match &*cache {
        Some((at, value)) if at.elapsed() < Duration::from_secs(5) => value.clone(),
        _ => {
            let value = current_situation();
            *cache = Some((Instant::now(), value.clone()));
            value
        }
    }
}

/// Lance `tidy <args>` (cet exécutable) en arrière-plan, sans l'attendre.
pub fn spawn_self(args: &[&str]) -> Result<()> {
    let exe = std::env::current_exe().context("chemin de l'exécutable")?;
    std::process::Command::new(exe)
        .args(args)
        .spawn()
        .context("lancement de Tidy")?;
    Ok(())
}

/// Arrête l'agent (qui arrête les autres processus) puis attend la fin de tous les
/// processus lancés depuis `exes`. `false` si certains tournent encore.
fn stop_tidy(store: &Store, exes: &[PathBuf]) -> bool {
    let deadline = Instant::now() + STOP_WAIT;
    if ipc::request(store, &Request::Quit).is_ok() {
        while ipc::request(store, &Request::Ping).is_ok() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    loop {
        let left = sys::processes_of(exes);
        if left.is_empty() {
            return true;
        }
        if Instant::now() >= deadline {
            log::warn!("processus de Tidy encore en marche : {left:?}");
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Copie l'exécutable puis le met en place d'un coup (un fichier temporaire renommé
/// par-dessus l'ancien).
fn copy_exe(from: &Path, layout: &Layout) -> Result<()> {
    std::fs::create_dir_all(&layout.dir)
        .with_context(|| format!("dossier {}", layout.dir.display()))?;
    let tmp = layout.dir.join(format!("{EXE_NAME}.new"));
    std::fs::copy(from, &tmp).with_context(|| format!("copie vers {}", tmp.display()))?;
    std::fs::rename(&tmp, &layout.exe).with_context(|| {
        let _ = std::fs::remove_file(&tmp);
        format!("remplacement de {}", layout.exe.display())
    })
}

/// Date du jour, `AAAAMMJJ`.
fn today() -> String {
    let d = jiff::tz::TimeZone::system()
        .to_datetime(jiff::Timestamp::now())
        .date();
    format!("{:04}{:02}{:02}", d.year(), d.month(), d.day())
}

/// Inscrit la copie installée dans le système : icône, entrée « Applications et
/// fonctionnalités », raccourci, démarrage avec la session, menu du bureau.
fn register(layout: &Layout, settings: &Settings) -> Result<()> {
    std::fs::write(&layout.icon, ICON).with_context(|| layout.icon.display().to_string())?;
    let size = [&layout.exe, &layout.icon]
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum::<u64>();
    let size_kb = u32::try_from(size / 1024).unwrap_or(u32::MAX);
    sys::write_uninstall_entry(&uninstall_values(layout, VERSION, size_kb, &today()))
        .context("entrée « Applications et fonctionnalités »")?;
    sys::create_shortcut(
        &layout.shortcut,
        &layout.exe,
        Some(&layout.icon),
        t!("finish.shortcut_description"),
    )
    .context("raccourci du menu Démarrer")?;
    let platform = tidy_platform::current();
    if settings.launch_at_login || platform.login_item() == LoginState::On {
        sys::set_login_for(&layout.exe).context("démarrage avec Windows")?;
    }
    if settings.desktop_mode != DesktopMode::Off {
        sys::set_desktop_menu_for(&layout.exe, t!("desktop.menu_create_box"))
            .context("menu du bureau")?;
    }
    Ok(())
}

/// `tidy --install` : installe (ou met à jour, ou répare) puis relance Tidy installé.
/// Rend le dossier d'installation.
pub fn install(store: &Store, force: bool) -> Result<PathBuf> {
    let Some(layout) = Layout::from_env() else {
        bail!("{}", t!("finish.install_unsupported"));
    };
    let current = std::env::current_exe().context("chemin de l'exécutable")?;
    let running_installed = same_path(&current, &layout.exe);
    let version = sys::uninstall_entry_value("DisplayVersion");
    let found = situation(
        running_installed,
        layout.exe.is_file(),
        version.as_deref(),
        VERSION,
    );
    if let Situation::Newer { installed } = &found
        && !force
    {
        bail!(
            "{}",
            t!(
                "finish.newer_installed",
                installed = installed,
                current = VERSION
            )
        );
    }
    let settings: Settings = store.load();
    let mut agent_running = ipc::request(store, &Request::Ping).is_ok();
    if !running_installed {
        if !stop_tidy(store, &[current.clone(), layout.exe.clone()]) {
            bail!("{}", t!("finish.still_running"));
        }
        agent_running = false;
        copy_exe(&current, &layout)?;
    }
    register(&layout, &settings)?;
    if !agent_running {
        std::process::Command::new(&layout.exe)
            .current_dir(&layout.dir)
            .spawn()
            .context("lancement de Tidy installé")?;
    }
    log::info!("Tidy {VERSION} installé dans {}", layout.dir.display());
    Ok(layout.dir)
}

/// Compte rendu de la désinstallation.
#[derive(Debug, Default)]
pub struct UninstallReport {
    /// Ce qui a été fait.
    pub done: Vec<String>,
    /// Ce qui a échoué ou reste à faire à la main.
    pub warnings: Vec<String>,
}

impl UninstallReport {
    fn check(&mut self, what: &str, result: Result<()>) {
        match result {
            Ok(()) => self.done.push(what.to_owned()),
            Err(e) => self.warnings.push(format!("{what} : {e:#}")),
        }
    }

    /// Texte complet, pour la console et la boîte de message.
    pub fn text(&self, data_dir: &Path) -> String {
        let mut out = String::new();
        out.push_str(t!("finish.uninstalled"));
        out.push('\n');
        for line in &self.done {
            out.push_str(&format!("  ✓ {line}\n"));
        }
        for line in &self.warnings {
            out.push_str(&format!("  ! {line}\n"));
        }
        out.push('\n');
        out.push_str(&t!(
            "finish.data_kept",
            path = data_dir.display().to_string()
        ));
        out
    }
}

/// Copies en clair laissées dans le coffre (fichiers ouverts lors d'un arrêt brutal).
fn vault_open_copies(store: &Store) -> usize {
    std::fs::read_dir(store.root().join("vault").join("open.noindex"))
        .map(|dir| dir.count())
        .unwrap_or(0)
}

/// `tidy --uninstall` : retire tout ce que Tidy a posé hors de son dossier de
/// données. Ne supprime aucun fichier de l'utilisateur.
pub fn uninstall(store: &Store) -> UninstallReport {
    let mut report = UninstallReport::default();
    let layout = Layout::from_env();
    let current = std::env::current_exe().ok();
    let mut exes: Vec<PathBuf> = current.iter().cloned().collect();
    if let Some(layout) = &layout {
        exes.push(layout.exe.clone());
    }

    // 1. Tous les processus de Tidy ; le bureau verrouille la boîte privée en partant.
    if stop_tidy(store, &exes) {
        report.done.push(t!("finish.step_stopped").into());
    } else {
        report.warnings.push(t!("finish.still_running").into());
    }
    match vault_open_copies(store) {
        0 => report.done.push(t!("finish.step_vault_locked").into()),
        _ => report.warnings.push(t!("finish.vault_open_copies").into()),
    }

    // 2. Icônes et fond d'écran.
    let platform = tidy_platform::current();
    if platform.desktop_icons_hidden() {
        report.check(
            t!("finish.step_explorer_icons"),
            platform.set_desktop_icons_hidden(false),
        );
    }
    let stashed = crate::icons::restore_all(store);
    if stashed > 0 {
        report
            .done
            .push(t!("finish.step_icons_restored", count = stashed));
    }
    let marker = store.finder_marker_path();
    match tidy_platform::finder::restore_icons(&tidy_platform::finder::SystemFinder, &marker) {
        Ok(true) => report.done.push(t!("finish.step_finder_icons").into()),
        Ok(false) => {}
        Err(e) => report
            .warnings
            .push(format!("{} : {e:#}", t!("finish.step_finder_icons"))),
    }
    match crate::agent::wallpaper::restore_original(store) {
        Ok(true) => report.done.push(t!("finish.step_wallpaper").into()),
        Ok(false) => {}
        Err(e) => report
            .warnings
            .push(format!("{} : {e:#}", t!("finish.step_wallpaper"))),
    }

    // 3. Entrées du système (le menu du bureau n'existe que sous Windows).
    let menu = platform.set_desktop_menu(None);
    if cfg!(windows) || menu.is_err() {
        report.check(t!("finish.step_desktop_menu"), menu);
    }
    if matches!(
        platform.login_item(),
        LoginState::On | LoginState::NeedsApproval
    ) {
        report.check(
            t!("finish.step_login"),
            platform.set_login_item(false).map(|_| ()),
        );
    }
    if let Some(layout) = &layout {
        report.check(
            t!("finish.step_notifications"),
            sys::remove_notification_id(),
        );
        report.check(
            t!("finish.step_uninstall_entry"),
            sys::remove_uninstall_entry(),
        );
        if layout.shortcut.is_file() {
            report.check(
                t!("finish.step_shortcut"),
                std::fs::remove_file(&layout.shortcut).map_err(Into::into),
            );
        }
        // 4. La copie installée (jamais l'exécutable téléchargé par l'utilisateur).
        if layout.exe.is_file() {
            let running_installed = current
                .as_deref()
                .is_some_and(|c| same_path(c, &layout.exe));
            let result = if running_installed {
                sys::delete_after_exit(
                    &[layout.exe.clone(), layout.icon.clone()],
                    Some(&layout.dir),
                )
            } else {
                remove_installed_files(layout)
            };
            report.check(t!("finish.step_program"), result);
        }
    }
    report
}

/// Retire les fichiers posés par l'installation, puis leur dossier s'il est vide.
fn remove_installed_files(layout: &Layout) -> Result<()> {
    for file in [
        &layout.exe,
        &layout.icon,
        &layout.dir.join(format!("{EXE_NAME}.new")),
    ] {
        if file.is_file() {
            std::fs::remove_file(file).with_context(|| file.display().to_string())?;
        }
    }
    // Sans erreur s'il contient encore autre chose.
    let _ = std::fs::remove_dir(&layout.dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_paths_follow_windows_conventions() {
        let layout = Layout::new(
            Path::new("/u/AppData/Local"),
            Path::new("/u/AppData/Roaming"),
        );
        assert_eq!(layout.dir, Path::new("/u/AppData/Local/Programs/Tidy"));
        assert_eq!(
            layout.exe,
            Path::new("/u/AppData/Local/Programs/Tidy/tidy.exe")
        );
        assert_eq!(
            layout.icon,
            Path::new("/u/AppData/Local/Programs/Tidy/tidy.ico")
        );
        assert_eq!(
            layout.shortcut,
            Path::new("/u/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Tidy.lnk")
        );
        if !cfg!(windows) {
            assert_eq!(Layout::from_env(), None);
        }
    }

    #[test]
    fn uninstall_entry_points_to_installed_copy() {
        let layout = Layout::new(Path::new("/L"), Path::new("/R"));
        let values = uninstall_values(&layout, "1.2.3", 9000, "20261126");
        let get = |name: &str| {
            values
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| panic!("{name} absent"))
        };
        let text = |s: &str| RegValue::Text(s.into());
        assert_eq!(get("DisplayName"), text("Tidy"));
        assert_eq!(get("DisplayVersion"), text("1.2.3"));
        assert_eq!(get("Publisher"), text(PUBLISHER));
        // Séparateurs du système (`\` sous Windows).
        let shown = |p: &Path| p.display().to_string().replace('\\', "/");
        let shown_value = |v: RegValue| match v {
            RegValue::Text(t) => t.replace('\\', "/"),
            RegValue::Number(n) => n.to_string(),
        };
        assert_eq!(shown(&layout.icon), "/L/Programs/Tidy/tidy.ico");
        assert_eq!(shown_value(get("DisplayIcon")), "/L/Programs/Tidy/tidy.ico");
        assert_eq!(shown_value(get("InstallLocation")), "/L/Programs/Tidy");
        assert_eq!(
            shown_value(get("UninstallString")),
            "\"/L/Programs/Tidy/tidy.exe\" --uninstall"
        );
        assert_eq!(get("QuietUninstallString"), get("UninstallString"));
        assert_eq!(get("InstallDate"), text("20261126"));
        assert_eq!(get("EstimatedSize"), RegValue::Number(9000));
        assert_eq!(get("NoModify"), RegValue::Number(1));
        assert_eq!(get("NoRepair"), RegValue::Number(1));
        // Pas de doublon.
        let mut names: Vec<_> = values.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), values.len());
    }

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(compare_versions("0.1.0", "0.1.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.2.0", "0.10.0"), Ordering::Less);
        assert_eq!(compare_versions("v1.0.0", "1.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.0.1-beta", "1.0.0"), Ordering::Greater);
        assert_eq!(compare_versions("n'importe", "0.0.0"), Ordering::Equal);
    }

    #[test]
    fn situation_choices() {
        use Situation::*;
        assert_eq!(situation(true, true, Some("9.0.0"), "0.1.0"), Installed);
        assert_eq!(
            situation(false, false, Some("0.1.0"), "0.1.0"),
            NotInstalled
        );
        assert_eq!(
            situation(false, true, Some("0.0.9"), "0.1.0"),
            Older {
                installed: "0.0.9".into()
            }
        );
        assert_eq!(
            situation(false, true, Some("0.1.0"), "0.1.0"),
            Same {
                installed: Some("0.1.0".into())
            }
        );
        assert_eq!(
            situation(false, true, None, "0.1.0"),
            Same { installed: None }
        );
        assert_eq!(
            situation(false, true, Some("0.2.0"), "0.1.0"),
            Newer {
                installed: "0.2.0".into()
            }
        );
        assert!(NotInstalled.offers_install());
        assert!(!Installed.offers_install());
        assert!(
            !Newer {
                installed: "1".into()
            }
            .offers_install()
        );
    }

    #[test]
    fn same_path_resolves_links_and_dots() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tidy.exe");
        std::fs::write(&file, b"x").unwrap();
        assert!(same_path(&file, &dir.path().join(".").join("tidy.exe")));
        assert!(!same_path(&file, &dir.path().join("autre.exe")));
    }

    #[test]
    fn installed_files_are_removed_but_not_others() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path(), root.path());
        std::fs::create_dir_all(&layout.dir).unwrap();
        std::fs::write(&layout.exe, b"exe").unwrap();
        std::fs::write(&layout.icon, b"ico").unwrap();
        let other = layout.dir.join("notes.txt");
        std::fs::write(&other, b"a moi").unwrap();
        remove_installed_files(&layout).unwrap();
        assert!(!layout.exe.exists() && !layout.icon.exists());
        // Un fichier étranger reste, et son dossier aussi.
        assert!(other.exists());
        std::fs::remove_file(&other).unwrap();
        remove_installed_files(&layout).unwrap();
        assert!(!layout.dir.exists());
    }

    #[test]
    fn copy_replaces_previous_exe() {
        let root = tempfile::tempdir().unwrap();
        let layout = Layout::new(root.path(), root.path());
        let new = root.path().join("téléchargé.exe");
        std::fs::write(&new, b"v2").unwrap();
        std::fs::create_dir_all(&layout.dir).unwrap();
        std::fs::write(&layout.exe, b"v1").unwrap();
        copy_exe(&new, &layout).unwrap();
        assert_eq!(std::fs::read(&layout.exe).unwrap(), b"v2");
        assert!(new.exists(), "l'exécutable d'origine reste");
        assert!(!layout.dir.join("tidy.exe.new").exists());
    }

    #[test]
    fn embedded_icon_is_an_ico() {
        assert_eq!(&ICON[..4], &[0, 0, 1, 0]);
    }
}
