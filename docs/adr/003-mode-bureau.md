# ADR 003 · Mode du bureau : superposé par défaut, géré avec accord

Statut : Acceptée ; forme des fenêtres tranchée par l'ADR 011 (24/11/2026)

## Décision

Deux modes :
- **superposé** (défaut) : les boîtes flottent au-dessus des icônes du Finder ; la surface ne prend les clics que sur les boîtes (`ignoresMouseEvents` basculé par des moniteurs d'événements, sans minuteur) ;
- **géré** : après accord explicite (modale), Tidy masque les icônes du Finder (`CreateDesktop`) et dessine tout le bureau.

Sécurité du Finder : un fichier témoin (`finder-hidden.json`) garde la valeur d'origine du réglage. L'agent rend les icônes en quittant, hors du mode géré, après trois plantages du bureau en une minute, et au démarrage s'il reste un témoin. Le bureau rend les icônes si l'agent disparaît. Secours : `tidy --restore-finder`.

**Mesure de la vague 0 (build release, écran Retina 1470×956 pt)** : une surface plein écran coûte 161 à 178 Mo (dont 107 Mo de tampons IOSurface). Une fenêtre de la taille d'une boîte (340×240 pt) coûte 51 Mo pour tout le processus (6,6 Mo d'IOSurface).

## Conséquences

- Tranché : **une fenêtre par boîte et par widget** (ADR 011). La surface plein écran n'est plus utilisée qu'en surcouche temporaire.
- Reste à faire : retour du réglage du Finder à la fermeture de session (SIGTERM) si Tidy ne démarre pas avec la session.
