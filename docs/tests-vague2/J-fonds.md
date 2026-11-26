# Tests manuels Windows · lot J (fonds d'écran)

Essai sans toucher aux vrais réglages de Tidy (le fond d'écran, lui, est bien celui de
Windows : **noter d'abord le fond actuel**, Paramètres > Personnalisation > Arrière-plan,
par une capture d'écran) :

```powershell
$env:TIDY_DATA_DIR = "$env:TEMP\tidy-fonds"; .\tidy.exe
```

Préparer : quelques images (JPEG, PNG, une en WebP, une très grande), deux vidéos MP4
(1080p et 4K si possible), une MOV et une WebM, dans un dossier avec un sous-dossier.
Journaux utiles : `<données>\logs\` (`$env:TIDY_LOG = "debug"` pour plus de détails).

## Bibliothèque (page Fonds d'écran)
- [ ] « Importer des fichiers » : les images et vidéos choisies apparaissent dans la grille ; message « N fond(s) ajouté(s)… ».
- [ ] Réimporter les mêmes fichiers : « déjà présent(s) », pas de doublon.
- [ ] « Importer un dossier » : les fichiers du dossier **et de ses sous-dossiers** arrivent, dans une catégorie au nom du dossier.
- [ ] Les miniatures apparaissent (images **et vidéos** : la vignette de l'Explorateur) ; relancer la fenêtre : elles s'affichent tout de suite (cache).
- [ ] Les fichiers importés n'ont **pas bougé** (ni copiés, ni déplacés) dans l'Explorateur.
- [ ] Favori (étoile), filtre « Favoris », filtres Images / Vidéos, nouvelle catégorie, changement de catégorie, « Supprimer la catégorie » (les fonds restent).
- [ ] « Retirer de la bibliothèque » : le fond disparaît de la grille, **le fichier reste** à sa place.
- [ ] Renommer ou déplacer un fichier importé dans l'Explorateur : sa miniature porte un triangle rouge et « Fichier introuvable ».

## Fonds fixes et couleur unie
- [ ] Double-clic sur une image (« Tous les écrans ») : elle devient le fond d'écran de Windows en une seconde environ.
- [ ] `<données>\wallpaper-original.json` existe et nomme le fond d'origine (si c'était `TranscodedWallpaper`, une copie est dans `<données>\wallpapers\original\`).
- [ ] Ajustement : Remplir, Ajuster (bandes de la « couleur autour de l'image »), Étirer, Centrer, Mosaïque : chaque choix change le fond aussitôt.
- [ ] Image WebP : appliquée correctement (Tidy en fait une copie PNG dans `<données>\wallpapers\converted\`).
- [ ] Couleur unie : choisir une pastille puis « Appliquer la couleur » : le fond devient cette couleur.
- [ ] Deux écrans : choisir un écran puis appliquer une image ; choisir l'autre, appliquer une autre image : un fond différent par écran. (Limite connue : l'ajustement est commun à tous les écrans.)
- [ ] « Laisser le fond du système » sur un écran : Tidy n'y touche plus.

## Fonds vidéo
- [ ] Appliquer une vidéo MP4 : elle joue **en boucle, derrière les icônes** du bureau (icônes et boîtes de Tidy restent visibles et cliquables), **sans son**.
- [ ] Gestionnaire des tâches : un processus `tidy.exe --wallpaper` existe ; noter sa mémoire et son processeur (1080p, puis 4K) ; noter l'usage du GPU « Video Decode » (décodage matériel).
- [ ] La boucle : à la fin de la vidéo, la reprise se fait au début (noter si un court arrêt se voit).
- [ ] Ajustements avec une vidéo : Remplir (rognée), Ajuster (bandes), Étirer.
- [ ] Son : couper « Muet », régler le volume : le son suit ; deux écrans avec la même vidéo : le son ne sort qu'une fois.
- [ ] Win+D : la vidéo reste ; ouvrir des fenêtres par-dessus : la vidéo reste dessous.
- [ ] Repasser à une image fixe : le processus `--wallpaper` disparaît et le bureau montre l'image (pas une dernière image figée de la vidéo).
- [ ] Tuer `tidy.exe --wallpaper` dans le Gestionnaire des tâches : il revient en une seconde ; le tuer trois fois de suite en moins d'une minute : Tidy abandonne et la page le signale.
- [ ] WebM et MOV : jouent (WebM demande l'extension « VP9 » de Windows).
- [ ] **Windows 11 24H2** : la vidéo est bien derrière les icônes. Sinon, quitter Tidy, relancer avec `$env:TIDY_WALLPAPER_HOST = "workerw"` et réessayer ; me dire laquelle marche (journal : « fond animé : hôte … »).
- [ ] Clic droit sur le bureau, sélection rectangulaire d'icônes, double-clic pour masquer : tout marche comme sans vidéo.

## Playlist
- [ ] Ajouter 3 fonds (images et vidéo mêlées), durée 5 min, « Utiliser la playlist » : le fond change toutes les 5 minutes, dans l'ordre.
- [ ] « Fond suivant » : passe tout de suite au suivant ; la page affiche « fond suivant dans … ».
- [ ] Aléatoire : jamais deux fois le même de suite, tous passent avant qu'un revienne.
- [ ] Monter un fond, le retirer de la playlist : l'ordre suit.

## Pause intelligente
- [ ] Jeu ou vidéo YouTube en plein écran (F11) : la vidéo du fond se met en pause (processeur du `--wallpaper` presque à 0) ; sortie du plein écran : elle reprend.
- [ ] Portable débranché (réglage « toujours ») : pause ; rebranché : reprise. Seuil à 30 % : pause seulement sous 30 %.
- [ ] Win+L puis retour : la vidéo était en pause pendant le verrouillage.
- [ ] Double-clic sur le bureau (masquer) : pause ; second double-clic : reprise.
- [ ] Pendant la pause, la playlist n'avance pas (et le temps restant est décalé d'autant).

## Sources en ligne
- [ ] Par défaut, « Image du jour de Bing » est désactivé ; aucun accès réseau (pare-feu ou journal).
- [ ] L'activer puis « Utiliser l'image du jour » : en quelques secondes, l'image du jour devient le fond ; elle est dans `<données>\wallpapers\online\bing\`.

## Fond d'origine
- [ ] « Rendre le fond d'origine » : le fond d'avant Tidy revient (comparer à la capture) et l'interrupteur « Tidy gère le fond d'écran » s'éteint.
- [ ] Éteindre l'interrupteur : même effet ; le processus `--wallpaper` s'arrête.
- [ ] Réappliquer un fond, puis `.\tidy.exe --uninstall` : le fond d'origine revient.
- [ ] Quitter Tidy (menu de l'icône) avec une vidéo : le processus `--wallpaper` se ferme aussi, le bureau montre le fond fixe.
