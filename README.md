<p align="center"><img src="assets/icon.png" width="72" alt="Logo de Tidy"></p>

# Tidy

Organiseur de bureau léger et personnalisable pour **Windows** (10 et 11) et macOS, 100 % Rust (egui) : boîtes
sur le bureau, rangement automatique, widgets, recherche rapide, fonds d'écran. De la
même famille que Snappy, Tappy et Soundy.

> **État : vague 0 bis (prototype Windows et macOS).** Agent, fenêtre principale et boîtes
> posées sur le bureau (une fenêtre par boîte : créer, déplacer, redimensionner, supprimer,
> masquer). Vérifié sous macOS ; sous Windows, compilé et testé en CI, essais réels en
> cours. Prochaine étape : les vraies boîtes (vague 1). Plan : [PLAN.md](PLAN.md), parité :
> [docs/FEATURES.md](docs/FEATURES.md).
>
> **Windows** : chaque push sur `main` compile `tidy.exe` (onglet *Actions* du dépôt,
> artefact `tidy-windows`).

## Ce qui marche déjà

- **Agent dans la barre des menus** (ni Dock ni ⌘Tab) : ouvrir Tidy, activer ou
  désactiver les boîtes, masquer le bureau, quitter.
- **Boîte de démonstration** posée sur le bureau, sous toutes les fenêtres, sur tous les
  Spaces. Elle se déplace d'un écran à l'autre ; sa position est mémorisée **par
  configuration d'écrans**.
- **Deux modes de bureau** :
  - *superposé* (défaut) : la boîte flotte au-dessus des icônes du Finder, les clics
    passent partout ailleurs ;
  - *géré* (avec ton accord) : Tidy masque les icônes du Finder et prend tout le bureau ;
    double-clic sur une zone vide pour masquer ou montrer.
- **Icônes du Finder toujours rendues** : en quittant, en changeant de mode, si le bureau
  plante trois fois en une minute, si l'agent disparaît, ou avec `tidy --restore-finder`.
- **Raccourcis** : ⌥Espace (recherche, à venir), ⌃⌥H (masquer le bureau), ⌥` (accès
  rapide aux boîtes, à venir).
- **Fenêtre principale** : Accueil, Options (langue FR/EN, ouverture de session, mode du
  bureau), Diagnostic (dossiers, journaux, Finder, écrans).

## Processus et mémoire

Un seul exécutable, plusieurs processus :

| Processus | Rôle | Mémoire (release, MacBook Air, écran Retina 1470×956 pt) |
|---|---|---|
| `tidy` | l'**agent** : barre des menus, raccourcis, surveillance | 14 Mo, 0 % de processeur au repos |
| `tidy --desktop` | une fenêtre par boîte, relancées par l'agent si besoin | 52 Mo avec une boîte, 58 Mo avec deux (au lieu de 161 Mo avec une surface plein écran) |
| `tidy --manager` | la fenêtre principale | ≈ 110 Mo, rendus à la fermeture |

Binaire : 4,5 Mo ; `Tidy.app` : 4,4 Mo. D'où le choix d'**une fenêtre par boîte**, comme
l'original ([ADR 011](docs/adr/011-fenetre-par-boite.md)).

## Construire

```sh
cargo build --release          # target/release/tidy
sh tools/bundle_macos.sh       # dist/Tidy.app
```

Essai sans toucher à tes données : `TIDY_DATA_DIR=$(mktemp -d) target/release/tidy`.

## En ligne de commande

```sh
tidy                     # lance l'agent (et ouvre la fenêtre principale)
tidy --background        # lance l'agent seul (ouverture de session)
tidy --manager [page]    # fenêtre principale (options, diagnostic…)
tidy --create-box        # crée une boîte sous la souris (menu du bureau sous Windows)
tidy --toggle-desktop    # masque ou montre le bureau
tidy --restore-desktop   # rend les icônes du bureau si Tidy les a masquées
tidy --uninstall         # retire Tidy du système (menu du bureau, démarrage, icônes)
tidy --quit              # arrête Tidy
```

## Fichiers

Dans `~/Library/Application Support/Tidy/` (ou `TIDY_DATA_DIR`) :
- `config.json` : réglages (langue, mode du bureau, raccourcis…) ;
- `desktop.json` : boîtes, règles, positions par écran ;
- `finder-hidden.json` : présent seulement quand Tidy a masqué les icônes du Finder ;
- `logs/` : journaux, un fichier par jour et par processus, 7 jours gardés.

## Crédits

Polices : Outfit et Unbounded (OFL), icônes Lucide (ISC/MIT), symboles de DejaVu Sans.
Voir `assets/fonts/`. Licence : MIT.
