//! Travail en arrière-plan : un fil nommé par tâche, résultat relevé sans bloquer par
//! le fil de l'interface. Le fil appelle le [`Waker`] en fin de travail (et à chaque
//! morceau d'un flux) pour faire redessiner la fenêtre du widget.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// Réveille la fenêtre qui attend le résultat (`ctx.request_repaint_of(...)` côté egui).
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Waker qui ne fait rien (tests).
pub fn no_waker() -> Waker {
    Arc::new(|| {})
}

/// État d'une tâche relevée.
#[derive(Debug, PartialEq)]
pub enum Polled<T> {
    /// Toujours en cours.
    Pending,
    /// Terminée, voici son résultat (rendu une seule fois).
    Done(T),
    /// Le fil s'est arrêté sans résultat (panique) : à traiter comme une erreur.
    Lost,
}

/// Tâche lancée sur un fil à part.
pub struct Task<T> {
    rx: Receiver<T>,
}

impl<T: Send + 'static> Task<T> {
    /// Lance `work` sur un fil nommé `name`.
    pub fn spawn(name: &str, waker: Waker, work: impl FnOnce() -> T + Send + 'static) -> Self {
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let out = work();
                let _ = tx.send(out);
                waker();
            });
        if let Err(e) = spawned {
            log::error!("fil {name} impossible à lancer : {e}");
        }
        Self { rx }
    }

    /// Relève le résultat sans attendre.
    pub fn poll(&self) -> Polled<T> {
        match self.rx.try_recv() {
            Ok(out) => Polled::Done(out),
            Err(TryRecvError::Empty) => Polled::Pending,
            Err(TryRecvError::Disconnected) => Polled::Lost,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_task_hands_its_result_once() {
        let task = Task::spawn("essai", no_waker(), || 41 + 1);
        let start = Instant::now();
        loop {
            match task.poll() {
                Polled::Pending => {
                    assert!(start.elapsed() < Duration::from_secs(5));
                    std::thread::yield_now();
                }
                Polled::Done(v) => {
                    assert_eq!(v, 42);
                    break;
                }
                Polled::Lost => panic!("tâche perdue"),
            }
        }
        assert_eq!(task.poll(), Polled::Lost);
    }
}
