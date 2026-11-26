<p align="center"><img src="assets/icon.png" width="72" alt="Logo de Tidy"></p>

# Tidy

Organiseur de bureau léger et personnalisable pour **Windows** (10 et 11) et macOS,
100 % Rust (egui) : boîtes sur le bureau, rangement automatique, boîte privée chiffrée,
dispositions, widgets, recherche rapide, fonds d'écran. Réimplémentation libre des
fonctions d'iTop Easy Desktop, sans rien reprendre de son identité. De la même famille
que Snappy, Tappy et Soundy.

> **État : vagues 1, 2 et 3 faites.** Vérifié sous macOS ; sous Windows (cible
> prioritaire), compilé et testé en CI, essais réels en cours sur PC
> ([docs/manual-tests.md](docs/manual-tests.md)). Plan : [PLAN.md](PLAN.md), parité :
> [docs/FEATURES.md](docs/FEATURES.md), décisions : [docs/adr/](docs/adr/).

## Installation sous Windows

1. Télécharger `tidy.exe` : page **Releases** du dépôt (versions `v*`), ou onglet
   **Actions** > dernier build réussi de `main` > artefact `tidy-windows` (un `.zip` qui
   contient l'exe).
2. Le lancer. L'exe n'est pas signé : SmartScreen affiche « Windows a protégé votre
   ordinateur » ; cliquer sur *Informations complémentaires* puis *Exécuter quand
   même*.
3. **Installer** : bouton « Installer Tidy » de l'assistant de premier lancement, de
   l'Accueil ou de la page Options, ou en ligne de commande :

   ```powershell
   .\tidy.exe --install
   ```

   Sans droits d'administrateur, Tidy se copie dans
   `%LOCALAPPDATA%\Programs\Tidy\tidy.exe`, crée son raccourci dans le menu Démarrer et
   son entrée dans **Applications et fonctionnalités**, repointe « Démarrer avec
   Windows » et le menu du bureau vers cette copie, puis redémarre. L'exe téléchargé
   peut ensuite être supprimé.

**Mettre à jour** : télécharger le nouvel exe et le lancer (ou `.\tidy.exe --install`) :
la page Options propose « Mettre à jour », qui arrête Tidy, remplace la copie installée
et la relance. Boîtes, boîte privée, dispositions et réglages sont gardés. Une copie
installée plus récente n'est remplacée qu'avec `--install --force`.

## macOS

```sh
sh tools/bundle_macos.sh   # dist/Tidy.app (barre des menus, ni Dock ni ⌘Tab)
sh tools/dmg_macos.sh      # dist/Tidy-<version>.dmg
```

Glisser `Tidy.app` dans Applications. Signature et notarisation :
[docs/distribution.md](docs/distribution.md). macOS demande l'accès au Bureau (boîtes)
et à Documents et Téléchargements (recherche) ; ni Accessibilité ni enregistrement de
l'écran.

## Fonctions

- **Agent** discret dans la zone de notification (Windows) ou la barre des menus
  (macOS). Son menu : ouvrir Tidy, rechercher, activer les boîtes, masquer le bureau,
  dispositions, verrouiller la boîte privée, widgets, fond d'écran suivant, quitter.
- **Boîtes** sur le bureau, une fenêtre chacune, sous toutes les fenêtres et immobiles
  pendant Win+D : vraies icônes et miniatures, grille libre ou liste, tri,
  regroupement par date, onglets, enroulement, aimantation, apparence (couleurs,
  opacité, verre dépoli), accès rapide par raccourci. Les icônes rangées disparaissent
  du bureau de l'Explorateur sans que les fichiers bougent.
- **Rangement automatique** par règles (catégories, extensions, priorités, fourre-tout),
  nouveaux fichiers rangés à leur arrivée, « Tout annuler ». Démarrage rapide qui crée
  les boîtes par défaut.
- **Portails** : une boîte qui montre un dossier en direct.
- **Boîte privée** chiffrée (Argon2id + XChaCha20-Poly1305), clé de récupération,
  verrouillage automatique ([ADR 013](docs/adr/013-coffre.md)).
- **Dispositions** : instantanés automatiques et manuels, dispositions nommées avec
  raccourci, une disposition par configuration d'écrans.
- **Recherche rapide** (Alt+Espace) : applis, fichiers, onglets, éléments récents.
- **Widgets** : horloge, notes, pomodoro, outils rapides, Search Mini, météo
  (Open-Meteo), calendrier (ICS), chat IA (Claude).
- **Fonds d'écran** : images, vidéos, couleur unie, playlist, un fond par écran, pause
  intelligente (plein écran, batterie, session verrouillée), image du jour de Bing
  (désactivée par défaut).
- **Jamais de fichier perdu** : ranger ne déplace rien, supprimer passe par la
  corbeille, supprimer une boîte rend ses éléments au bureau.

## En ligne de commande

```sh
tidy                     # lance l'agent (et ouvre la fenêtre principale)
tidy --background        # lance l'agent seul (ouverture de session)
tidy --manager [page]    # fenêtre principale (options, widgets, wallpapers, diagnostic…)
tidy --search [texte]    # lanceur de recherche rapide
tidy --create-box        # crée une boîte sous la souris (menu du bureau sous Windows)
tidy --toggle-desktop    # masque ou montre le bureau
tidy --organize          # range le bureau selon les règles (--undo-organize : annule)
tidy --restore-desktop   # rend les icônes du bureau si Tidy les a masquées
tidy --install [--force] # installe Tidy pour cet utilisateur (Windows) ou le met à jour
tidy --uninstall         # retire Tidy du système ; les données restent
tidy --quit              # arrête Tidy
tidy --version
```

## Données et journaux

| | Windows | macOS |
|---|---|---|
| Programme installé | `%LOCALAPPDATA%\Programs\Tidy\` | `/Applications/Tidy.app` |
| Données | `%APPDATA%\Tidy\data\` | `~/Library/Application Support/Tidy/` |

Le dossier de données contient `config.json` (réglages), `desktop.json` (boîtes,
règles, positions par écran), `snapshots/` (instantanés et dispositions), `vault/` (boîte
privée chiffrée), `notes/`, `search/` (index), `wallpapers.json` et `logs/` (un journal par jour et par
processus, 7 jours gardés ; `TIDY_LOG=debug` pour plus de détails). Options > À propos
et la page Diagnostic montrent ces dossiers et les ouvrent.

Essai sans toucher à ses données : `TIDY_DATA_DIR=$(mktemp -d) tidy` (et
`TIDY_DESKTOP_DIR=<faux bureau>` pour ne pas toucher au vrai bureau).

## Désinstallation

Windows : **Paramètres > Applications > Tidy > Désinstaller**, ou Options >
Désinstaller, ou `tidy --uninstall`. Tidy arrête tous ses processus (la boîte privée
est verrouillée, jamais supprimée), remet les icônes rangées à leur place, rend les
icônes de l'Explorateur et le fond d'écran d'origine, retire ses entrées du registre
(menu du bureau, démarrage, notifications, Applications et fonctionnalités), le
raccourci du menu Démarrer et la copie installée. **Le dossier de données reste** (il
contient la boîte privée et les dispositions) : son chemin est affiché à la fin ; le
supprimer à la main si on n'en veut plus.

macOS : Options > Désinstaller (ou `tidy --uninstall`), puis Tidy.app à la corbeille.

## Processus et mesures

Un seul exécutable, un processus par rôle : la mémoire d'une fenêtre est rendue au
système dès qu'elle se ferme ([ADR 010](docs/adr/010-processus.md),
[ADR 011](docs/adr/011-fenetre-par-boite.md)).

Mesures en release (`Tidy.app`), MacBook Air M4, macOS 26.4, écran Retina intégré :
mémoire = `footprint <pid>` (valeur du Moniteur d'activité), processeur = temps
consommé sur 20 à 30 s sans rien toucher. Données jetables (`TIDY_DATA_DIR`), faux
bureau de 13 éléments (`TIDY_DESKTOP_DIR`), recherche limitée aux applis et à ce
faux bureau.

| Processus | Situation | Mémoire | Processeur au repos |
|---|---|---|---|
| `tidy` (agent : icône, raccourcis, surveillance) | toujours | 16 Mo | 0,03 à 0,07 % |
| `tidy --desktop` (boîtes et widgets) | 0 boîte | 43 Mo | 0 % |
| | 3 boîtes | 62 Mo | 0 % |
| | 10 boîtes | 95 Mo | 0 % |
| | 10 boîtes et 4 widgets (horloge, notes, pomodoro, outils rapides) | 118 Mo | 0 % |
| `tidy --search` (lanceur, gardé en mémoire fenêtre cachée) | ouvert | 133 Mo | 0,5 % |
| `tidy --wallpaper` (fond vidéo 1080p, H.264, muet) | en lecture | 18 Mo | 2 % |
| `tidy --manager` (fenêtre principale) | ouverte | 113 Mo, rendus à la fermeture | 0 % |

Une boîte coûte 6 à 7 Mo (une fenêtre et sa surface de dessin). Le fond vidéo est
décodé par le matériel (AVFoundation sous macOS, Media Foundation sous Windows) : sa
mémoire vidéo n'entre pas dans le `footprint`. À refaire sous Windows (Gestionnaire
des tâches > Détails > « Plage de travail privée »).

Binaire : 8,1 Mo ; `Tidy.app` : 7,8 Mo ; `.dmg` : 4,1 Mo.

## Construire

```sh
cargo build --release          # target/release/tidy
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo xwin clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

CI (`.github/workflows/build.yml`) : tests sous Windows et macOS à chaque push, `tidy.exe`
en artefact, joint à la Release GitHub sur chaque tag `v*`.

## Licences

Tidy : licence MIT. Polices Outfit et Unbounded (SIL OFL 1.1), icônes Lucide (ISC),
symboles de DejaVu Sans (licence Bitstream Vera / DejaVu) : textes dans `assets/fonts/`.
Bibliothèques Rust : MIT ou Apache-2.0 pour l'essentiel (`nucleo-matcher` : MPL-2.0 ;
`notify` : CC0-1.0) ; liste dans Options > À propos > Licences tierces.
