//! Dialogue entre les processus de Tidy et l'agent : une requête JSON par ligne, une
//! réponse JSON par ligne (repris de Snappy).
//!
//! - macOS : socket Unix `tidy.sock` dans le dossier de données, lisible et
//!   inscriptible par le seul utilisateur (`0600`) ;
//! - Windows : TCP sur 127.0.0.1, port et jeton notés dans ce même fichier (dossier de
//!   données de l'utilisateur). Le client envoie le jeton en première ligne ; sans le
//!   bon jeton, la connexion est fermée (voir [`tcp`]).
//!
//! Une seule instance de l'agent : il tient le verrou `agent.lock` ([`lock_instance`]).
//!
//! [`Request::Subscribe`] garde la connexion ouverte : l'agent y pousse ensuite les
//! [`Event`] du bus, un par ligne. C'est ainsi que le processus du bureau apprend
//! qu'il faut se masquer, relire les réglages ou s'arrêter, et qu'il voit l'agent
//! disparaître (fin du flux).

use std::io::{BufRead, BufReader, Write};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tidy_core::Store;
use tidy_core::events::{Event, EventBus};

/// Délai maximal d'une requête.
pub const TIMEOUT: Duration = Duration::from_secs(5);
/// Longueur maximale d'un message (une ligne JSON).
const MAX_LINE: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// L'agent tourne-t-il ?
    Ping,
    /// État de l'agent (pour la fenêtre principale).
    Status,
    /// Ouvrir (ou ramener devant) la fenêtre principale, sur `page` si donnée.
    OpenManager {
        #[serde(default)]
        page: Option<String>,
    },
    /// `config.json` a changé : le relire et prévenir les autres processus.
    ReloadSettings,
    /// `desktop.json` a changé.
    ReloadDesktop,
    /// Basculer la visibilité du bureau.
    ToggleDesktop,
    /// Créer une boîte sous la souris (menu du bureau, `tidy --create-box`).
    CreateBox,
    /// Modifier l'état du bureau (seul le processus du bureau écrit `desktop.json` ;
    /// l'agent relaie, ou applique lui-même si le bureau ne tourne pas).
    Desktop {
        command: tidy_core::DesktopCommand,
    },
    /// Appliquer les règles de rangement : à tous les éléments (`all`) ou seulement
    /// aux non affectés.
    ApplyRules {
        all: bool,
    },
    /// Annuler le dernier rangement automatique (« Tout annuler »).
    UndoOrganize,
    /// Rendre les icônes du Finder (bouton du diagnostic) : repasse en mode superposé.
    RestoreFinder,
    /// Le processus du bureau a posé ses surfaces : en mode géré, l'agent peut masquer
    /// les icônes du Finder.
    DesktopReady,
    /// Recevoir les événements de l'agent sur cette connexion, jusqu'à sa fermeture.
    Subscribe,
    Quit,
    /// Demande à un module des vagues 2 et 3 (`"layouts"`, `"search"`, `"vault"`,
    /// `"widgets"`, `"wallpaper"`…). Traitée par `agent::modules::handle` ; à défaut,
    /// relayée à tous les abonnés en [`tidy_core::events::Event::Module`].
    Module {
        module: String,
        action: String,
        #[serde(default)]
        data: serde_json::Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Status(Status),
    Error {
        message: String,
    },
    /// Réponse d'un module, propre à lui.
    Data {
        data: serde_json::Value,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub version: String,
    /// Processus du bureau en marche.
    pub desktop_running: bool,
    /// Icônes du Finder masquées par Tidy.
    pub finder_hidden: bool,
    pub desktop_hidden: bool,
}

/// Envoie une requête à l'agent et attend sa réponse.
pub fn request(store: &Store, req: &Request) -> Result<Response> {
    let socket = store.socket_path();
    let mut stream = transport::connect(&socket)
        .with_context(|| format!("agent injoignable ({})", socket.display()))?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    write_line(&mut stream, req)?;
    let mut reader = BufReader::new(stream);
    read_line(&mut reader)?.context("l'agent a fermé la connexion sans répondre")
}

/// S'abonne aux événements de l'agent : `on_event` est appelé pour chacun, sur un fil
/// à part ; `on_end` quand l'agent ferme la connexion (arrêt ou plantage).
pub fn subscribe(
    store: &Store,
    on_event: impl Fn(Event) + Send + 'static,
    on_end: impl FnOnce() + Send + 'static,
) -> Result<()> {
    let socket = store.socket_path();
    let mut stream = transport::connect(&socket)
        .with_context(|| format!("agent injoignable ({})", socket.display()))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    write_line(&mut stream, &Request::Subscribe)?;
    thread::Builder::new()
        .name("tidy-events".into())
        .spawn(move || {
            let mut reader = BufReader::new(stream);
            loop {
                match read_message::<Event>(&mut reader) {
                    Ok(Line::Message(event)) => on_event(event),
                    // Un événement illisible (trop long, d'une version plus récente)
                    // est ignoré : l'abonnement continue.
                    Ok(Line::Bad(why)) => log::warn!("événement ignoré : {why}"),
                    Ok(Line::End) => break,
                    Err(e) => {
                        log::warn!("abonnement coupé : {e}");
                        break;
                    }
                }
            }
            on_end();
        })
        .context("fil des événements")?;
    Ok(())
}

/// Verrou d'instance unique de l'agent, tenu tant que la valeur vit.
#[derive(Debug)]
pub struct InstanceLock(#[allow(dead_code)] std::fs::File);

/// Prend le verrou `agent.lock` du dossier de données. `None` : un autre agent le tient
/// (Tidy tourne déjà). Le système le rend si l'agent s'arrête, même brutalement.
pub fn lock_instance(store: &Store) -> Result<Option<InstanceLock>> {
    let path = store.root().join("agent.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .with_context(|| format!("verrou {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(InstanceLock(file))),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => {
            Err(e).with_context(|| format!("verrou {}", path.display()))
        }
    }
}

/// Ouvre le socket de l'agent. L'appelant tient [`InstanceLock`] : un fichier restant
/// vient d'un agent arrêté (y compris un fichier de port dont le port a été repris par
/// un autre programme) ; il est remplacé.
pub fn bind(store: &Store, _lock: &InstanceLock) -> Result<(transport::Listener, transport::Auth)> {
    let path = store.socket_path();
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("suppression de l'ancien socket {}", path.display()))?;
    }
    transport::listen(&path).with_context(|| format!("socket {}", path.display()))
}

