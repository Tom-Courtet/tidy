# ADR 001 · OS cibles : macOS d'abord, Windows prévu

Statut : Acceptée (24/11/2026)

## Décision

Tidy vise macOS en premier. Windows n'est pas implémenté, mais tout ce qui touche à l'OS passe par le trait `DesktopPlatform` de `tidy-platform`, et la cible `x86_64-pc-windows-msvc` doit compiler (`cargo xwin check`), comme pour Snappy.

## Conséquences

- Le portage Windows (WorkerW/Progman, ListView du bureau, `SHGetFileInfo`) arrive après la parité macOS.
- L'implémentation Windows actuelle (`other.rs`) répond par des erreurs claires.
