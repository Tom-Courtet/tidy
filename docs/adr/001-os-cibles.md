# ADR 001 · OS cibles : Windows en priorité, macOS en parallèle

Statut : Révisée le 24/11/2026 (remplace « macOS d'abord »)

## Décision

Le propriétaire utilise Tidy **d'abord sous Windows** (10 et 11, 64 bits), comme l'original. Tidy vise donc la parité **sous Windows en priorité**, et garde macOS au même niveau tant que cela ne retarde pas Windows.

- Tout ce qui touche à l'OS passe par le trait `DesktopPlatform` de `tidy-platform`, avec une implémentation `windows/` (crates `windows-sys` / `windows`) et une implémentation `macos/`.
- Le développement se fait sur un Mac : la cible Windows est vérifiée par `cargo xwin check`, et **une CI GitHub (`windows-latest`)** compile, teste et publie `tidy.exe` en artefact à chaque push. Le propriétaire teste cet exe sur son PC.
- Chaque vague se termine par une liste de tests manuels **Windows** (en plus de macOS).

## Conséquences

- La vague 0 bis porte le prototype du bureau sur Windows avant la vague 1.
- Les décisions propres à macOS sont à revoir pour Windows : boîte privée (ADR 006 : pas d'image APFS), calendrier (ADR 008 : pas d'EventKit), recherche (ADR 007 : Spotlight → index maison seul, Windows Search en option).
- Les points délicats de l'original sous Windows (pilote de système de fichiers, injection dans l'Explorateur, Windhawk) ne sont **pas** repris : Tidy reste une application utilisateur sans pilote ni injection (voir ADR 011).
