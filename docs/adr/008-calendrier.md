# ADR 008 · Calendrier : EventKit, puis ICS

Statut : Acceptée (24/11/2026)

## Décision

Le widget Calendrier lit le calendrier de macOS par EventKit : les comptes Google et Outlook déjà ajoutés au Calendrier remontent sans OAuth. Une URL ICS sert de secours. L'OAuth Google/Microsoft n'arrivera que sur demande.

## Conséquences

- Autorisation « Calendriers » de macOS à demander avec un écran d'explication.


## À revoir pour Windows (ADR 001)

EventKit est propre à macOS. Sous Windows : lecture d'une URL ICS (Google et Outlook en fournissent une) en premier, puis OAuth Google et Microsoft Graph si besoin, derrière `CalendarProvider`.
