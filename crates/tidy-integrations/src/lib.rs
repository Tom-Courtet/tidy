//! Services en ligne des widgets de Tidy, chacun derrière son trait :
//! - [`weather::WeatherProvider`] : météo, avec Open-Meteo (sans clé) ;
//! - [`calendar::CalendarProvider`] : agendas, par URL ICS (Google et Outlook en
//!   donnent une), avec l'analyse ICS de [`ics`] ;
//! - [`ai::AiProvider`] : chat IA, avec l'API Claude en flux (SSE) ;
//! - [`secret`] : clé de l'API dans le gestionnaire d'identifiants du système.
//!
//! Pas de runtime async : le réseau passe par `ureq` (TLS du système), toujours sur un
//! fil à part ([`task`]), jamais dans le fil de l'interface. Aucune dépendance d'egui :
//! les widgets réveillent leur fenêtre par un [`task::Waker`].
//!
//! Rien ne part sur le réseau tant qu'un widget ne le demande pas (ADR 004).

pub mod ai;
pub mod calendar;
pub mod ics;
pub mod net;
pub mod secret;
pub mod task;
pub mod weather;

pub use net::NetError;
