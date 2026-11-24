# ADR 009 · Livraison par vagues, sous-agents en parallèle

Statut : Acceptée (24/11/2026)

## Décision

Le travail est découpé en vagues (PLAN § 5). La vague 0 (socle, contrats) est faite par un seul agent. Les suivantes sont réparties en lots parallèles, chacun dans son worktree et sur ses propres crates, puis fusionnées et vérifiées. Arrêt pour validation à la fin de chaque vague.

## Conséquences

- Les contrats du socle (traits, IPC, modèle) ne changent pas sans accord.
