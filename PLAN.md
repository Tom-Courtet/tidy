# Tidy : plan de projet (à valider)

> Rédigé le 24/11/2026 à partir de `prompt-easy-desktop-rust.md` (le « prompt »), de `iTop_Easy_Desktop.md` (le « bilan ») et de l'étude de Snappy, Tappy et Soundy.
> Rien n'est codé tant que ce plan n'est pas validé. Les décisions à trancher sont en § 2 ; mes choix par défaut, contestables, en § 3.

## 1. Résumé

1. **Nom : Tidy** (le nom du dossier) plutôt que `deskbox`. Binaire `tidy`, données dans `~/Library/Application Support/Tidy`, essais avec `TIDY_DATA_DIR`.
2. **Même famille que Snappy, Tappy et Soundy** : Rust edition 2024, **egui/eframe 0.37** (glow), agent sans fenêtre (winit, tray-icon, global-hotkey), objc2 pour macOS. **Pas de Tauri** : c'est le principal écart avec le prompt (voir D2).
3. **Même DA** : fond noir, surfaces `#101012` / `#1b1b1f`, violet `#7F3AED` en secondaire, lime `#C6FF34` en principal avec texte noir, rayons de 10 et 16 px. Polices Unbounded (titres) et Outfit (texte), icônes Lucide réduites aux glyphes utilisés. `theme.rs`, les widgets partagés (`primary`, `choice`, `hint`, `confirm_delete`, saisie de raccourci…) et les outils (`subset_icons.py`, `gen_icon.py`, `bundle_macos.sh`) sont repris de Snappy et Soundy dans une crate `tidy-ui`.
4. **Léger, comme les frères** : un exécutable, plusieurs processus. L'agent tourne en permanence (≈ 10–15 Mo visés, comme Tappy et Snappy). Les fenêtres lourdes (réglages, fond animé) vivent dans des processus lancés à la demande et rendent leur mémoire en se fermant. Les mesures `phys_footprint` figureront dans le README, comme pour les frères.
5. **Bureau en mode « géré »** (recommandé, D3) : Tidy masque les icônes du Finder et dessine tout `~/Desktop` sur **une surface transparente par écran**, au niveau des icônes du bureau. Snappy fait déjà une fenêtre plein écran par écran pour sa sélection : la technique est éprouvée. Les fichiers ne bougent jamais : l'appartenance à une boîte n'est qu'une métadonnée.
6. **Sécurité du bureau** : l'agent surveille le processus du bureau. Si celui-ci plante trois fois de suite, ou si l'agent est quitté, les icônes du Finder reviennent. `tidy --restore-finder` est le secours manuel.
7. **Capture d'écran** : Tidy ne la refait pas, puisque Snappy existe. La tuile « Capture » des outils rapides lance Snappy (D5).
8. **Méthode** : je pose seul un socle commun (vague 0), avec les contrats (traits, IPC, modèle). Ensuite viennent des vagues de sous-agents en parallèle, chacun dans sa copie du dépôt (worktree) et sur ses propres crates. Je fusionne et je vérifie. Tu valides à la fin de chaque vague. Commits en français, au format Conventional Commits, sans mention de Claude.

## 1 bis. Révision du 24/11/2026 : Windows en priorité

Après la vague 0, deux changements (ADR 001 et 011) :

1. **Tidy vise d'abord Windows** (usage réel du propriétaire), macOS suit. Développement sur Mac, cible Windows vérifiée par `cargo xwin check` et par une **CI GitHub** qui compile, teste et publie `tidy.exe` à chaque push. Chaque vague se termine par des tests manuels **sur le PC Windows**.
2. **Une fenêtre par boîte et par widget**, comme l'original (manuel officiel : boîtes = panneaux séparés, icônes hors boîte laissées à l'Explorateur). La surface plein écran coûtait 161 Mo ; une fenêtre de boîte, 51 Mo pour tout le processus. Une surcouche plein écran temporaire sert seulement pour dessiner une boîte ou glisser entre boîtes.

