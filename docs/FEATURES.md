# Tidy : tableau de parité

Référence : `prompt-easy-desktop-rust.md` (le prompt, marqueurs [C] confirmé / [V] à vérifier) et `iTop_Easy_Desktop.md` (le bilan, observations locales). Statuts : **fait**, **partiel**, **à faire**, **hors périmètre**. Vague : voir `PLAN.md` § 5.

## Socle

| Fonction | Statut | Vague | Notes |
|---|---|---|---|
| Workspace, crates, lints (`unsafe` confiné) | fait | 0 | |
| Agent dans la barre des menus, sans Dock | fait | 0 | icône modèle (s'adapte au thème) |
| Instance unique, réouverture → fenêtre principale | fait | 0 | ping du socket + événement Apple « rouvrir » |
| Fenêtre principale, navigation latérale | fait | 2 | toutes les pages : Accueil, Boîtes, Rangement, Actions rapides, Dispositions, Fonds d'écran, Widgets, Recherche, Options, Diagnostic |
| i18n FR/EN, extensible | fait | 0 | `t!`, TOML embarqués, test des clés |
| Configuration JSON versionnée, migrations, écriture atomique | fait | 0 | `config.json`, `desktop.json` |
| Journaux avec rotation (7 jours) | fait | 0 | `tracing`, `logs/<rôle>.log.*` |
| Panneau Diagnostic | fait | 1 | version, dossiers, écrans, « Copier le rapport » |
| Démarrage à l'ouverture de session | fait | 0 | `SMAppService`, seulement depuis `Tidy.app` |
| Raccourcis globaux | fait | 1 | éditeur, conflits internes, avertissement pour les raccourcis du système |
| Assistant de premier lancement (Démarrage rapide / Personnaliser) | fait | 1 | relançable depuis Options |
| Interrupteur global des boîtes | fait | 0 | menu de la barre des menus, Options |
| Installation Windows, mise à jour, désinstallation complète | fait | 3 | `tidy --install` (dossier des programmes, menu Démarrer, « Applications et fonctionnalités ») ; `--uninstall` garde les données et le coffre |
| Instance unique, dialogue entre processus protégé | fait | 3 | verrou `agent.lock`, jeton aléatoire, socket en 0600 |
| macOS : Tidy.app, .dmg, signature et notarisation documentées | fait | 3 | `tools/bundle_macos.sh`, `tools/dmg_macos.sh`, `docs/distribution.md` |

## Bureau

| Fonction | Statut | Vague | Notes |
|---|---|---|---|
| Une fenêtre par boîte, posée sur le bureau | partiel | 0 bis | macOS vérifié ; Windows : propriétaire `Progman` + toujours en bas (à tester sur PC) |
| Prototype Windows (Win+D, DPI, Explorateur) | partiel | 0 bis | compilé et testé en CI ; tests manuels sur PC à faire |
| CI GitHub : tests Windows et macOS, artefact `tidy.exe` | fait | 0 bis | |
| Mode superposé (clics traversants hors des boîtes) | partiel | 0 | moniteurs d'événements, sans minuteur |
| Mode géré (icônes du Finder masquées) | partiel | 0 | macOS ; plus le mode par défaut visé (ADR 011) |
| Icônes de l'Explorateur : tout masquer / montrer | partiel | 0 bis | `ShowWindow` sur la liste du bureau, sans injection ; à tester |
| Icônes rangées masquées du bureau système | partiel | 1 | Windows : déplacées hors de l'écran, journal des positions, verrou entre processus, repli sans alignement sur la grille (ADR 012) ; à confirmer sur PC |
| Sécurité du Finder (plantages, arrêt, secours) | fait | 0 | 3 plantages/min → retour des icônes ; `--restore-finder` |
| Positions par configuration d'écrans | fait | 1 | changements d'écran à chaud |
| Double-clic pour masquer | partiel | 0 bis | Windows : crochet souris bas niveau + sélection vide ; macOS : à faire |

## Boîtes

« fait » = fait et vérifié sous macOS ; sous Windows, compilé et testé en CI, essais réels à faire (`docs/manual-tests.md`).

| Fonction | Statut | Vague | Notes |
|---|---|---|---|
| Créer, déplacer, redimensionner, renommer, supprimer une boîte | fait | 1 | suppression : éléments rendus au bureau, aucun fichier touché |
| Création en dessinant un rectangle | fait | 2 | Windows : crochet souris ; macOS : moniteur d'événements global (sans autorisation) ; bouton « Créer une boîte » |
| Menu du bureau « Créer une boîte » | partiel | 0 bis | Windows : verbe du registre (Win 11 : « Afficher plus d'options ») ; `tidy --create-box` |
| Contenu réel : grille/liste, tri, regroupement par date, sélection, ouverture | fait | 1 | simple ou double clic selon le réglage ; grille libre (cases vides possibles) en ordre manuel |
| Icônes système et miniatures | fait | 1 | cache borné ; repli Lucide par catégorie |
| Menu d'un élément | fait | 1 | Windows : menu natif de l'Explorateur ; macOS : menu Tidy (ouvrir, afficher, renommer, corbeille, copier le chemin, retirer) |
| Renommer un fichier, corbeille | fait | 1 | jamais de suppression définitive |
| Menu ☰ comme l'original (créer, affichage, règles, réglages, actualiser, geler) | fait | 2 | recherche dans la barre de titre, « Ranger ici… », exclusions du masquage et de l'accès rapide |
| Glisser-déposer | partiel | 1 | dépôt d'un élément du bureau dans une boîte ; glisser vers l'Explorateur/Finder ; fichier venu d'ailleurs refusé (message) |
| Rangement automatique, règles, priorités, fourre-tout, non affectés | fait | 1 | classement des raccourcis `.lnk`, liens `.url`/`.webloc`, jeux |
| Démarrage rapide (boîtes par défaut) | fait | 1 | « Programmes », « Dossiers », « Fichiers et documents » ; `tidy --organize` |
| Nouveaux fichiers rangés à leur arrivée | fait | 1 | surveillance du bureau (`notify`), renommage reconnu |
| « Appliquer aux non affectées / à toutes », « Tout annuler » | fait | 1 | copie `desktop.before-organize.json` |
| Bandeau des icônes non affectées | fait | 1 | pilule en bas de l'écran |
| Tri (nom naturel, type, date, taille, manuel) | fait | 1 | |
| Apparence (couleurs, titre, opacité, coins, bordure, icônes, verre dépoli) | fait | 2 | verre : acrylique sous Windows 11, flou sous Windows 10 (`TIDY_GLASS=blur` pour forcer), `NSVisualEffectView` sous macOS |
| Enroulement, Smarter Roll, enroulement au bord | fait | 1 | |
| Onglets (fusionner, détacher, renommer) | fait | 1 | |
| Aimantation (bords, autres boîtes, 10 px, Maj) | fait | 1 | |
| Accès rapide (maintenir / bascule), raccourci par boîte | fait | 1 | à essayer sous Windows (premier plan) |
| Portail de dossier | fait | 1 | en direct, sous-dossiers, fil d'Ariane ; tout dossier sauf le bureau lui-même (l'original refuse aussi les dossiers du bureau) |
| Icônes rangées masquées du bureau système | partiel | 1 | Windows (ADR 012) ; macOS : non |
| Bureau public de Windows (`C:\Users\Public\Desktop`) | fait | 1 | ses éléments sont rangés comme les autres (26/11) |
| Boîte privée chiffrée | fait | 2 | ADR 013 (remplace l'ADR 006) : chiffrement fichier par fichier, même coffre sous Windows et macOS ; clé de récupération à la place du courriel ; verrouillage auto (inactivité, veille, session), opacité jusqu'à 0 % ; essais Windows : `docs/tests-vague2/F-coffre.md` |

## Dispositions, recherche, widgets, fonds d'écran

| Fonction | Statut | Vague | Notes |
|---|---|---|---|
| Instantanés auto (50), manuels, restauration | fait | 2 | premier et manuels protégés, miniatures, instantané de sécurité avant restauration, menu de l'icône |
| Changement de résolution (redimensionner, mémoriser, rien) | fait | 2 | boîtes et widgets |
| Dispositions nommées et raccourcis | fait | 2 | raccourci global par disposition, doublons refusés partout |
| Recherche rapide (index, Spotlight, onglets, actions) | fait | 2 | `tidy --search`, Alt+Espace, index maison (< 20 ms sur 500 000 éléments), menu Démarrer, cache, `mdfind` sous macOS ; Windows Search : non |
| Search Mini | fait | 2 | widget |
| Horloge, iNotes, iPomodoro, Quick Tools | fait | 2 | une fenêtre par widget (≈ 19 Mo chacune en debug) ; notes archivées, jamais supprimées sans corbeille ; tuile Snappy (ADR 005) |
| Météo (Open-Meteo), Calendrier, Chat IA (Claude) | fait | 2 | Calendrier : adresses ICS (Google, Outlook) avec récurrences et rappels ; EventKit : plus tard ; clé de l'API dans le gestionnaire d'identifiants |
| Fonds statiques, vidéo, couleur, playlist, pause intelligente | fait | 3 | `tidy --wallpaper` (MFPlay / AVPlayerLayer derrière les icônes), fond d'origine rendu, image du jour de Bing (désactivée par défaut) |
| Capture d'écran | hors périmètre | – | déléguée à Snappy (ADR 005) |
| Personnalisation de Windows, PawPal, créateur de fonds, bibliothèque en ligne | hors périmètre | – | |

## Évolutions

(À venir : `EVO-001`…)
