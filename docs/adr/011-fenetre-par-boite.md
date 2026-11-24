# ADR 011 · Une fenêtre par boîte et par widget

Statut : Acceptée (24/11/2026) ; tranche la question laissée ouverte par l'ADR 003

## Contexte

Le prototype de la vague 0 dessinait tout le bureau sur une surface plein écran par écran. Mesure (release, écran Retina 1470×956 pt) : **161 à 178 Mo** pour le processus du bureau, dont 107 Mo de tampons de fenêtre ; une fenêtre de la taille d'une boîte coûte **51 Mo** pour tout le processus.

L'original (manuel officiel, bilan local) fonctionne ainsi :
- chaque boîte est un panneau séparé posé sur le bureau ; sa position et sa taille sont mémorisées par boîte et par écran (`BoxConfig.ini`) ;
- les icônes hors boîte restent celles de Windows (« Personnaliser » les aligne sur la grille du bureau) ;
- on crée une boîte en dessinant un rectangle sur le bureau vide, par le menu contextuel du bureau, ou en sélectionnant des icônes puis « Créer une boîte pour ces icônes » ;
- on fusionne deux boîtes en onglets en glissant l'une sur l'autre.

## Décision

- **Chaque boîte, portail, widget et note est une fenêtre à sa taille** (viewport egui), posée au niveau du bureau. Coût : une base d'environ 50 Mo pour le processus du bureau, puis quelques Mo par boîte.
- **Les icônes hors boîte restent celles du système** (Explorateur sous Windows, Finder sous macOS). Les icônes rangées dans une boîte sont masquées du bureau système (Windows : à prototyper en vague 0 bis, sans injection de DLL ; macOS : voir plus bas).
- **Opérations qui traversent plusieurs boîtes** (dessiner une boîte, glisser un élément d'une boîte à l'autre, aimantation) : une surcouche transparente plein écran, ouverte le temps de l'opération puis fermée (technique de Snappy), pour ne pas payer sa mémoire en permanence.
- **Masquer le bureau** (double-clic) : masque les fenêtres de Tidy et les icônes du système.

## Conséquences

- Le prototype de la vague 0 (surface plein écran) est remplacé en vague 0 bis.
- macOS : le mode « géré » (icônes du Finder masquées, Tidy dessine tout) n'a plus de raison d'être par défaut. Le mode superposé devient la référence ; masquer les icônes du Finder qui sont dans des boîtes n'a pas d'équivalent sûr sur macOS (à étudier, sinon on garde des doublons visibles derrière les boîtes, ou le mode géré avec une fenêtre par icône hors boîte).
- Chaque fenêtre de boîte doit résister à « Afficher le bureau » (Win+D) et rester sous les applis : point principal du prototype Windows.