/// Sert les requêtes : un fil par connexion. `handle` rend la réponse de chaque
/// requête ; les abonnés reçoivent les événements de `bus`. Une connexion qui ne
/// présente pas le bon jeton (Windows) est fermée sans réponse.
pub fn serve(
    listener: transport::Listener,
    auth: transport::Auth,
    bus: EventBus,
    handle: impl Fn(Request) -> Response + Send + Sync + 'static,
) -> Result<()> {
    let handle = std::sync::Arc::new(handle);
    let auth = std::sync::Arc::new(auth);
    thread::Builder::new()
        .name("tidy-ipc".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let handle = std::sync::Arc::clone(&handle);
                let auth = std::sync::Arc::clone(&auth);
                let bus = bus.clone();
                let spawned =
                    thread::Builder::new()
                        .name("tidy-ipc-conn".into())
                        .spawn(move || {
                            if let Err(e) = serve_one(stream, &auth, &bus, &*handle) {
                                log::warn!("requête sur le socket : {e:#}");
                            }
                        });
                if let Err(e) = spawned {
                    log::warn!("fil de connexion : {e}");
                }
            }
        })
        .context("fil du socket")?;
    Ok(())
}

fn serve_one(
    stream: transport::Stream,
    auth: &transport::Auth,
    bus: &EventBus,
    handle: &(dyn Fn(Request) -> Response + Send + Sync),
) -> Result<()> {
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    if !auth.verify(&mut reader) {
        log::warn!("connexion refusée : jeton absent ou faux");
        return Ok(());
    }
    let Some(req) = read_line::<Request>(&mut reader)? else {
        return Ok(());
    };
    let mut stream = stream;
    if req == Request::Subscribe {
        let events = bus.subscribe();
        for event in events {
            let last = event == Event::Shutdown;
            // Un événement impossible à envoyer est ignoré (journal) : il ne coupe pas
            // l'abonnement (le bureau s'arrêterait).
            let Some(line) = event_line(&event) else {
                continue;
            };
            if write_raw(&mut stream, &line).is_err() || last {
                break;
            }
        }
        return Ok(());
    }
    write_line(&mut stream, &handle(req))
}