### Vague 0 bis · Prototype Windows (moi, seul)
- CI GitHub Actions : fmt, clippy, tests sur Windows et macOS ; artefact `tidy.exe`.
- `tidy-platform/windows` : écrans (identifiant stable, DPI par écran), fenêtre de boîte au niveau du bureau qui **résiste à Win+D** et reste sous les applis, démarrage avec la session (registre, repris de Snappy), ouverture et « afficher dans l'Explorateur », langue du système, icône de la zone de notification.
- Icônes de l'Explorateur : masquer et montrer toutes les icônes (double-clic pour masquer) sans injection ; étude de la façon de masquer seulement les icônes rangées dans une boîte (décision à prendre ensemble).
- Menu contextuel du bureau « Tidy > Créer une boîte » par une entrée de registre (sous Windows 11 : « Afficher plus d'options »).
- Prototype du bureau passé à une fenêtre par boîte (Windows et macOS).
- 🛑 **Validation sur ton PC** : Win+D, multi-écrans, DPI mélangés, plantage simulé, mémoire (liste : `docs/manual-tests.md`).

**État (24/11/2026)** : fait, vérifié sur macOS et compilé/testé en CI pour Windows ; les essais réels sous Windows restent à faire.

**À décider ensemble : masquer seulement les icônes rangées dans une boîte (Windows).** Sans injecter de code dans l'Explorateur, trois pistes :
1. *Déplacer les icônes rangées hors de l'écran* par les messages de la liste du bureau (`LVM_SETITEMPOSITION`, mémoire partagée avec l'Explorateur), puis les remettre en quittant. Aucun fichier touché ; demande « Organisation automatique » désactivée ; un plantage peut laisser des icônes hors de l'écran (l'agent les replace au démarrage suivant).
2. *Attribut « caché » sur les fichiers rangés* : l'Explorateur ne les affiche plus. Modifie un attribut des fichiers (pas leur contenu), visible dans d'autres outils ; à rétablir en quittant.
3. *Tout masquer et tout redessiner* (comme le mode géré de macOS) : liste du bureau cachée, Tidy dessine aussi les icônes hors boîte dans une fenêtre par groupe. Le plus propre visuellement, le plus long à faire.
Recommandation : **1**, avec repli sur 3 si l'Explorateur la contrarie. **Décidé : piste 1** (ADR 012), primitive faite et testable avec `tidy --hide-icon`.

Les vagues 1 à 3 restent valables ; leurs lots implémentent Windows **et** macOS derrière `DesktopPlatform`, Windows d'abord. Les décisions macOS à revoir pour Windows sont notées dans les ADR 006, 007 et 008.

## 2. Décisions à trancher

