# Tests manuels

Ce qui touche au vrai bureau ne se teste pas automatiquement.

## Windows (cible prioritaire)

### Récupérer l'exe
Onglet **Actions** du dépôt GitHub → dernier run « build » vert → artefact **tidy-windows** → `tidy.exe` (aucune installation). Exe non signé : SmartScreen affiche « Windows a protégé votre ordinateur » → *Informations complémentaires* → *Exécuter quand même*.

Pour essayer sans toucher à tes vrais réglages, dans PowerShell :

```powershell
$env:TIDY_DATA_DIR = "$env:TEMP\tidy-essai"; .\tidy.exe
```

Journaux : `<données>\logs\` (Options > Diagnostic > Ouvrir les journaux). Plus de détails : `$env:TIDY_LOG = "debug"`.

### Vague 0 bis : prototype Windows
**Lancement**
- [ ] L'icône Tidy apparaît dans la zone de notification ; aucune entrée dans la barre des tâches ni dans Alt+Tab pour les boîtes.
- [ ] La fenêtre principale s'ouvre ; relancer `tidy.exe` la ramène devant au lieu d'en ouvrir une deuxième.

**Boîtes posées sur le bureau** (le point le plus important)
- [ ] La « Boîte de démonstration » est en haut à droite de l'écran principal, **sous** les fenêtres des applis, **au-dessus** du fond d'écran et des icônes.
- [ ] Cliquer dans une boîte ne la fait **pas** passer devant les applis ouvertes.
- [ ] **Win+D** (Afficher le bureau) : les boîtes **restent visibles** ; Win+D à nouveau : les applis reviennent, les boîtes restent dessous.
- [ ] Clic sur le coin en bas à droite de la barre des tâches (Afficher le bureau) : même résultat.
- [ ] Bureaux virtuels (Win+Ctrl+→) : noter si les boîtes suivent ou non.
- [ ] Si Win+D cache les boîtes : quitter Tidy, relancer avec `$env:TIDY_WIN_DESKTOP = "bottom"` et refaire l'essai (variante sans propriétaire), puis me dire laquelle marche.

**Déplacer, redimensionner, créer, supprimer**
- [ ] La boîte se déplace par sa barre de titre, se redimensionne par ses bords et ses coins.
- [ ] Après un déplacement, quitter puis relancer Tidy : la boîte revient au même endroit.
- [ ] Clic droit sur la barre de titre → « Nouvelle boîte » : une boîte vide apparaît sous la souris.
- [ ] Clic droit sur le bureau → *Afficher plus d'options* (Windows 11) → **« Créer une boîte (Tidy) »** : une boîte apparaît sous la souris.
- [ ] Clic droit sur une boîte → « Supprimer la boîte » : elle disparaît (aucun fichier touché).

**Masquer le bureau**
- [ ] **Double-clic sur une zone vide du bureau** : les boîtes **et** les icônes de Windows disparaissent ; second double-clic : tout revient.
- [ ] Double-clic sur une **icône** : l'élément s'ouvre normalement, rien ne se masque.
- [ ] **Ctrl+Alt+H** : même bascule que le double-clic.
- [ ] Icônes masquées, puis « Quitter Tidy » : les icônes reviennent.

**Icônes rangées (piste 1, ADR 012)**
- [ ] `.\tidy.exe --desktop-icons` : liste chaque icône du bureau avec son chemin complet et sa position.
- [ ] Tidy lancé, `.\tidy.exe --hide-icon "<nom exact d'un fichier du bureau>"` (avec l'extension) : l'icône disparaît du bureau ; le fichier est intact dans l'Explorateur.
- [ ] `.\tidy.exe --restore-desktop` : l'icône revient **exactement à sa place**.
- [ ] Même essai avec « Organiser automatiquement les icônes » coché (clic droit sur le bureau > Affichage) : l'icône disparaît, et l'option est recochée après `--restore-desktop`.
- [ ] Masquer une icône, puis « Quitter Tidy » : elle revient.
- [ ] Masquer une icône, puis redémarrer l'Explorateur (Gestionnaire des tâches > Explorateur Windows > Redémarrer) : noter si elle réapparaît ; relancer Tidy puis quitter : elle doit être à sa place.

**Écrans**
- [ ] Deux écrans à échelles différentes (ex. 100 % et 150 %) : une boîte déplacée sur le second garde une taille correcte et revient au bon endroit après redémarrage.
- [ ] Débrancher le second écran puis le rebrancher : la boîte retrouve sa place sur lui.
- [ ] Options > Diagnostic : la liste des écrans est juste (nom, taille, échelle).

**Robustesse**
- [ ] Gestionnaire des tâches → terminer le processus `tidy.exe` qui porte les boîtes (le plus gros) : les boîtes reviennent en moins d'une seconde.
- [ ] Terminer l'agent (le plus petit `tidy.exe`) : les boîtes disparaissent et les icônes de Windows sont visibles.
- [ ] `tidy.exe --uninstall` : l'entrée du menu du bureau disparaît, le démarrage avec la session est retiré.

**Mesures**
- [ ] Gestionnaire des tâches, colonne Mémoire : agent et bureau avec 1 puis 5 boîtes.
- [ ] Processeur au repos ≈ 0 %.

### Vague 1 : les vraies boîtes
Essaie d'abord sur un **faux bureau** : crée un dossier `C:\TidyEssai` avec quelques fichiers (pdf, images, un raccourci de jeu Steam, un dossier), puis :

```powershell
$env:TIDY_DATA_DIR = "$env:TEMP\tidy-essai"; $env:TIDY_DESKTOP_DIR = "C:\TidyEssai"; .\tidy.exe
```

Les boîtes montrent alors ce dossier (les icônes de l'Explorateur ne sont pas concernées : elles ne viennent que du vrai bureau).

**Assistant et rangement**
- [ ] Premier lancement : l'assistant s'ouvre ; « Démarrage rapide » crée « Programmes », « Dossiers », « Fichiers et documents » et range tout.
- [ ] Les raccourcis de jeux Steam/Epic vont dans « Programmes » ; les raccourcis vers des programmes aussi.
- [ ] Copier un nouveau fichier dans le dossier : il est rangé en quelques secondes. Le renommer dans l'Explorateur : il reste dans sa boîte.
- [ ] Rangement > « Tout annuler » : on retrouve l'état d'avant.
- [ ] Une catégorie sans boîte cible : le nouveau fichier apparaît dans le bandeau « non affectées » ; « Appliquer les règles » le range.

**Dans une boîte**
- [ ] Les icônes sont les vraies icônes de Windows (miniatures des images).
- [ ] Double-clic ouvre le fichier ; clic droit donne le **menu de l'Explorateur** (Ouvrir avec, Envoyer vers, Propriétés…).
- [ ] Renommer (F2), Mettre à la corbeille (Suppr) : le fichier part bien à la corbeille.
- [ ] Glisser un fichier d'une boîte vers une autre boîte : il change de boîte ; vers un dossier de l'Explorateur : l'Explorateur le déplace ou le copie comme d'habitude.
- [ ] Menu ☰ : trier, grille/liste, taille des icônes, enrouler, onglets (nouvel onglet, fusionner, détacher), nouvelle boîte, supprimer.
- [ ] Enroulement automatique : la boîte s'enroule quand la souris s'en va, se déroule au survol.
- [ ] Déplacer une boîte près d'une autre : elle s'aligne (Maj enfoncée : pas d'aimantation).
- [ ] Portail de dossier : affiche un dossier en direct, navigation dans les sous-dossiers.

**Sur le vrai bureau** (quand tout ce qui précède va bien)
- [ ] Les icônes rangées disparaissent du bureau de Windows ; « Quitter Tidy » les remet toutes à leur place.
- [ ] Tracer un rectangle sur le bureau vide : un bouton « Créer une boîte » apparaît.
- [ ] Accès rapide (Alt+` maintenu) : les boîtes passent devant les applis, puis redescendent.

