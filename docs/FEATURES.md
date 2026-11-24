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
| Panneau Diagnostic | fait | 1 | version, dossiers, écrans, « Copier le rapport » |
| Démarrage à l'ouverture de session | fait | 0 | `SMAppService`, seulement depuis `Tidy.app` |
| Raccourcis globaux | fait | 1 | éditeur, conflits internes, avertissement pour les raccourcis du système |
| Assistant de premier lancement (Démarrage rapide / Personnaliser) | fait | 1 | relançable depuis Options |
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
| Positions par configuration d'écrans | fait | 1 | changements d'écran à chaud |
| Double-clic pour masquer | partiel | 0 bis | Windows : crochet souris bas niveau + sélection vide ; macOS : à faire |

## Boîtes

« fait » = fait et vérifié sous macOS ; sous Windows, compilé et testé en CI, essais réels à faire (`docs/manual-tests.md`).

| Fonction | Statut | Vague | Notes |
|---|---|---|---|
| Créer, déplacer, redimensionner, renommer, supprimer une boîte | fait | 1 | suppression : éléments rendus au bureau, aucun fichier touché |
| Création en dessinant un rectangle | partiel | 1 | Windows : crochet souris + bouton « Créer une boîte » ; macOS : pas encore |
| Menu du bureau « Créer une boîte » | partiel | 0 bis | Windows : verbe du registre (Win 11 : « Afficher plus d'options ») ; `tidy --create-box` |
| Contenu réel : grille/liste, tri, regroupement par date, sélection, ouverture | fait | 1 | simple ou double clic selon le réglage |
| Icônes système et miniatures | fait | 1 | cache borné ; repli Lucide par catégorie |
| Menu d'un élément | fait | 1 | Windows : menu natif de l'Explorateur ; macOS : menu Tidy (ouvrir, afficher, renommer, corbeille, copier le chemin, retirer) |
| Renommer un fichier, corbeille | fait | 1 | jamais de suppression définitive |
| Glisser-déposer | partiel | 1 | dépôt d'un élément du bureau dans une boîte ; glisser vers l'Explorateur/Finder ; fichier venu d'ailleurs refusé (message) |
| Rangement automatique, règles, priorités, fourre-tout, non affectés | fait | 1 | classement des raccourcis `.lnk`, liens `.url`/`.webloc`, jeux |
| Démarrage rapide (boîtes par défaut) | fait | 1 | « Programmes », « Dossiers », « Fichiers et documents » ; `tidy --organize` |
| Nouveaux fichiers rangés à leur arrivée | fait | 1 | surveillance du bureau (`notify`), renommage reconnu |
| « Appliquer aux non affectées / à toutes », « Tout annuler » | fait | 1 | copie `desktop.before-organize.json` |
| Bandeau des icônes non affectées | fait | 1 | pilule en bas de l'écran |
| Tri (nom naturel, type, date, taille, manuel) | fait | 1 | |
| Apparence (couleurs, titre, opacité, coins, bordure, icônes) | partiel | 1 | fond « verre dépoli » pas fait |
| Enroulement, Smarter Roll, enroulement au bord | fait | 1 | |
| Onglets (fusionner, détacher, renommer) | fait | 1 | |
| Aimantation (bords, autres boîtes, 10 px, Maj) | fait | 1 | |
| Accès rapide (maintenir / bascule), raccourci par boîte | fait | 1 | à essayer sous Windows (premier plan) |
| Portail de dossier | fait | 1 | en direct, sous-dossiers, fil d'Ariane ; refusé pour un dossier du bureau |
| Icônes rangées masquées du bureau système | partiel | 1 | Windows (ADR 012) ; macOS : non |
| Bureau public de Windows (`C:\Users\Public\Desktop`) | à faire | 2 | |
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