| # | Question | Options | Ma recommandation |
|---|---|---|---|
| **D1** | OS cibles | a) macOS seul ; b) macOS d'abord, Windows prévu dès la conception ; c) les deux en même temps | **b)** Un trait `DesktopPlatform` dès le socle, une implémentation Windows vide qui compile (`cargo xwin check`, comme Snappy). Le vrai portage viendra après la parité. |
| **D2** | Stack d'interface | a) Tauri 2 + Svelte (prompt) ; b) Tauri + Leptos ; c) **egui/eframe** (projets frères) ; d) Slint | **c)** Elle reprend la DA telle quelle (thème, widgets, polices, icônes), garde la mémoire basse (une webview coûte 50 à 100 Mo par fenêtre) et tout reste en Rust. Contrepartie : pas d'éditeur de texte riche tout fait (voir § 3, iNotes). |
| **D3** | Mode bureau | a) **géré** : icônes du Finder masquées, Tidy dessine tout ; b) superposé : les boîtes flottent au-dessus des icônes du Finder, d'où des doublons | **a)**, avec ton accord explicite au premier lancement. Le mode b) reste disponible en secours : même surface, mais elle laisse passer les clics hors des boîtes (`ignoresMouseEvents` basculé selon la position de la souris). |
| **D4** | Chat IA | a) API Claude directement ; b) trait `AiProvider`, fournisseur choisi plus tard | **a+b)** Trait `AiProvider`, avec Claude comme première implémentation (clé dans le trousseau, réponses en flux). Ajouter un autre fournisseur ne touchera qu'un module. |
| **D5** | Capture d'écran (absente du prompt, présente dans l'original) | a) déléguer à Snappy ; b) la refaire dans une crate `capture` | **a)** La tuile « Capture » lance `snappy --region` (ou propose de l'installer s'il manque). Aucun code en double. |
| **D6** | Boîte privée | a) chiffrement maison fichier par fichier (Argon2id + XChaCha20-Poly1305), déchiffrés dans un dossier temporaire pendant le déverrouillage ; b) **image disque APFS chiffrée** (`hdiutil`, sparsebundle), montée sans apparaître dans le Finder | **b), avec une enveloppe maison.** La clé aléatoire de l'image est chiffrée deux fois (Argon2id + XChaCha20-Poly1305) : une fois par ton mot de passe, une fois par une clé de récupération à noter. Les applis ouvrent les fichiers normalement, rien n'est jamais en clair sur le disque et les gros fichiers ne posent pas de problème. Le code cryptographique se limite à 32 octets, testé aller-retour. Derrière un trait `VaultBackend`, pour Windows plus tard. |
| **D7** | Recherche rapide | a) index maison seul (prompt : < 50 ms sur 500 000 fichiers) ; b) Spotlight seul ; c) **les deux** | **c)** Index maison en mémoire (`nucleo`, mise à jour par FSEvents) sur les applis, le bureau, Documents et Téléchargements (dossiers réglables), donc instantané et léger. Puis « Plus de résultats » via Spotlight (`NSMetadataQuery`) pour le reste du disque, sans index de plus ni accès complet au disque. L'objectif de 50 ms sur 500 000 fichiers est tenu par le banc d'essai de la crate. |
| **D8** | Calendrier | a) OAuth Google + Microsoft Graph ; b) **EventKit** (le calendrier de macOS) ; c) URL ICS | **b) puis c)**. Les comptes Google et Outlook déjà ajoutés au Calendrier de macOS remontent par EventKit, sans OAuth ni serveur. L'URL ICS sert de secours. L'OAuth n'arrivera que si tu en as besoin. |
| **D9** | Points d'arrêt | a) un par jalon du prompt (7) ; b) **un par vague** (4) | **b)** Voir § 5. Chaque vague se termine par une démo et une liste de tests manuels. |

## 3. Choix par défaut (dis-moi si l'un ne te va pas)