**Fenêtre principale**
- [ ] Pages Boîtes (apparence, aperçu), Rangement (extensions, boîte cible), Actions rapides ; saisie d'un raccourci et refus d'un doublon.
- [ ] Menu ☰ d'une boîte > « Apparence… » : la fenêtre principale s'ouvre (ou revient) sur la page Boîtes, sans en ouvrir une deuxième.

### Correctifs du 26/11 (après le premier essai)
**Icônes rangées** (sur le vrai bureau)
- [ ] Après le démarrage rapide, les icônes rangées dans les boîtes **disparaissent** du bureau de Windows (plus de doublons).
- [ ] « Aligner les icônes sur la grille » coché (clic droit sur le bureau > Affichage) : même résultat ; l'option est recochée en quittant Tidy.
- [ ] « Quitter Tidy » : toutes les icônes reviennent à leur place.
- [ ] Si des icônes restent visibles : envoyer les lignes « icônes rangées » du journal (Options > Diagnostic > Ouvrir les journaux), et la sortie de `.\tidy.exe --hide-icon "<nom exact>"` (elle dit si l'icône est « absente du bureau de l'Explorateur » ou « restée à l'écran »).

**Raccourcis**
- [ ] Les raccourcis posés par les installateurs (Chrome, Steam, Office… souvent dans `C:\Users\Public\Desktop`) sont rangés dans « Programmes », et leur icône disparaît du bureau.
- [ ] Ils s'ouvrent par double-clic depuis la boîte.

**Grille libre** (ordre manuel, vue en grille : réglages par défaut)
- [ ] Glisser une icône sur une case vide de la boîte, même loin des autres : elle y reste et laisse un trou à sa place d'origine.
- [ ] Glisser une icône sur une case occupée : elle va sur la case libre la plus proche.
- [ ] Glisser plusieurs icônes sélectionnées : celle qu'on tient va sur la case visée, les autres suivent.
- [ ] Glisser une icône depuis une autre boîte sur une case précise : elle s'y pose.
- [ ] Quitter puis relancer Tidy : la disposition est la même.
- [ ] Rétrécir la boîte puis l'élargir : les icônes reprennent leurs cases.
- [ ] Menu ☰ > Trier > « Supprimer les cases vides » : les icônes se suivent à nouveau. Trier par nom ignore les cases ; revenir à « Ordre manuel » les retrouve.

### Vague 2 et fonds d'écran (27/11)
Une liste par partie, à suivre dans cet ordre :
1. [Boîtes : menu, recherche, gel, règles, verre](tests-vague2/M-boites.md)
2. [Dispositions et instantanés](tests-vague2/E-dispositions.md)
3. [Boîte privée](tests-vague2/F-coffre.md)
4. [Recherche rapide](tests-vague2/G-recherche.md)
5. [Widgets : horloge, notes, pomodoro, outils](tests-vague2/H-widgets.md)
6. [Widgets : météo, calendrier, chat IA](tests-vague2/I-widgets-reseau.md)
7. [Fonds d'écran](tests-vague2/J-fonds.md)

## macOS

Avec des données jetables :

```sh
export TIDY_DATA_DIR=$(mktemp -d)
cargo build && target/debug/tidy
```

### Vague 0 bis
- [ ] L'icône Tidy apparaît dans la barre des menus, pas dans le Dock ni dans ⌘Tab.
- [ ] La boîte de démonstration est sous les fenêtres des applis, au-dessus du fond d'écran ; présente sur chaque Space ; immobile pendant Mission Control et « Afficher le bureau ».
- [ ] Déplacer et redimensionner la boîte ; la position survit à un redémarrage ; elle est mémorisée par configuration d'écrans.
- [ ] Clic droit sur le titre → « Nouvelle boîte » / « Supprimer la boîte ».
- [ ] ⌃⌥H et `tidy --toggle-desktop` masquent puis montrent les boîtes (les icônes du Finder restent : pas de masquage rapide sous macOS).
- [ ] `kill -9` du processus `tidy --desktop` : il revient ; trois fois en une minute : il ne revient plus.
- [ ] `kill -9` de l'agent : le bureau disparaît aussi.

Vérifié sur la machine de développement (24/11/2026) : niveaux des fenêtres, création par `--create-box`, masquer/montrer, retour après plantage, 0 % de processeur au repos, 52 Mo (debug) pour le bureau avec une boîte, 58 Mo avec deux.
