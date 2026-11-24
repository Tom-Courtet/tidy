# Tidy : tableau de parité

Référence : `prompt-easy-desktop-rust.md` (le prompt, marqueurs [C] confirmé / [V] à vérifier) et `iTop_Easy_Desktop.md` (le bilan, observations locales). Statuts : **fait**, **partiel**, **à faire**, **hors périmètre**. Vague : voir `PLAN.md` § 5.

## Socle

| Fonction | Statut | Vague | Notes |
|---|---|---|---|
| Workspace, crates, lints (`unsafe` confiné) | fait | 0 | |
| Agent dans la barre des menus, sans Dock | fait | 0 | icône modèle (s'adapte au thème) |
| Instance unique, réouverture → fenêtre principale | fait | 0 | ping du socket + événement Apple « rouvrir » |
| Fenêtre principale, navigation latérale | partiel | 0 | Accueil, Options, Diagnostic ; autres pages annoncées |
| i18n FR/EN, extensible | fait | 0 | `t!`, TOML embarqués, test des clés |
| Configuration JSON versionnée, migrations, écriture atomique | fait | 0 | `config.json`, `desktop.json` |
| Journaux avec rotation (7 jours) | fait | 0 | `tracing`, `logs/<rôle>.log.*` |
| Panneau Diagnostic | partiel | 0 | version, dossiers, Finder, écrans ; rapport à copier : vague 3 |
| Démarrage à l'ouverture de session | fait | 0 | `SMAppService`, seulement depuis `Tidy.app` |
| Raccourcis globaux | partiel | 0 | enregistrés depuis les réglages ; éditeur et conflits : vague 1 |
| Assistant de premier lancement (Démarrage rapide / Personnaliser) | à faire | 1 | |
| Interrupteur global des boîtes | fait | 0 | menu de la barre des menus, Options |

## Bureau

| Fonction | Statut | Vague | Notes |
|---|---|---|---|
| Une fenêtre par boîte, posée sur le bureau | partiel | 0 bis | macOS vérifié ; Windows : propriétaire `Progman` + toujours en bas (à tester sur PC) |
| Prototype Windows (Win+D, DPI, Explorateur) | partiel | 0 bis | compilé et testé en CI ; tests manuels sur PC à faire |
| CI GitHub : tests Windows et macOS, artefact `tidy.exe` | fait | 0 bis | |
| Mode superposé (clics traversants hors des boîtes) | partiel | 0 | moniteurs d'événements, sans minuteur |
| Mode géré (icônes du Finder masquées) | partiel | 0 | macOS ; plus le mode par défaut visé (ADR 011) |
| Icônes de l'Explorateur : tout masquer / montrer | partiel | 0 bis | `ShowWindow` sur la liste du bureau, sans injection ; à tester |
| Icônes rangées masquées du bureau système | partiel | 0 bis | Windows : déplacées hors de l'écran, journal des positions (ADR 012) ; branchement au contenu des boîtes : vague 1 |
| Sécurité du Finder (plantages, arrêt, secours) | fait | 0 | 3 plantages/min → retour des icônes ; `--restore-finder` |
| Positions par configuration d'écrans | partiel | 0 | modèle et tests ; changements d'écran à chaud : vague 1 |
| Double-clic pour masquer | partiel | 0 bis | Windows : crochet souris bas niveau + sélection vide ; macOS : à faire |

## Boîtes

| Fonction | Statut | Vague | Notes |
|---|---|---|---|
| Création manuelle, déplacer, redimensionner, renommer, supprimer | partiel | 0 bis | créer, déplacer, redimensionner, supprimer : faits ; renommer : vague 1 |
| Création en dessinant un rectangle | à faire | 1 | surcouche temporaire (ADR 011) |
| Menu du bureau « Créer une boîte » | partiel | 0 bis | Windows : verbe du registre (Win 11 : « Afficher plus d'options ») ; `tidy --create-box` |
| Glisser-déposer (bureau, boîtes, Finder) | à faire | 1 | |
| Icônes système, miniatures, ouverture, menu contextuel | à faire | 1 | |
| Rangement automatique, règles par catégorie, priorités, fourre-tout | partiel | 1 | moteur de règles et catégories (annexe A) faits |
| « Appliquer aux non affectées » / « à toutes » | à faire | 1 | |
| Tri (nom, type, date, taille, manuel), regroupement par date | à faire | 1 | modèle prêt |
| Apparence (couleurs, police, transparence, coins, verre) | à faire | 1 | modèle prêt |
| Enroulement, Smarter Roll, enroulement au bord | à faire | 1 | |
| Onglets dans une boîte | à faire | 1 | modèle prêt |
| Raccourci par boîte, accès rapide (maintenir / bascule) | à faire | 1 | événements prêts |
| Aimantation (alignement, 10 px, Maj) | à faire | 1 | |
| Portail de dossier | à faire | 1 | interdit sur un dossier du bureau |
| Boîte privée chiffrée | à faire | 2 | ADR 006 |

## Dispositions, recherche, widgets, fonds d'écran

| Fonction | Statut | Vague | Notes |
|---|---|---|---|
| Instantanés auto (50), manuels, restauration | à faire | 2 | |
| Dispositions nommées et raccourcis | à faire | 2 | [V] dans le prompt |
| Recherche rapide (index, Spotlight, onglets, actions) | à faire | 2 | ADR 007 |
| Search Mini | à faire | 2 | |
| Horloge, iNotes, iPomodoro, Quick Tools | à faire | 2 | Quick Tools : tuile Snappy (ADR 005) |
| Météo (Open-Meteo), Calendrier (EventKit), Chat IA (Claude) | à faire | 2 | ADR 004, 008 |
| Fonds statiques, vidéo, couleur, playlist, pause intelligente | à faire | 3 | |
| Capture d'écran | hors périmètre | – | déléguée à Snappy (ADR 005) |
| Personnalisation de Windows, PawPal, créateur de fonds, bibliothèque en ligne | hors périmètre | – | |

## Évolutions

(À venir : `EVO-001`…)
