<p align="center"><img src="assets/icon.png" width="72" alt="Logo de Tidy"></p>

# Tidy

Organiseur de bureau léger et personnalisable pour **Windows** (10 et 11) et macOS, 100 % Rust (egui) : boîtes
sur le bureau, rangement automatique, widgets, recherche rapide, fonds d'écran. De la
même famille que Snappy, Tappy et Soundy.

> **État : vague 0 (socle et prototype macOS).** L'agent, la fenêtre principale et une boîte
> de démonstration sur le bureau fonctionnent sous macOS. Prochaine étape : le prototype
> Windows (vague 0 bis), puis les vraies boîtes. Plan : [PLAN.md](PLAN.md), parité :
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
| `tidy --desktop` | les surfaces du bureau, relancées par l'agent si besoin | 161 Mo avec une surface plein écran (Retina) ; 51 Mo avec une fenêtre par boîte (mesure de comparaison) |
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
tidy --restore-finder    # rend les icônes du Finder si Tidy les a masquées
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
