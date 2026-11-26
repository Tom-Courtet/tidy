# Tests manuels Windows · lot G (recherche rapide)

À faire sur le PC Windows 10/11 avec `tidy.exe` de la CI. Les essais sur un faux bureau
se font avec `TIDY_DATA_DIR` (dossier vide) et `TIDY_DESKTOP_DIR` ; la recherche lit
aussi les vrais Documents, Téléchargements, Images, Musique, Vidéos et le menu Démarrer
(lecture seule). Journaux : `<données>\logs\` (lignes « recherche : … »).

## Ouverture et fermeture
- [ ] Alt+Espace ouvre le lanceur, centré sur l'écran où est la souris, au premier plan, le champ prêt à la saisie (essayer depuis le bureau, puis depuis une appli au premier plan : Edge, Explorateur).
- [ ] Alt+Espace une deuxième fois (lanceur ouvert et actif) le referme.
- [ ] Échap ferme ; cliquer dans une autre fenêtre le ferme aussi.
- [ ] Deuxième ouverture : instantanée (processus gardé en mémoire), champ vide, onglet « Tout ».
- [ ] Deux écrans, DPI différents : le lanceur s'ouvre sur l'écran de la souris, à une taille correcte.
- [ ] Le lanceur n'apparaît pas dans la barre des tâches ; Alt+F4 le cache sans arrêter le processus (Gestionnaire des tâches : un `tidy.exe --search` reste).
- [ ] Quitter Tidy (icône de la zone de notification) arrête aussi le processus du lanceur.
- [ ] Options > « Garder en mémoire » désactivé : après fermeture, plus de processus `--search` ; Alt+Espace le relance.
- [ ] `tidy.exe --search facture` (agent lancé) ouvre le lanceur avec « facture » saisi, sans second processus.

## Recherche
- [ ] Premier lancement (données vides) : « Création de l'index… », puis des résultats en quelques secondes. Noter dans les journaux le nombre d'éléments et la durée du parcours.
- [ ] Taper : les résultats suivent la frappe sans saccade ; la ligne d'état donne le temps (ms) de chaque requête (noter la valeur typique).
- [ ] Insensible à la casse et aux accents : « ete » trouve « Été 2024.jpg » ; « ÉTÉ » aussi.
- [ ] Plusieurs mots : « fact mars » trouve « Facture mars.pdf ».
- [ ] Applis : « fire » (ou une appli installée) trouve son raccourci du menu Démarrer, affiché sans « .lnk », avec son icône ; Entrée la lance.
- [ ] Onglets : Bureau, Applis, Documents, Images, Vidéos, Musique, Dossiers, Archives filtrent ; Tab et Maj+Tab passent d'un onglet à l'autre. Onglet sans saisie : ses éléments (applis par nom, le reste du plus récent au plus ancien).
- [ ] Exclusions : rien de `node_modules`, `.git`, `$Recycle.Bin`, ni fichiers cachés/système (`desktop.ini`).
- [ ] Mise à jour en direct : créer, renommer, supprimer un fichier dans Documents ; le lanceur ouvert le reflète en moins de 3 s.
- [ ] Redémarrer le PC (ou quitter Tidy) puis rouvrir : résultats immédiats (cache `search\index.bin`), puis mise à jour silencieuse.

## Actions
- [ ] ↑↓, Page préc./suiv. déplacent la sélection, la liste défile pour la suivre.
- [ ] Entrée ou double-clic ouvre l'élément et ferme le lanceur ; il apparaît ensuite dans « Ouverts récemment » (champ vide) et remonte dans les résultats (badge « Récent »).
- [ ] Ctrl+Entrée affiche l'élément sélectionné dans l'Explorateur.
- [ ] Ctrl+Maj+C copie le chemin complet (coller dans le Bloc-notes).
- [ ] Boutons au survol de la ligne : afficher dans l'Explorateur, copier le chemin, corbeille.
- [ ] Corbeille : demande de confirmation, puis le fichier est dans la Corbeille (restaurable) et disparaît de la liste. Annuler ne touche à rien.
- [ ] Clic droit sur un résultat : menu de l'Explorateur (Ouvrir avec, Copier, Couper, Propriétés…).
- [ ] La roue dentée ouvre la fenêtre principale sur la page Recherche.

## Search Mini (quand l'hôte des widgets est fusionné)
- [ ] Ajouter « Search Mini » au bureau, taper un mot, Entrée : le lanceur s'ouvre avec ce mot.
- [ ] Clic sur la loupe du widget : le lanceur s'ouvre.

## Page Recherche (fenêtre principale)
- [ ] Changer le raccourci (ex. Ctrl+Alt+Espace) : l'ancien ne fait plus rien, le nouveau ouvre le lanceur ; conflit signalé avec un autre raccourci de Tidy.
- [ ] Désactiver « Téléchargements » : ses fichiers disparaissent des résultats après quelques secondes.
- [ ] Ajouter un dossier (ex. `D:\Photos`) : ses fichiers apparaissent ; le retirer : ils disparaissent.
- [ ] Ajouter une exclusion `*.tmp`, puis un chemin complet : les éléments correspondants disparaissent.
- [ ] Masquer des onglets : ils disparaissent du lanceur ; « Tout » reste.
- [ ] « Reconstruire l'index » : la ligne d'état du lanceur montre « mise à jour de l'index… ».
- [ ] « Effacer les récents » : le champ vide du lanceur ne montre plus rien.

## Mémoire
- [ ] Gestionnaire des tâches > Détails : mémoire (« Plage de travail privée ») de `tidy.exe --search` fenêtre cachée, avec l'index réel ; noter le nombre d'éléments. L'agent (`tidy.exe` sans argument) doit rester sous ~20 Mo.
