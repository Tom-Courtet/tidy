//! Dialogue entre les processus de Tidy et l'agent : une requête JSON par ligne, une
//! réponse JSON par ligne (repris de Snappy).
//!
//! - macOS : socket Unix `tidy.sock` dans le dossier de données ;
//! - Windows : TCP sur 127.0.0.1, port noté dans ce même fichier.
//!
//! [`Request::Subscribe`] garde la connexion ouverte : l'agent y pousse ensuite les
//! [`Event`] du bus, un par ligne. C'est ainsi que le processus du bureau apprend
//! qu'il faut se masquer, relire les réglages ou s'arrêter, et qu'il voit l'agent
//! disparaître (fin du flux).

use std::io::{BufRead, BufReader, Write};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tidy_core::Store;
use tidy_core::events::{Event, EventBus};

/// Délai maximal d'une requête.
pub const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// L'agent tourne-t-il ?
    Ping,
    /// État de l'agent (pour la fenêtre principale).
    Status,
    /// Ouvrir (ou ramener devant) la fenêtre principale.
    OpenManager,
    /// `config.json` a changé : le relire et prévenir les autres processus.
    ReloadSettings,
    /// `desktop.json` a changé.
    ReloadDesktop,
    /// Basculer la visibilité du bureau.
    ToggleDesktop,
    /// Rendre les icônes du Finder (bouton du diagnostic) : repasse en mode superposé.
    RestoreFinder,
    /// Le processus du bureau a posé ses surfaces : en mode géré, l'agent peut masquer
    /// les icônes du Finder.
    DesktopReady,
    /// Recevoir les événements de l'agent sur cette connexion, jusqu'à sa fermeture.
    Subscribe,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Status(Status),
    Error { message: String },
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
                match read_line::<Event>(&mut reader) {
                    Ok(Some(event)) => on_event(event),
                    Ok(None) => break,
                    Err(e) => {
                        log::warn!("événement illisible : {e:#}");
                        break;
                    }
                }
            }
            on_end();
        })
        .context("fil des événements")?;
    Ok(())
}

/// Ouvre le socket de l'agent. Un fichier laissé par un agent arrêté brutalement est
/// retiré ; si un agent répond encore dessus, c'est une erreur.
pub fn bind(store: &Store) -> Result<transport::Listener> {
    let path = store.socket_path();
    if path.exists() {
        if transport::connect(&path).is_ok() {
            bail!("Tidy tourne déjà ({})", path.display());
        }
        std::fs::remove_file(&path)
            .with_context(|| format!("suppression de l'ancien socket {}", path.display()))?;
    }
    transport::listen(&path).with_context(|| format!("socket {}", path.display()))
}

/// Sert les requêtes : un fil par connexion. `handle` rend la réponse de chaque
/// requête ; les abonnés reçoivent les événements de `bus`.
pub fn serve(
    listener: transport::Listener,
    bus: EventBus,
    handle: impl Fn(Request) -> Response + Send + Sync + 'static,
) -> Result<()> {
    let handle = std::sync::Arc::new(handle);
    thread::Builder::new()
        .name("tidy-ipc".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let handle = std::sync::Arc::clone(&handle);
                let bus = bus.clone();
                let spawned =
                    thread::Builder::new()
                        .name("tidy-ipc-conn".into())
                        .spawn(move || {
                            if let Err(e) = serve_one(stream, &bus, &*handle) {
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
    bus: &EventBus,
    handle: &(dyn Fn(Request) -> Response + Send + Sync),
) -> Result<()> {
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let Some(req) = read_line::<Request>(&mut reader)? else {
        return Ok(());
    };
    let mut stream = stream;
    if req == Request::Subscribe {
        let events = bus.subscribe();
        for event in events {
            let last = event == Event::Shutdown;
            if write_line(&mut stream, &event).is_err() || last {
                break;
            }
        }
        return Ok(());
    }
    write_line(&mut stream, &handle(req))
}

/// Écrit un message JSON suivi d'un saut de ligne.
fn write_line<T: Serialize>(w: &mut impl Write, msg: &T) -> Result<()> {
    let mut line = serde_json::to_string(msg)?;
    line.push('\n');
    w.write_all(line.as_bytes())?;
    w.flush()?;
    Ok(())
}

/// Lit un message JSON (une ligne). `None` : fin de flux.
fn read_line<T: for<'de> Deserialize<'de>>(r: &mut impl BufRead) -> Result<Option<T>> {
    let mut line = String::new();
    if std::io::Read::take(&mut *r, 1 << 20).read_line(&mut line)? == 0 {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(line.trim_end())?))
}

/// Canal local : socket Unix, ou TCP sur la boucle locale sous Windows.
pub mod transport {
    use std::io;
    use std::path::Path;

    #[cfg(unix)]
    pub use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};

    #[cfg(unix)]
    pub fn connect(path: &Path) -> io::Result<Stream> {
        Stream::connect(path)
    }

    #[cfg(unix)]
    pub fn listen(path: &Path) -> io::Result<Listener> {
        Listener::bind(path)
    }

    #[cfg(windows)]
    pub use std::net::{TcpListener as Listener, TcpStream as Stream};

    /// Port noté dans le fichier par l'agent.
    #[cfg(windows)]
    pub fn connect(path: &Path) -> io::Result<Stream> {
        let port: u16 = std::fs::read_to_string(path)?
            .trim()
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "port illisible"))?;
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        Stream::connect_timeout(&addr, std::time::Duration::from_millis(500))
    }

    /// Écoute sur un port libre de 127.0.0.1 (injoignable depuis le réseau) et le
    /// note dans `path`.
    #[cfg(windows)]
    pub fn listen(path: &Path) -> io::Result<Listener> {
        let listener = Listener::bind(("127.0.0.1", 0))?;
        std::fs::write(path, listener.local_addr()?.port().to_string())?;
        Ok(listener)
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
        let listener = bind(&store).unwrap();
        serve(listener, bus.clone(), |req| match req {
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

    #[test]
    fn second_agent_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let listener = bind(&store).unwrap();
        serve(listener, EventBus::default(), |_| Response::Ok).unwrap();
        assert!(bind(&store).is_err());
    }
}
