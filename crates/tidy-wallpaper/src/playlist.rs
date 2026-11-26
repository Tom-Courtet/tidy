//! Playlist : quel fond montrer, et quand passer au suivant.
//!
//! - **dans l'ordre** : le suivant dans la liste, en boucle ;
//! - **aléatoire** : un « sac » mélangé, vidé fond par fond puis remélangé ; aucun fond
//!   ne revient avant que tous les autres soient passés, et jamais deux fois de suite
//!   (même à la jonction de deux sacs).
//!
//! L'état ne vit qu'en mémoire (agent) : un redémarrage repart du premier fond (ou d'un
//! nouveau tirage).

use std::time::{Duration, Instant};

use tidy_core::settings::wallpaper::{PlaylistOrder, PlaylistSettings};
use uuid::Uuid;

/// Générateur pseudo-aléatoire minimal (xorshift64*), sans dépendance : il ne sert
/// qu'à mélanger des fonds d'écran.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    /// Graine tirée de l'heure.
    pub fn from_time() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Self::new(nanos ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Entier dans `0..n` (`n` > 0).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Durée d'un fond (bornée de 5 min à 24 h).
pub fn interval(settings: &PlaylistSettings) -> Duration {
    Duration::from_secs(u64::from(settings.interval()) * 60)
}

#[derive(Debug, Clone)]
pub struct PlaylistState {
    current: Option<Uuid>,
    switched_at: Option<Instant>,
    /// Fonds restant à tirer (aléatoire), le prochain en dernier.
    bag: Vec<Uuid>,
    rng: Rng,
}

impl Default for PlaylistState {
    fn default() -> Self {
        Self::new(Rng::from_time())
    }
}

impl PlaylistState {
    pub fn new(rng: Rng) -> Self {
        Self {
            current: None,
            switched_at: None,
            bag: Vec::new(),
            rng,
        }
    }

    /// Fond courant : celui en cours s'il est encore dans la liste, sinon un nouveau
    /// (le premier, ou un tirage).
    pub fn current(&mut self, settings: &PlaylistSettings, now: Instant) -> Option<Uuid> {
        let items = &settings.items;
        if items.is_empty() {
            self.current = None;
            return None;
        }
        match self.current {
            Some(id) if items.contains(&id) => Some(id),
            _ => self.advance(settings, now),
        }
    }

    /// Le fond courant a-t-il fait son temps ?
    pub fn due(&self, settings: &PlaylistSettings, now: Instant) -> bool {
        self.switched_at
            .is_some_and(|t| now.saturating_duration_since(t) >= interval(settings))
    }

    /// Temps restant avant le prochain fond.
    pub fn remaining(&self, settings: &PlaylistSettings, now: Instant) -> Option<Duration> {
        let t = self.switched_at?;
        Some(interval(settings).saturating_sub(now.saturating_duration_since(t)))
    }

    /// Repart de zéro pour le fond courant (la pause ne doit pas « consommer » son
    /// temps : à la reprise, on décale le départ de la durée de la pause).
    pub fn postpone(&mut self, by: Duration) {
        if let Some(t) = &mut self.switched_at {
            *t += by;
        }
    }

    /// Passe au fond suivant et le rend.
    pub fn advance(&mut self, settings: &PlaylistSettings, now: Instant) -> Option<Uuid> {
        let items = &settings.items;
        let next = match settings.order {
            _ if items.is_empty() => None,
            PlaylistOrder::InOrder => {
                let i = self
                    .current
                    .and_then(|c| items.iter().position(|x| *x == c))
                    .map_or(0, |i| (i + 1) % items.len());
                Some(items[i])
            }
            PlaylistOrder::Shuffle => self.draw(items),
        };
        self.current = next;
        self.switched_at = next.map(|_| now);
        next
    }

    fn draw(&mut self, items: &[Uuid]) -> Option<Uuid> {
        // Le sac ne garde que les fonds encore dans la liste.
        self.bag.retain(|id| items.contains(id));
        if self.bag.is_empty() {
            self.bag = items.to_vec();
            // Mélange de Fisher-Yates.
            for i in (1..self.bag.len()).rev() {
                let j = self.rng.below(i + 1);
                self.bag.swap(i, j);
            }
            // Pas deux fois de suite à la jonction de deux sacs.
            if self.bag.len() > 1 && self.bag.last() == self.current.as_ref() {
                let last = self.bag.len() - 1;
                self.bag.swap(0, last);
            }
        }
        let next = self.bag.pop();
        if items.len() > 1 && next == self.current {
            // Cas restant : le fond courant n'était plus dans le sac mais y revient.
            return self.bag.pop().or(next);
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn settings(n: usize, order: PlaylistOrder) -> PlaylistSettings {
        PlaylistSettings {
            items: (0..n).map(|_| Uuid::new_v4()).collect(),
            order,
            interval_minutes: 10,
        }
    }

    #[test]
    fn in_order_loops_through_the_list() {
        let s = settings(3, PlaylistOrder::InOrder);
        let now = Instant::now();
        let mut p = PlaylistState::new(Rng::new(1));
        assert_eq!(p.current(&s, now), Some(s.items[0]));
        assert_eq!(p.advance(&s, now), Some(s.items[1]));
        assert_eq!(p.advance(&s, now), Some(s.items[2]));
        assert_eq!(p.advance(&s, now), Some(s.items[0]));
    }

    #[test]
    fn removed_current_item_moves_on() {
        let mut s = settings(3, PlaylistOrder::InOrder);
        let now = Instant::now();
        let mut p = PlaylistState::new(Rng::new(1));
        p.current(&s, now);
        p.advance(&s, now);
        s.items.remove(1);
        assert_eq!(p.current(&s, now), Some(s.items[0]));
        s.items.clear();
        assert_eq!(p.current(&s, now), None);
    }

    #[test]
    fn shuffle_never_repeats_immediately_and_visits_everything() {
        for seed in 1..50 {
            let s = settings(5, PlaylistOrder::Shuffle);
            let now = Instant::now();
            let mut p = PlaylistState::new(Rng::new(seed));
            let mut previous = p.current(&s, now).unwrap();
            let mut seen = HashSet::from([previous]);
            for step in 0..200 {
                let next = p.advance(&s, now).unwrap();
                assert_ne!(next, previous, "répétition immédiate (graine {seed})");
                seen.insert(next);
                if step == 3 {
                    assert_eq!(seen.len(), 5, "un sac entier avant de recommencer");
                }
                previous = next;
            }
        }
    }

    #[test]
    fn shuffle_with_two_items_alternates() {
        let s = settings(2, PlaylistOrder::Shuffle);
        let now = Instant::now();
        let mut p = PlaylistState::new(Rng::new(7));
        let mut previous = p.current(&s, now).unwrap();
        for _ in 0..20 {
            let next = p.advance(&s, now).unwrap();
            assert_ne!(next, previous);
            previous = next;
        }
        let one = settings(1, PlaylistOrder::Shuffle);
        let mut p = PlaylistState::new(Rng::new(7));
        assert_eq!(p.current(&one, now), Some(one.items[0]));
        assert_eq!(p.advance(&one, now), Some(one.items[0]));
    }

    #[test]
    fn interval_decides_when_to_switch() {
        let mut s = settings(2, PlaylistOrder::InOrder);
        let start = Instant::now();
        let mut p = PlaylistState::new(Rng::new(1));
        p.current(&s, start);
        assert!(!p.due(&s, start + Duration::from_secs(9 * 60)));
        assert!(p.due(&s, start + Duration::from_secs(10 * 60)));
        assert_eq!(
            p.remaining(&s, start + Duration::from_secs(4 * 60)),
            Some(Duration::from_secs(6 * 60))
        );
        // Une pause de 5 minutes repousse le changement d'autant.
        p.postpone(Duration::from_secs(5 * 60));
        assert!(!p.due(&s, start + Duration::from_secs(10 * 60)));
        assert!(p.due(&s, start + Duration::from_secs(15 * 60)));
        // Bornes : 5 min au moins, 24 h au plus.
        s.interval_minutes = 0;
        assert_eq!(interval(&s), Duration::from_secs(5 * 60));
        s.interval_minutes = 100_000;
        assert_eq!(interval(&s), Duration::from_secs(24 * 3600));
    }
}
