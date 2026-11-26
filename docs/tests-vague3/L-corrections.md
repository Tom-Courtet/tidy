# Tests manuels Windows · corrections de la relecture (lot L)

Sur un vrai PC Windows 10/11, avec le `tidy.exe` de la CI. Pour un essai à part :
`$env:TIDY_DATA_DIR = "$env:TEMP\tidy-l"` et `$env:TIDY_DESKTOP_DIR = "$env:TEMP\bureau-l"`
(dossier créé avant, avec quelques fichiers), dans chaque console qui lance `tidy.exe`.
Journaux : `<données>\logs\`.

## Démarrage du bureau (point 1)
- [ ] Une boîte avec une règle (par exemple « Images »), Tidy arrêté ; copier trois images dans le bureau d'essai, puis lancer Tidy : les trois images arrivent dans la boîte dès que le bureau s'affiche (rattrapage de la surveillance), sans relance.
- [ ] Fenêtre principale > Dispositions : restaurer une disposition juste après « Boîtes activées » (menu de la zone de notification : désactiver puis réactiver) : la disposition est bien appliquée quand le bureau apparaît.
- [ ] Journal de l'agent : pas d'erreur « commande du bureau perdue ».

## Agendas ICS (point 2)
- [ ] Widget Calendrier, ajouter un agenda `https://…` : il se charge ; `desktop.json` et les fichiers de `snapshots\` ne contiennent pas l'adresse (chercher un morceau de l'adresse secrète), seulement l'hôte.
- [ ] Gestionnaire d'identifiants Windows > Informations d'identification Windows : une entrée « Tidy » / `ics-<identifiant>` par agenda.
- [ ] Une adresse `http://…` est refusée (message « Adresse refusée… »).
- [ ] Retirer l'agenda (icône corbeille) : son entrée disparaît du gestionnaire d'identifiants.
- [ ] Ancien `desktop.json` avec une adresse en clair (version précédente) : au premier affichage, l'adresse passe dans le gestionnaire d'identifiants et disparaît de `desktop.json` ; l'agenda s'affiche toujours.

## Dialogue entre processus et instance unique (points 3 et 4)
- [ ] `type "$env:APPDATA\Tidy\data\tidy.sock"` (ou le dossier d'essai) : deux lignes, le port puis un jeton de 64 caractères hexadécimaux, différent à chaque lancement de l'agent.
- [ ] Depuis PowerShell : `$c = New-Object Net.Sockets.TcpClient('127.0.0.1', <port>); $s = $c.GetStream(); $w = New-Object IO.StreamWriter($s); $w.WriteLine('{"type":"quit"}'); $w.Flush()` : Tidy **ne s'arrête pas** (connexion fermée, « connexion refusée : jeton absent ou faux » dans le journal).
- [ ] `tidy.exe --quit`, `--toggle-desktop`, la fenêtre principale, la recherche, les widgets, le fond animé : tout marche comme avant (ils présentent le jeton).
- [ ] Lancer `tidy.exe` une deuxième fois : la fenêtre principale du premier s'ouvre, pas de deuxième icône dans la zone de notification.
- [ ] Tidy arrêté brutalement (Gestionnaire des tâches), un autre programme écoute sur l'ancien port (par exemple `python -m http.server <port>`), relancer Tidy : il démarre normalement.

## Boîtes désactivées (point 5)
- [ ] Boîte privée déverrouillée, un fichier ouvert et modifié, puis menu de la zone de notification > désactiver les boîtes : le bureau disparaît après quelques secondes au plus ; au déverrouillage suivant, la modification est gardée ; plus de copie en clair dans `vault\open.noindex\`.

## Coffre (points 6 et 7)
- [ ] Ouvrir un fichier de la boîte privée, puis le renommer dans la boîte : refusé, message « … a été ouvert depuis la boîte privée : ferme-le, verrouille la boîte, puis recommence ». Même chose pour « Mettre à la corbeille ».
- [ ] Après verrouillage puis déverrouillage : renommage et corbeille marchent.
- [ ] Sortir un élément du coffre vers le bureau alors qu'un fichier du même nom y est : l'élément arrive sous un autre nom (« … (2) »), l'existant est intact.
- [ ] Renommer un élément d'une boîte en changeant seulement la casse (`photo.JPG` → `photo.jpg`) : marche.

## Fenêtre principale (point 8)
- [ ] Widgets : écrire dans une note, puis tout de suite (fenêtre principale ouverte) cliquer « Geler » puis « Exclure du masquage rapide » sur ce widget : le texte, la place et la taille du widget ne reviennent pas en arrière.
- [ ] Dispositions > « Réinitialiser les autres résolutions » après avoir déplacé une boîte : la boîte reste où elle est.

## Fonds d'écran (points 9, 14, 18)
- [ ] Premier fond posé par Tidy : `wallpaper-original.json` existe avant que le fond change ; « Rendre le fond d'origine » le rend.
- [ ] Fonds désactivés : l'agent ne se réveille plus pour les fonds (Gestionnaire des tâches > Détails : colonne « Changements de contexte » quasi immobile pour l'agent).
- [ ] Vidéo en pause (plein écran d'une autre appli) : le processus `tidy.exe --wallpaper` ne consomme plus rien (processeur 0 %).
- [ ] Deux écrans, un fond « le même partout », puis choisir un autre fond pour l'écran 1 seulement : l'écran 2 garde son fond (même vidéo : elle continue).
- [ ] Bibliothèque de plus de 100 fonds : la page Fonds d'écran défile sans à-coups ; mémoire de la fenêtre principale stable en défilant.

## Icônes et arrêt (points 10 et 11)
- [ ] Icônes rangées masquées, `icon-stash.json` supprimé à la main, puis `tidy.exe --restore-desktop` : les icônes reviennent à des places libres visibles (« … icône(s) hors de l'écran ramenée(s) »).
- [ ] Faire planter le bureau trois fois en une minute (Gestionnaire des tâches > Fin de tâche sur `tidy.exe --desktop`) : les icônes rangées reviennent et ne sont plus remasquées (ajouter un fichier au bureau ensuite : il reste visible).
- [ ] « Quitter » pendant que la boîte privée verrouille un gros fichier modifié : la souris reste fluide pendant l'arrêt.
- [ ] Mettre à la corbeille un gros dossier depuis une boîte : la souris reste fluide.

## Divers (points 12 à 21)
- [ ] Recherche : copier 500 fichiers d'un coup dans Documents : la recherche les trouve, l'agent ne monte pas longtemps à 100 % d'un cœur.
- [ ] Note : taper un mot puis quitter Tidy aussitôt (moins d'une seconde) : le mot est dans la note au redémarrage.
- [ ] Menu natif d'un fichier dans une boîte, Maj enfoncée en cliquant « Supprimer » : le fichier va à la Corbeille (pas de suppression définitive).
- [ ] Search Mini : une recherche qui échoue (agent arrêté) ne laisse pas le texte tapé dans `logs\`.
- [ ] Rangement > « Tout annuler » : un instantané « avant restauration » apparaît dans Dispositions ; le restaurer rend le rangement.
- [ ] Verrouiller la session (Win+L) puis la rouvrir : la boîte privée suit le réglage de verrouillage comme avant.
- [ ] Calendrier avec un rappel à 5 minutes, relancer Tidy pendant ces 5 minutes : un seul rappel.
- [ ] Calendrier en erreur (réseau coupé), cliquer plusieurs fois « Réessayer » : un seul chargement (journal).
- [ ] Chat IA : « Arrêter » pendant que la réponse arrive, ou avant qu'elle commence : l'arrêt est immédiat (« Arrêté »).