/// Écrit un message JSON suivi d'un saut de ligne.
fn write_line<T: Serialize>(w: &mut impl Write, msg: &T) -> Result<()> {
    let line = serde_json::to_string(msg)?;
    write_raw(w, &line)
}

/// Écrit une ligne déjà sérialisée.
fn write_raw(w: &mut impl Write, line: &str) -> Result<()> {
    w.write_all(line.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()?;
    Ok(())
}

/// Ligne JSON d'un événement, ou `None` (journal) s'il ne peut pas être envoyé :
/// sérialisation impossible, ou plus long que [`MAX_LINE`].
fn event_line(event: &Event) -> Option<String> {
    match serde_json::to_string(event) {
        Ok(line) if line.len() < MAX_LINE => Some(line),
        Ok(line) => {
            log::warn!(
                "événement trop long ({} octets) : non envoyé aux abonnés",
                line.len()
            );
            None
        }
        Err(e) => {
            log::warn!("événement impossible à envoyer : {e}");
            None
        }
    }
}

/// Ligne lue sur un abonnement.
enum Line<T> {
    Message(T),
    /// Ligne illisible ou trop longue (ignorée), avec la raison.
    Bad(String),
    /// Fin du flux.
    End,
}

/// Lit une ligne JSON sans jamais en garder plus de [`MAX_LINE`] octets : une ligne
/// trop longue est sautée jusqu'à son saut de ligne.
fn read_message<T: for<'de> Deserialize<'de>>(r: &mut impl BufRead) -> std::io::Result<Line<T>> {
    let mut buf = Vec::new();
    let n = std::io::Read::take(&mut *r, MAX_LINE as u64).read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(Line::End);
    }
    if buf.last() != Some(&b'\n') && buf.len() >= MAX_LINE {
        // Reste de la ligne : lu et jeté, par morceaux.
        let mut rest = Vec::new();
        loop {
            rest.clear();
            let k = std::io::Read::take(&mut *r, 1 << 16).read_until(b'\n', &mut rest)?;
            if k == 0 || rest.last() == Some(&b'\n') {
                break;
            }
        }
        return Ok(Line::Bad("ligne trop longue".into()));
    }
    let text = String::from_utf8_lossy(&buf);
    Ok(match serde_json::from_str(text.trim_end()) {
        Ok(msg) => Line::Message(msg),
        Err(e) => Line::Bad(e.to_string()),
    })
}

/// Lit un message JSON (une ligne). `None` : fin de flux.
fn read_line<T: for<'de> Deserialize<'de>>(r: &mut impl BufRead) -> Result<Option<T>> {
    let mut line = String::new();
    if std::io::Read::take(&mut *r, MAX_LINE as u64).read_line(&mut line)? == 0 {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(line.trim_end())?))
}

/// Canal local : socket Unix, ou TCP sur la boucle locale sous Windows.
pub mod transport {
    #[cfg(unix)]
    use std::io;
    #[cfg(unix)]
    use std::path::Path;

    #[cfg(unix)]
    pub use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};

    #[cfg(unix)]
    pub fn connect(path: &Path) -> io::Result<Stream> {
        Stream::connect(path)
    }

    /// Socket réservé à l'utilisateur (`0600`) : les autres comptes de la machine ne
    /// peuvent ni s'y connecter ni le lire.
    #[cfg(unix)]
    pub fn listen(path: &Path) -> io::Result<(Listener, Auth)> {
        use std::os::unix::fs::PermissionsExt;
        let listener = Listener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok((listener, Auth))
    }

    /// Sous Unix, les droits du socket suffisent : pas de jeton.
    #[cfg(unix)]
    #[derive(Debug)]
    pub struct Auth;

    #[cfg(unix)]
    impl Auth {
        pub fn verify(&self, _: &mut impl std::io::BufRead) -> bool {
            true
        }
    }

    #[cfg(windows)]
    pub use super::tcp::{Auth, connect, listen};
    #[cfg(windows)]
    pub use std::net::{TcpListener as Listener, TcpStream as Stream};
}

/// TCP sur 127.0.0.1 avec jeton : transport de Windows (compilé partout pour les
/// essais). L'agent tire un jeton aléatoire de 32 octets et l'écrit, avec le port,
/// dans le fichier du port (dossier de données de l'utilisateur) ; un client envoie le
/// jeton en première ligne. Un autre compte de la machine, ou une page web qui viserait
/// le port, ne connaît pas le jeton : sa connexion est fermée.
#[cfg_attr(not(windows), allow(dead_code))]
pub mod tcp {
    use std::io::{self, BufRead, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::path::Path;
    use std::time::Duration;

    /// Longueur du jeton, en octets.
    pub const TOKEN_LEN: usize = 32;

    /// Jeton attendu des clients.
    pub struct Auth([u8; TOKEN_LEN]);

    impl std::fmt::Debug for Auth {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            // Jamais le jeton dans un journal.
            f.write_str("Auth(…)")
        }
    }

