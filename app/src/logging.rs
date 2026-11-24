//! Journaux : console et fichier `logs/<rôle>.log.AAAA-MM-JJ` dans le dossier de
//! données, un fichier par jour, 7 jours gardés.
//!
//! Niveau : `TIDY_LOG` (`error`, `warn`, `info`, `debug`, `trace`), `info` par défaut.
//! Les messages de la crate `log` (egui, winit…) passent aussi par ici.

use tidy_core::Store;
use tracing::level_filters::LevelFilter;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::writer::MakeWriterExt;

const KEEP_DAYS: usize = 7;

/// Installe les journaux ; garder la valeur rendue jusqu'à la fin du processus (elle
/// vide le tampon du fichier en disparaissant).
pub fn init(store: &Store, role: &str) -> Option<WorkerGuard> {
    let level = std::env::var("TIDY_LOG")
        .ok()
        .and_then(|v| v.parse::<LevelFilter>().ok())
        .unwrap_or(LevelFilter::INFO);
    let _ = std::fs::create_dir_all(store.logs_dir());
    let file = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(format!("{role}.log"))
        .max_log_files(KEEP_DAYS)
        .build(store.logs_dir());
    let (writer, guard) = match file {
        Ok(appender) => {
            let (nb, guard) = tracing_appender::non_blocking(appender);
            (Some(nb), Some(guard))
        }
        Err(e) => {
            eprintln!("journal sur disque impossible : {e}");
            (None, None)
        }
    };
    let builder = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(false)
        .with_ansi(false);
    let result = match writer {
        Some(file) => builder.with_writer(std::io::stderr.and(file)).try_init(),
        None => builder.with_writer(std::io::stderr).try_init(),
    };
    if let Err(e) = result {
        eprintln!("journaux déjà installés : {e}");
    }
    guard
}
