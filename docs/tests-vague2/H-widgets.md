# Tests manuels Windows · lot H (widgets)

À faire sur le PC Windows 10/11, avec `tidy.exe` de la CI. Les widgets s'ajoutent depuis la fenêtre principale, page **Widgets**.

## Cadre commun et fenêtres
- [ ] Ajouter une **Horloge** : elle apparaît à une place libre de l'écran principal (en haut à gauche), sans chevaucher une boîte.
- [ ] Le widget est **sous les fenêtres des applis** (ouvrir le Bloc-notes par-dessus : il le cache) et **au-dessus des icônes** du bureau.
- [ ] **Win+D** : le widget reste affiché.
- [ ] Il n'apparaît ni dans la barre des tâches ni dans Alt+Tab.
- [ ] Survol : la barre apparaît (poignée à gauche ; ⚙, épingle, cadenas, croix à droite). Sans survol, elle disparaît.
- [ ] Glisser le fond ou la barre déplace le widget ; quitter puis relancer Tidy : il revient au même endroit.
- [ ] Multi-écrans : déplacer un widget sur le 2e écran, débrancher l'écran puis le rebrancher : le widget reprend sa place sur cet écran.
- [ ] Écrans d'échelles différentes (100 % et 150 %) : le widget garde sa taille apparente en changeant d'écran.
- [ ] **Geler** (cadenas) : le widget ne se déplace plus ; la poignée devient un cadenas. Dégeler : il bouge de nouveau.
- [ ] **Épingler** : le widget passe au-dessus des applis (bordure violette). Désépingler : il repasse sous les applis. L'état survit à un redémarrage de Tidy.
- [ ] **Double-clic sur le bureau** (ou Ctrl+Alt+H) : les widgets se masquent avec les boîtes, puis reviennent.
- [ ] Réglages ⚙ → « Ne pas masquer avec le bureau » : ce widget reste affiché quand le bureau est masqué.
- [ ] Page Widgets → « Masquer les widgets avec le bureau » désactivé : aucun widget ne se masque.
- [ ] Croix → confirmation dans une petite fenêtre ; « Annuler » ne retire rien, « Supprimer » retire le widget.
- [ ] Clic droit sur le fond d'un widget : ouvre ses réglages ; Échap les ferme.
- [ ] Au repos (aucun pomodoro en marche), le processus `tidy.exe --desktop` reste à ~0 % de processeur dans le Gestionnaire des tâches.

## Horloge
- [ ] Numérique, puis analogique ; petite, moyenne, grande : la fenêtre s'ajuste toute seule.
- [ ] Ajouter « Tokyo » et « New York » dans les réglages : trois horloges, avec les bonnes heures (comparer avec l'horloge de Windows).
- [ ] Date affichée ou non ; secondes (la trotteuse avance chaque seconde) ; format 12 h (AM/PM).
- [ ] L'heure change bien à la minute pile (sans les secondes).

## iNotes
- [ ] Page Widgets → « Nouvelle note » : une note jaune apparaît ; y taper plusieurs lignes, avec accents.
- [ ] Fermer Tidy (menu de la zone de notification > Quitter) juste après avoir tapé, le relancer : le texte est là.
- [ ] Changer la couleur dans ⚙ : la note change de couleur ; le texte reste lisible.
- [ ] Redimensionner la note par le coin en bas à droite ; la taille est gardée.
- [ ] Épingler une note au premier plan, puis taper dedans par-dessus une appli.
- [ ] Retirer la note du bureau (croix) : elle reste dans « Gérer les notes », marquée « archivée » ; « Remettre sur le bureau » la ramène avec son texte.
- [ ] « Gérer les notes » : recherche (Ctrl+F puis un mot), regroupement Aujourd'hui / Hier / Cette semaine…
- [ ] « Mettre à la corbeille » (note archivée) : confirmation, puis le fichier est dans la Corbeille de Windows (`%APPDATA%\…\Tidy\data\notes\<id>.json`).

## iPomodoro
- [ ] Régler concentration à 1 min, pause courte à 1 min, 2 périodes avant la pause longue.
- [ ] Démarrer : le compte à rebours avance ; Pause le fige ; « Passer » change de période ; « Recommencer » remet 01:00.
- [ ] Fin de période : **notification Windows** (toast « Tidy ») avec son ; son désactivé dans ⚙ : toast silencieux.
- [ ] Fin de période pendant que le bureau est masqué : la notification arrive quand même à l'heure.
- [ ] « Enchaîner automatiquement » : la pause démarre seule après la concentration.
- [ ] Après 2 concentrations : pause longue, et les points des cycles se remplissent puis se vident.
- [ ] **Mode concentration** : Démarrer masque le bureau (boîtes, icônes, autres widgets) mais le minuteur reste visible ; à la fin de la concentration (ou en pause), le bureau revient.
- [ ] Si le toast n'apparaît pas : noter si un toast « Windows PowerShell » apparaît à la place (repli), et joindre le journal.

## Outils rapides
- [ ] Chaque tuile ouvre son outil : Calculatrice, Bloc-notes, Gestionnaire des tâches, Paramètres, Corbeille, Invite de commandes, Explorateur, Panneau de configuration (à cocher dans ⚙).
- [ ] « Verrouiller la session » verrouille Windows.
- [ ] Capture : avec `snappy.exe` à côté de `tidy.exe` (ou Snappy lancé avec la session), la capture de zone de Snappy démarre ; sans Snappy, un message l'explique ; le chemin réglé à la main dans ⚙ est pris en compte.
- [ ] Aucune fenêtre de console ne clignote en cliquant sur Capture.
- [ ] Colonnes (1 à 8) et noms affichés : la fenêtre s'ajuste.

## Mémoire
- [ ] Gestionnaire des tâches, onglet Détails, colonne « Mémoire (jeu de travail privé) » de `tidy.exe --desktop` : relever la valeur avec 0 widget, puis avec 4 (horloge, note, pomodoro, outils).
