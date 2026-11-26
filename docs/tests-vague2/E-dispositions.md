# Tests manuels Windows · Lot E · Dispositions et instantanés

Pour essayer sans toucher à tes vraies données, dans PowerShell :

```powershell
$env:TIDY_DATA_DIR = "$env:TEMP\tidy-essai"; $env:TIDY_LOG = "debug"; .\tidy.exe
```

Les instantanés sont dans `<données>\snapshots\` (`index.json` et un fichier par instantané). Journaux : `<données>\logs\` (lignes « instantané automatique pris », « disposition restaurée »).

## Instantanés automatiques
- [ ] Premier lancement avec au moins une boîte : la page **Dispositions** montre un instantané « Premier instantané » (badge lime « Premier »).
- [ ] Déplacer une boîte, attendre 5 secondes : un instantané « Disposition modifiée » (Auto) apparaît en haut de l'historique, avec le badge « Actuelle ».
- [ ] Déplacer une boîte plusieurs fois de suite (moins de 3 s entre deux mouvements) : **un seul** instantané pour toute la série.
- [ ] Un fichier qui arrive sur le bureau, sans changer de boîte : **pas** de nouvel instantané.
- [ ] Ranger un fichier dans une boîte, renommer une boîte, l'enrouler, créer un onglet : un instantané à chaque fois.
- [ ] Désactiver « Instantanés automatiques », déplacer une boîte : rien de nouveau. Réactiver.
- [ ] Choisir « 10 » instantanés gardés après en avoir plus de 10 : les plus anciens automatiques disparaissent ; le premier et les manuels restent.

## Miniatures
- [ ] Chaque instantané montre une miniature de l'écran principal avec les boîtes à leur place (barre violette en haut de chaque boîte, boîte enroulée réduite à sa barre).
- [ ] Avec deux écrans : seules les boîtes de l'écran principal sont dans la miniature.

## Prendre, renommer, supprimer
- [ ] Saisir un nom puis **Prendre un instantané** : il apparaît avec le badge « Manuel » et ce nom.
- [ ] Crayon → changer le nom → Entrée : renommé ; Échap annule.
- [ ] Corbeille sur un automatique → confirmer : il disparaît. Sur un manuel ou le premier : le message prévient qu'il ne reviendra pas.
- [ ] Aucun fichier du bureau n'est touché par ces actions.

## Restaurer
- [ ] Supprimer une boîte (ses éléments reviennent sur le bureau), puis **Restaurer** l'instantané d'avant : la boîte revient **à sa place**, avec ses éléments, et ces icônes disparaissent à nouveau du bureau de l'Explorateur.
- [ ] Juste après, l'historique garde l'état d'avant : « Avant restauration », ou le dernier « Disposition modifiée » s'il était identique (pas de doublon). Le restaurer ramène la disposition d'avant.
- [ ] Déplacer des boîtes, puis restaurer un ancien instantané : les fenêtres des boîtes **bougent** tout de suite à leurs anciennes places (sans relancer Tidy).
- [ ] Le bouton Restaurer est grisé sur l'instantané « Actuelle ».
- [ ] Restaurer avec la fenêtre principale fermée, via l'icône de la zone de notification → **Dispositions → Revenir à la disposition précédente** : même résultat.

## Dispositions nommées
- [ ] Saisir « Travail » → **Enregistrer la disposition actuelle** : elle apparaît avec sa miniature, son nombre de boîtes et sa date.
- [ ] Changer la disposition, puis **Appliquer** « Travail » → confirmer : la disposition revient (boîtes à leur place) ; l'état d'avant reste dans l'historique.
- [ ] Bouton ↻ (Remplacer) : la disposition prend le contenu actuel (date mise à jour), nom et raccourci gardés.
- [ ] Renommer, supprimer une disposition : seule la disposition change, aucune boîte ni aucun fichier.
- [ ] Menu de l'icône → **Dispositions** : les dispositions nommées y sont ; cliquer l'une l'applique. « Gérer les dispositions… » ouvre cette page.

## Raccourcis des dispositions
- [ ] Donner `Ctrl+Alt+1` à « Travail » : depuis n'importe quelle appli, `Ctrl+Alt+1` applique la disposition.
- [ ] Donner à une autre disposition un raccourci déjà pris (celui de « Travail », ou `Ctrl+Alt+H` du masquage) : refusé avec « Déjà utilisé par… ».
- [ ] « Retirer » le raccourci : il ne fait plus rien. Le raccourci reste actif après un redémarrage de Tidy.

## Changement de résolution ou d'écrans
Paramètres Windows → Affichage → Résolution (ou mise à l'échelle), ou brancher / débrancher un écran.
- [ ] **Mémoriser par configuration** (défaut) : placer des boîtes avec l'écran externe, le débrancher, les placer autrement, le rebrancher : la disposition avec l'écran externe revient.
- [ ] **Redimensionner** : passer d'une résolution à une plus petite : les boîtes gardent leurs proportions (une boîte au milieu de l'écran reste au milieu, plus petite).
- [ ] **Ne rien faire** : changer de résolution : les boîtes ne bougent pas ; débrancher l'écran où se trouve une boîte : elle revient sur un écran branché.
- [ ] **Oublier les autres configurations** → confirmer : message « N boîte(s) mise(s) à jour » ; débrancher puis rebrancher l'écran : les boîtes gardent les places d'aujourd'hui.

## Robustesse
- [ ] Quitter Tidy pendant qu'il prend un instantané (juste après un déplacement), relancer : la page s'affiche, rien d'illisible.
- [ ] Remplacer `snapshots\index.json` par du texte invalide, relancer : Tidy démarre, l'ancien index est mis de côté en `index.json.bad`, l'historique repart de zéro.