- **Version minimale** : macOS 14 (comme Snappy).
- **Données** : JSON versionné (`schema_version` et migrations), écriture atomique (fichier temporaire puis renommage) et fichier illisible mis de côté en `.bad`, comme `Store` dans Tappy.
- **Journaux** : `tracing` + `tracing-appender` (rotation quotidienne, 7 fichiers). Un panneau « Diagnostic » dans les réglages ouvre le dossier et copie un rapport.
- **Langues** : FR et EN, avec un petit module maison (fichiers TOML embarqués, macro `t!`, repli sur le français). Un test vérifie que chaque clé existe dans chaque langue. Ajouter une langue revient à ajouter un fichier.
- **Réseau** : `ureq` comme dans Soundy (TLS du système, sans tokio). Seuls la météo, l'IA et l'éventuel « Bing du jour » s'en servent, et seulement s'ils sont activés.
- **Démarrage avec la session** : `SMAppService`, repris de Snappy. **Instance unique** : ping sur le socket, comme les frères.
- **Raccourcis macOS par défaut** (tous réassignables, avec détection des conflits) : recherche **⌥Espace**, masquer/afficher le bureau **⌃⌥H**, accès rapide aux boîtes **⌥`** (maintenir ou bascule), aucun pour la fenêtre principale. Ils évitent ⌘Espace (Spotlight), ⇧⌘3/4/5 (captures) et les raccourcis de Snappy. Le double appui sur une touche (« Ctrl Ctrl » dans l'original) est en option, désactivé : il demande l'autorisation Surveillance de l'entrée.
- **Classement** : par UTI (Uniform Type Identifier) de macOS, avec les listes d'extensions de l'annexe A du bilan en repli. Les types sont ceux de l'original : Applications, Dossiers, Documents, Images, Vidéos, Musique, Archives, Exécutables, Liens web, Jeux, Autres. On y ajoute `.app`, `.webloc` et les alias.
- **iNotes** : notes stockées en Markdown. L'édition se fait en texte brut, avec une barre d'outils qui insère la syntaxe (titres, gras, italique, listes, cases à cocher, citation, code). L'affichage est mis en forme, et les cases se cochent au clic. C'est plus léger que l'éditeur riche de l'original. Un vrai WYSIWYG viendra plus tard, en évolution, si besoin.
- **Fonds d'écran** : les fonds fixes passent par l'API native (`NSWorkspace.setDesktopImageURL`, un par écran), sans fenêtre ni mémoire. Les fonds vidéo utilisent un processus `tidy --wallpaper` dédié, lancé seulement quand il en faut un, avec AVPlayer et décodage matériel au niveau du fond d'écran. Pause en plein écran, sur batterie sous un seuil, et quand le bureau est masqué.
- **Météo** : Open-Meteo, sans clé. La ville se cherche par nom (géocodage Open-Meteo), sans géolocalisation par défaut, ce qui évite une autorisation de plus.
- **Hors périmètre** : personnalisation de Windows (curseurs, barre des tâches, menu Démarrer), créateur de fonds à effets, bibliothèque en ligne, PawPal, comptes, télémétrie, publicité, licence. Tout sans limite.

## 4. Architecture

### 4.1 Processus (un seul exécutable `tidy`)

| Processus | Rôle | Durée de vie |
|---|---|---|
| `tidy` (**agent**) | Barre des menus, raccourcis globaux, surveillance de `~/Desktop` (FSEvents), moteur de règles, sauvegarde automatique des dispositions, index de recherche, serveur IPC. Il surveille le processus du bureau et gère la sécurité du Finder. | Permanent |
| `tidy --desktop` | Une surface par écran : boîtes, portails, icônes hors boîte, widgets, lanceur de recherche (fenêtre cachée, pour qu'il s'ouvre instantanément). | Permanent, relancé par l'agent s'il plante |
| `tidy --manager` | Fenêtre principale : Boîtes, Rangement, Actions rapides, Dispositions, Fonds d'écran, Widgets, Recherche, Options, Diagnostic. | À la demande |
| `tidy --wallpaper` | Lecteur des fonds vidéo. | Seulement si un fond vidéo est actif |
| `tidy --restore-finder` | Rend les icônes du Finder. | Secours |

Dialogue : JSON ligne par ligne sur un socket Unix (`tidy.sock`), comme Tappy et Snappy. L'agent est la seule source de vérité. Les autres processus lui envoient des commandes et reçoivent des événements (bus d'événements du prompt § 6).

### 4.2 Dépôt (workspace Cargo)

```
tidy/
├─ Cargo.toml                 # workspace, profils release de Snappy (opt-level s, lto, strip)
├─ crates/
│  ├─ tidy-core/              # modèle (Box, Item, Rule, Layout, Snapshot, Screen…), règles, classement,
│  │                          # tri, aimantation, dispositions par écran, instantanés, journal des
│  │                          # déplacements, événements, migrations. Aucune dépendance UI ni OS.
│  ├─ tidy-ui/                # thème, polices, icônes Lucide, widgets partagés, i18n (repris des frères)
│  ├─ tidy-platform/          # trait DesktopPlatform ; macos/ (objc2) ; windows/ (vide, compile)
│  ├─ tidy-search/            # index, FSEvents, nucleo, Spotlight
│  ├─ tidy-vault/             # enveloppe de clés, VaultBackend (image APFS), verrouillage auto
│  ├─ tidy-wallpaper/         # bibliothèque, playlist, WallpaperSource, règles de pause
│  ├─ tidy-widgets/           # trait Widget + un module par widget
│  └─ tidy-integrations/      # météo, calendrier, IA, chacun derrière son trait
├─ app/                       # binaire `tidy` : agent, desktop, manager, wallpaper, IPC
├─ assets/                    # polices, icônes, logo
├─ tools/                     # gen_icon.py, subset_icons.py, bundle_macos.sh
└─ docs/                      # FEATURES.md (parité + Évolutions EVO-xxx), adr/, manual-tests.md
```

`unsafe` est réservé à `tidy-platform` (et à `tidy-wallpaper/macos` pour AVPlayer), avec un `// SAFETY:` à chaque fois. Lints du workspace : `unsafe_code = "forbid"` partout ailleurs.

