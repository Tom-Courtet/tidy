# Tests manuels · lot M (boîtes : ce qui manquait par rapport à iTop)

Exe et essai sans toucher aux vraies données : voir `docs/manual-tests.md`
(`$env:TIDY_DATA_DIR = "$env:TEMP\tidy-essai"`). Journaux détaillés :
`$env:TIDY_LOG = "debug"`.

Préparer deux boîtes ordinaires avec quelques éléments, dont un nom accentué
(« Été 2026.jpg », « Facture électricité.pdf »), et un portail de dossier.

## Windows (cible prioritaire)

### Menu ☰
- [ ] Le menu ☰ (ou clic droit sur la barre de titre) montre, dans l'ordre : « Créer une boîte ▸ », « Affichage ▸ », « Trier par ▸ », « Règles ▸ », Enrouler, Enroulement automatique, les onglets, « Apparence… », « Réglages… », « Actualiser les icônes », « Renommer la boîte », « Geler la boîte », « Supprimer la boîte ».
- [ ] Le menu tient à l'écran, même ouvert près du bas de l'écran.
- [ ] Sur un portail : pas de « Règles ▸ ».
- [ ] « Créer une boîte ▸ Boîte » : une boîte vide apparaît à côté.
- [ ] « Créer une boîte ▸ Portail de dossier… » : le choix d'un dossier s'ouvre, le portail apparaît à côté.

### Recherche dans la barre de titre
- [ ] « Affichage ▸ Recherche dans la barre de titre » : une loupe apparaît à droite de la barre de titre (l'option est cochée).
- [ ] Clic sur la loupe : un champ « Rechercher… » remplace le nom ; taper « ete » ne garde que « Été 2026.jpg » (casse et accents ignorés).
- [ ] Un texte sans résultat affiche « Aucun élément ne correspond. ».
- [ ] **Échap** vide le champ et le ferme : tous les éléments reviennent.
- [ ] Ctrl+F dans la boîte (après un clic dedans) ouvre le champ.
- [ ] En **grille libre** (tri manuel, éléments placés avec des cases vides) : le filtre cache les autres éléments **sans déplacer** ceux qui restent ; après Échap, chaque élément est à sa case d'origine.
- [ ] En **liste** et en grille triée (par nom) : les éléments trouvés se suivent.
- [ ] Dans un **portail** : la recherche filtre le dossier affiché.
- [ ] Ctrl+A pendant une recherche ne sélectionne que les éléments affichés.
- [ ] Une boîte avec une recherche en cours ne s'enroule pas toute seule ; l'enrouler à la main ferme la recherche.
- [ ] Décocher l'option : la loupe et le filtre disparaissent.

### Geler la boîte
- [ ] « Geler la boîte » : un petit cadenas apparaît dans la barre de titre (info-bulle « Boîte gelée… »).
- [ ] Gelée : la barre de titre ne déplace plus la boîte ; les bords et les coins ne la redimensionnent plus (pas de curseur de redimensionnement).
- [ ] Gelée, titre « Jamais » (apparence) : glisser le fond ne déplace pas la boîte.
- [ ] Gelée : ouvrir, glisser des éléments, enrouler (double-clic sur le titre) marchent toujours.
- [ ] Dégeler : tout redevient possible.

### Exclusions
- [ ] « Affichage ▸ Exclure du masquage rapide » coché : double-clic sur le bureau vide, cette boîte reste visible, les autres disparaissent.
- [ ] « Affichage ▸ Exclure de l'accès rapide » coché : avec le raccourci d'accès rapide, cette boîte reste sous les applis ; cocher/décocher pendant l'accès rapide la fait suivre aussitôt.

### Actualiser les icônes
- [ ] Changer l'icône d'un raccourci (Propriétés > Changer d'icône) : « Actualiser les icônes » affiche la nouvelle icône et le message « Icônes actualisées. ».

### Règles
- [ ] « Règles ▸ Appliquer les règles aux non affectées » : un message confirme, les éléments non affectés sont rangés selon la page Rangement.
- [ ] « Règles ▸ Ranger ici… » : la liste des catégories (Applications, Dossiers, Documents, Images…) et « Tout le reste (fourre-tout) » ; celles qui vont dans cette boîte sont cochées.
- [ ] Cocher « Images » : la fenêtre principale (page Rangement) montre cette boîte comme cible des images ; si une autre boîte l'était, elle ne l'est plus.
- [ ] Décocher : la catégorie n'a plus de boîte (extensions personnalisées gardées).
- [ ] « Règles ▸ Règles… » : la fenêtre principale s'ouvre (ou revient devant) sur la page Rangement.

### Réglages… et Apparence…
- [ ] « Réglages… » : la fenêtre principale s'ouvre sur la page Boîtes, **cette boîte dépliée** (aussi si la fenêtre était déjà ouverte sur une autre page).
- [ ] « Apparence… » : même chose.
- [ ] Dans la page Boîtes, les interrupteurs « Recherche dans la barre de titre » et « Geler la boîte » suivent le menu (et inversement).

### Verre dépoli
- [ ] Page Boîtes > apparence propre > « Effet verre » : le fond de la boîte devient flou et plus transparent (on devine le fond d'écran derrière).
- [ ] Windows 11 : l'effet **reste** quand on clique dans une autre appli (boîte non active). S'il devient un fond uni gris : relancer avec `$env:TIDY_GLASS = "blur"` et me dire si c'est mieux.
- [ ] Windows 10 : l'effet est visible (acrylique ou flou simple) ; noter si le déplacement de la boîte devient saccadé.
- [ ] Coins : arrondis par le système sous Windows 11 ; sous Windows 10, le flou peut dépasser des coins arrondis de la boîte (limite connue).
- [ ] Retirer « Effet verre » : la boîte retrouve son fond ordinaire sans redémarrer.
- [ ] Boîte enroulée puis déroulée, redimensionnée : l'effet couvre toujours toute la boîte.

### Après redémarrage
- [ ] Quitter Tidy (icône de la zone de notification > Quitter) puis relancer : recherche dans le titre, gel, exclusions et verre de chaque boîte sont conservés.

## macOS
- [ ] Mêmes essais du menu, de la recherche, du gel, des règles et des réglages.
- [ ] Verre : la boîte montre le flou sombre de macOS derrière son contenu, coins arrondis comme la boîte.
- [ ] **Tracer un rectangle sur le bureau vide** (glisser sur le fond d'écran, hors de toute fenêtre) : la proposition « Créer une boîte » apparaît ; l'accepter crée la boîte sur ce rectangle.
- [ ] La sélection d'icônes du Finder par rectangle marche comme avant (Tidy n'intercepte rien).
- [ ] Glisser une **icône** du Finder sur le bureau ne propose **pas** de boîte.
- [ ] Tracer un rectangle dans une fenêtre d'appli (Finder, navigateur) ne propose rien.
- [ ] Aucun dialogue d'autorisation (Accessibilité, Surveillance de l'entrée) n'apparaît.
