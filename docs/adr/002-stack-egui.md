# ADR 002 · Interface en egui/eframe, pas en Tauri

Statut : Acceptée (24/11/2026)

## Décision

Le prompt recommandait Tauri 2. Tidy suit plutôt ses projets frères (Snappy, Tappy, Soundy) : egui/eframe 0.37 avec glow, winit pour l'agent sans fenêtre, tray-icon, global-hotkey, objc2. Le thème, les polices, les icônes et les widgets sont repris tels quels dans `tidy-ui`.

## Conséquences

- DA identique aux frères, tout en Rust, pas de webview (50 à 100 Mo par fenêtre).
- Pas d'éditeur de texte riche tout fait : iNotes en Markdown (voir PLAN § 3).
- Chaque fenêtre egui coûte environ cinq tampons de sa taille (mesuré : voir ADR 003).