**Points d'extension** pour tes évolutions : `RuleMatcher` (nouveaux critères de règle), `Widget`, `WallpaperSource`, `AiProvider`, `CalendarProvider`, `WeatherProvider`, `VaultBackend`, `DesktopPlatform`. Chaque widget ou critère s'enregistre dans un registre : en ajouter un ne touche ni `tidy-core` ni les autres modules.

### 4.3 Sécurité des fichiers (non négociable)

- Tidy ne déplace rien dans `~/Desktop` pour ranger : une boîte est une métadonnée. Les seuls déplacements réels viennent d'actions explicites : glisser vers le Finder, vers un portail ou vers la boîte privée.
- Tout déplacement réel est journalisé avant d'être fait (intention, puis exécution, puis validation) et repris au démarrage après un plantage. Les suppressions passent toujours par la corbeille (`trash`).
- Avant le premier rangement automatique : un instantané complet et un bouton « Tout annuler ».
- Supprimer une boîte, même la privée, ne supprime aucun fichier.

## 5. Vagues de travail

Les lots d'une même vague tournent en parallèle, chacun dans son worktree, sur ses propres crates ou dossiers. Je fusionne, puis je lance `cargo build`, `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` et `cargo xwin check`. Ensuite je mets à jour `docs/FEATURES.md` et je te présente le bilan (fait, pas fait, pas testé). **Arrêt pour validation à la fin de chaque vague.**

### Vague 0 · Socle et prototype du bureau (moi, seul)
- `git init`, workspace, `CLAUDE.md`, `docs/FEATURES.md` (tableau de parité complet, statut « à faire »), ADR 001 à 009 (une par décision D1 à D9), `docs/manual-tests.md`.
- `tidy-ui` : thème, polices, icônes, widgets repris des frères ; i18n FR/EN ; logo Tidy, de la même famille (grille de 48, carrés arrondis lime et violet sur noir : une boîte à barre de titre lime qui contient quatre petites icônes violettes).
- `tidy-core` : types du modèle, `Store` (atomique, migrations), événements, et tous les traits d'extension avec leurs signatures.
- `app` : agent (barre des menus, instance unique, démarrage avec la session, journaux), IPC, aiguillage `--desktop`, `--manager`, etc. Fenêtre principale vide aux couleurs de la DA.
- **Prototype du bureau** : une surface transparente par écran, au niveau des icônes du bureau, sur tous les Spaces, hors du Dock et de ⌘Tab, insensible à Mission Control et à « Afficher le bureau ». Une boîte factice dessus, déplaçable. Masquage et retour des icônes du Finder, avec surveillance du processus. **Mesure de la mémoire** avec 1 et 2 écrans.
- 🛑 **Validation** : tu testes sur ton Mac (Spaces, écran externe, Mission Control, plantage simulé du bureau, retour des icônes). Si la surface plein écran coûte trop de mémoire, on passe à une fenêtre par boîte avant d'aller plus loin.

