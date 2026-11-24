# ADR 004 · Chat IA derrière un trait, Claude d'abord

Statut : Acceptée (24/11/2026)

## Décision

Le chat IA passe par un trait `AiProvider` (crate `tidy-integrations`). La première implémentation est l'API Claude, avec réponses en flux et clé dans le trousseau. Réseau via `ureq` (comme Soundy), sans runtime async.

## Conséquences

- Ajouter un fournisseur ne touche qu'un module.
- Aucun appel réseau tant que le widget n'est pas activé.