    impl Auth {
        /// Lit la première ligne (le jeton) et la compare en temps constant. `false` :
        /// connexion à fermer.
        pub fn verify(&self, r: &mut impl BufRead) -> bool {
            let mut line = String::new();
            let limit = (2 * TOKEN_LEN + 2) as u64;
            if r.take(limit).read_line(&mut line).is_err() {
                return false;
            }
            unhex(line.trim_end()).is_some_and(|given| constant_time_eq(&given, &self.0))
        }
    }

    /// Écoute sur un port libre de 127.0.0.1 (injoignable depuis le réseau) ; note le
    /// port et un nouveau jeton dans `path`.
    pub fn listen(path: &Path) -> io::Result<(TcpListener, Auth)> {
        let mut token = [0u8; TOKEN_LEN];
        getrandom::fill(&mut token).map_err(|e| io::Error::other(e.to_string()))?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let text = format!("{}\n{}\n", listener.local_addr()?.port(), hex(&token));
        tidy_core::store::write_atomic(path, text.as_bytes()).map_err(io::Error::other)?;
        Ok((listener, Auth(token)))
    }

    /// Se connecte à l'agent et présente le jeton du fichier du port.
    pub fn connect(path: &Path) -> io::Result<TcpStream> {
        let text = std::fs::read_to_string(path)?;
        let (port, token) = parse(&text).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "fichier du port illisible")
        })?;
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500))?;
        stream.write_all(format!("{token}\n").as_bytes())?;
        Ok(stream)
    }

    /// Port et jeton (hexadécimal) du fichier du port.
    fn parse(text: &str) -> Option<(u16, &str)> {
        let mut lines = text.lines();
        let port = lines.next()?.trim().parse().ok()?;
        let token = lines.next()?.trim();
        (token.len() == 2 * TOKEN_LEN).then_some((port, token))
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn unhex(text: &str) -> Option<Vec<u8>> {
        if text.len() != 2 * TOKEN_LEN || !text.is_ascii() {
            return None;
        }
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
            .collect()
    }

    /// Comparaison sans sortie anticipée (même durée quel que soit le premier octet
    /// différent).
    fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        let diff = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
        std::hint::black_box(diff) == 0
    }

    #[cfg(test)]
    mod tests {
        use std::io::BufReader;

        use super::*;

        #[test]
        fn only_the_right_token_gets_in() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("tidy.sock");
            let (listener, auth) = listen(&path).unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            let (port, token) = parse(&text).unwrap();
            assert_eq!(port, listener.local_addr().unwrap().port());
            assert_eq!(token.len(), 64);

            // Bon jeton.
            let _client = connect(&path).unwrap();
            let (server, _) = listener.accept().unwrap();
            assert!(auth.verify(&mut BufReader::new(server)));

            // Jeton faux, absent, ou trop long.
            for bad in [
                format!("{}\n", "0".repeat(64)),
                "{\"type\":\"ping\"}\n".to_owned(),
                format!("{token}{token}\n"),
            ] {
                let mut client =
                    TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], port))).unwrap();
                client.write_all(bad.as_bytes()).unwrap();
                let (server, _) = listener.accept().unwrap();
                assert!(!auth.verify(&mut BufReader::new(server)), "{bad}");
            }
        }

        #[test]
        fn tokens_differ_and_compare_safely() {
            let dir = tempfile::tempdir().unwrap();
            let (_, a) = listen(&dir.path().join("a")).unwrap();
            let (_, b) = listen(&dir.path().join("b")).unwrap();
            assert_ne!(a.0, b.0);
            assert!(constant_time_eq(&a.0, &a.0));
            assert!(!constant_time_eq(&a.0, &b.0));
            assert!(!constant_time_eq(&a.0, &a.0[..31]));
            assert_eq!(unhex(&hex(&a.0)).unwrap(), a.0);
            assert_eq!(format!("{a:?}"), "Auth(…)");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn requests_and_subscriptions_work() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let bus = EventBus::default();
        let lock = lock_instance(&store).unwrap().unwrap();
        let (listener, auth) = bind(&store, &lock).unwrap();
        serve(listener, auth, bus.clone(), |req| match req {
            Request::Ping => Response::Ok,
            _ => Response::Error {
                message: "non".into(),
            },
        })
        .unwrap();

        assert_eq!(request(&store, &Request::Ping).unwrap(), Response::Ok);
        assert!(matches!(
            request(&store, &Request::Quit).unwrap(),
            Response::Error { .. }
        ));

        let (tx, rx) = mpsc::channel();
        let (end_tx, end_rx) = mpsc::channel();
        subscribe(
            &store,
            move |e| tx.send(e).unwrap(),
            move || end_tx.send(()).unwrap(),
        )
        .unwrap();
        // Attendre que l'abonnement soit enregistré côté agent.
        for _ in 0..100 {
            if bus.subscriber_count() > 0 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        bus.publish(Event::ToggleDesktop);
        bus.publish(Event::Shutdown);
        assert_eq!(rx.recv_timeout(TIMEOUT).unwrap(), Event::ToggleDesktop);
        assert_eq!(rx.recv_timeout(TIMEOUT).unwrap(), Event::Shutdown);
        end_rx.recv_timeout(TIMEOUT).unwrap();
    }

    /// Un événement trop long (ou illisible) est ignoré sans couper l'abonnement.
    #[test]
    fn one_bad_event_does_not_end_the_subscription() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let bus = EventBus::default();
        let lock = lock_instance(&store).unwrap().unwrap();
        let (listener, auth) = bind(&store, &lock).unwrap();
        serve(listener, auth, bus.clone(), |_| Response::Ok).unwrap();
        let (tx, rx) = mpsc::channel();
        subscribe(&store, move |e| tx.send(e).unwrap(), || {}).unwrap();
        for _ in 0..100 {
            if bus.subscriber_count() > 0 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        bus.publish(Event::Module {
            module: "essai".into(),
            action: "gros".into(),
            data: serde_json::Value::String("x".repeat(MAX_LINE + 10)),
        });
        bus.publish(Event::ToggleDesktop);
        assert_eq!(rx.recv_timeout(TIMEOUT).unwrap(), Event::ToggleDesktop);
    }

    /// Côté abonné : ligne illisible ou trop longue sautée, la suivante est lue.
    #[test]
    fn bad_lines_are_skipped_by_the_reader() {
        let long = "y".repeat(MAX_LINE + 5);
        let text = format!(
            "{{\"type\":\"inconnu_plus_recent\"}}\n{long}\n{}\n",
            serde_json::to_string(&Event::ToggleDesktop).unwrap()
        );
        let mut r = std::io::BufReader::new(text.as_bytes());
        assert!(matches!(
            read_message::<Event>(&mut r).unwrap(),
            Line::Bad(_)
        ));
        assert!(matches!(
            read_message::<Event>(&mut r).unwrap(),
            Line::Bad(_)
        ));
        assert!(matches!(
            read_message::<Event>(&mut r).unwrap(),
            Line::Message(Event::ToggleDesktop)
        ));
        assert!(matches!(read_message::<Event>(&mut r).unwrap(), Line::End));
    }

    #[test]
    fn second_agent_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let lock = lock_instance(&store).unwrap().expect("premier agent");
        assert!(lock_instance(&store).unwrap().is_none(), "deuxième agent");
        drop(lock);
        assert!(lock_instance(&store).unwrap().is_some(), "verrou rendu");
    }

    /// Fichier du socket laissé par un agent arrêté (ou port repris par un autre
    /// programme) : le nouvel agent, qui tient le verrou, le remplace.
    #[test]
    fn a_stale_socket_file_does_not_block_the_agent() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        std::fs::write(store.socket_path(), b"9\n").unwrap();
        let lock = lock_instance(&store).unwrap().unwrap();
        let (listener, auth) = bind(&store, &lock).unwrap();
        serve(listener, auth, EventBus::default(), |_| Response::Ok).unwrap();
        assert_eq!(request(&store, &Request::Ping).unwrap(), Response::Ok);
    }

    #[cfg(unix)]
    #[test]
    fn the_socket_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let lock = lock_instance(&store).unwrap().unwrap();
        let _listener = bind(&store, &lock).unwrap();
        let mode = std::fs::metadata(store.socket_path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