### Vague 1 · Boîtes (jalons 2 et 3 du prompt), 4 lots en parallèle
| Lot | Contenu | Où |
|---|---|---|
| **A · Moteur** | Classement (UTI et extensions), règles avec priorités, boîte fourre-tout, éléments non affectés, « Appliquer aux non affectés » ou « à tous », tri (nom, type, date, taille, manuel), regroupement par date, aimantation (alignement, écart de 10 px, multiples d'icône, ⇧ pour l'ignorer, évitement), positions par écran (UUID de l'écran et coordonnées relatives, par configuration d'écrans). Tests unitaires et tests sur un faux `~/Desktop`. | `tidy-core` |
| **B · Plateforme macOS** | Fenêtres du bureau (fin du prototype), écrans et changements d'écran, icônes système et miniatures (QuickLook), ouverture des fichiers, menu contextuel natif (ouvrir, afficher dans le Finder, renommer, corbeille, copier), glisser vers et depuis le Finder, FSEvents, autorisations avec écrans d'explication. | `tidy-platform` |
| **C · Bureau** | Dessin des boîtes et des icônes hors boîte, déplacer, redimensionner, renommer, menu ☰, apparence (fond, titre, police, transparence, coins, bordure, effet verre), enroulement, « Smarter Roll » et enroulement au bord de l'écran, onglets, glisser-déposer entre boîtes, création par dessin d'un rectangle, double-clic pour masquer (boîtes, icônes, widgets, avec exclusions), accès rapide par raccourci, raccourci par boîte, bandeau « icônes non affectées », portails de dossier (navigation, fil d'Ariane ; interdit sur un dossier du bureau). | `app/desktop` |
| **D · Fenêtre principale** | Pages Boîtes, Rangement (types et extensions, avec « Réinitialiser »), Actions rapides, Options (langue, démarrage, raccourcis avec détection des conflits), Diagnostic. Assistant de premier lancement (« Démarrage rapide » ou « Personnaliser », avec l'accord pour le mode géré). | `app/manager` |
- 🛑 **Validation** : démo du rangement sur une copie de ton bureau (`TIDY_DATA_DIR` et un dossier factice), puis sur le vrai.

### Vague 2 · Dispositions, boîte privée, recherche, widgets (jalons 4 et 5), 5 lots
| Lot | Contenu | Où |
|---|---|---|
| **E · Dispositions** | Instantanés automatiques à chaque changement (50 au plus ; le premier et les manuels protégés), miniature, restaurer, renommer, supprimer, dispositions nommées avec raccourci, réglage du changement de résolution (redimensionner, mémoriser par résolution, ne rien faire), page Dispositions. | `tidy-core`, `app/manager` |
| **F · Boîte privée** | Enveloppe Argon2id + XChaCha20-Poly1305 (mot de passe de 6 à 20 caractères, clé de récupération), image APFS chiffrée, déverrouiller et verrouiller, verrouillage auto (inactivité, veille), opacité jusqu'à 0 % avec retour au survol, changement de mot de passe, suppression qui rend les fichiers. | `tidy-vault` |
| **G · Recherche** | Index, FSEvents, nucleo, onglets (Tout, Bureau, Applis, Documents, Images, Vidéos, Musique, Dossiers, Archives), récents, actions (ouvrir, afficher dans le Finder, copier le chemin, copier, corbeille), Spotlight, banc d'essai à 500 000 entrées, « Search Mini » épinglable sur le bureau. | `tidy-search`, `app/desktop` |
| **H · Widgets 1** | Trait `Widget`, registre, cadre commun (geler, masquer, exclure du masquage rapide, position par écran). Horloge (numérique ou analogique, plusieurs fuseaux, 3 tailles), iNotes (plusieurs notes, épingler au premier plan, fenêtre « Gérer les notes » avec recherche et regroupement par date), iPomodoro (cycles, enchaînement, son, notification, mode concentration), outils rapides (grille réglable, tuile Snappy). | `tidy-widgets` |
| **I · Widgets 2** | Météo (Open-Meteo, plusieurs villes, prévisions), calendrier (EventKit, puis ICS ; rappels en notifications natives), chat IA (Claude en flux, invites prédéfinies : traduire, résumer, rédiger, expliquer du code ; clé dans le trousseau). | `tidy-integrations`, `tidy-widgets` |
- 🛑 **Validation.**

### Vague 3 · Fonds d'écran et finition (jalons 6 et 7), 3 lots
| Lot | Contenu |
|---|---|
| **J · Fonds d'écran** | Bibliothèque locale (import de fichiers et de dossiers, catégories, favoris), fonds fixes, vidéo et couleur unie, playlist (ordre ou aléatoire, de 5 min à 24 h), volume (muet par défaut), un fond par écran ou le même partout, ajustement (remplir, ajuster, étirer), pause intelligente, `WallpaperSource` en ligne désactivée par défaut (Bing du jour en exemple). |
| **K · Finition** | Écran des autorisations, réglages complets, `bundle_macos.sh` (Tidy.app, `LSUIElement`), `.dmg`, instructions de signature et de notarisation, README avec les mesures de mémoire et de processeur, nettoyage à la désinstallation (rend le Finder). |
| **L · Vérification** | Relecture indépendante du code, audit de la sécurité des fichiers, mesures du repos (< 1 % de processeur), check-list manuelle complète. |
- 🛑 **Validation**, puis portage Windows si tu le veux (D1), puis tes évolutions (EVO-001…).

## 6. Règles données à chaque sous-agent

- Lire `CLAUDE.md`, ce plan et les sections du bilan et du prompt qui concernent son lot.
- Ne travailler que dans ses crates ou dossiers. Si un contrat du socle doit changer, ne pas le faire seul : le noter dans « Décisions à demander ».
- Textes d'interface et commentaires en français (`//!`, `///`), textes visibles passés par `t!`, thème et widgets de `tidy-ui` uniquement, aucune couleur codée en dur.
- Nouveaux champs persistés : `#[serde(default)]`.
- Tests unitaires pour toute logique pure. Pour les essais manuels, `TIDY_DATA_DIR=$(mktemp -d)` et un faux bureau. Ne jamais toucher au vrai `~/Desktop` ni au réglage du Finder sans moi.
- Vérifications : build, test, clippy `-D warnings`, fmt, check Windows. Pas de build release (place sur le disque).
- Commits petits, en français, au format `feat:` / `fix:` / `docs:` / `test:` / `refactor:`, **sans mention de Claude**. Pas de push.
- Compte rendu : ajouté, touché dans l'existant, décisions à demander, à vérifier sur la machine, commits.

## 7. Risques identifiés

1. **Mémoire de la surface plein écran** (≈ 25 à 35 Mo par tampon sur un écran Retina) : mesurée dès la vague 0, avec une fenêtre par boîte comme solution de repli.
2. **Masquage des icônes du Finder** : il faut relancer le Finder, ce qui ferme puis rouvre ses fenêtres. Les icônes doivent revenir à coup sûr, d'où la double surveillance (l'agent surveille le bureau, le bureau surveille l'agent) et la commande de secours.
3. **« Cliquer sur le fond d'écran pour afficher le bureau »** (macOS 14 et suivants) peut gêner en mode superposé. En mode géré, la surface reçoit le clic à la place du fond d'écran.
4. **Autorisations liées à la signature** : avec une signature ad hoc, macOS peut redemander les autorisations après chaque build, comme pour Snappy. Un certificat de développeur règle le problème (vague 3).
5. **Bureau synchronisé avec iCloud** : sans effet sur les boîtes, puisque rien ne bouge, mais à tester avec les fichiers non téléchargés (icônes nuage).
