# Tests manuels Windows · lot I (widgets Météo, Calendrier, Chat IA)

Essais avec des données jetables, dans PowerShell :

```powershell
$env:TIDY_DATA_DIR = "$env:TEMP\tidy-essai"; $env:TIDY_LOG = "debug"; .\tidy.exe
```

Les widgets se posent depuis la page Widgets de la fenêtre principale (lot H). Leurs
réglages s'ouvrent depuis le cadre du widget. Journaux : `<données>\logs\`.

Avant l'hôte du lot H, on peut les voir dans l'aperçu (sur un Mac ou un PC avec Rust) :
`cargo run -p tidy-widgets --example apercu` (variables `APERCU_VILLE=Lyon`,
`APERCU_ICS=<adresse ICS>`).

## Météo
- [ ] Widget posé sans ville : message « Ajoute une ville dans les réglages du widget » ; aucune connexion réseau tant qu'aucune ville n'est ajoutée (pare-feu ou Moniteur de ressources › Réseau : pas de `tidy.exe` vers open-meteo.com).
- [ ] Réglages › chercher « Lyon » : la liste montre Lyon (Auvergne-Rhône-Alpes, France) et les homonymes des États-Unis ; un clic l'ajoute et l'affiche.
- [ ] Le temps actuel s'affiche : pictogramme, température, texte (« Nuageux »…), ressenti, humidité, vent.
- [ ] Le bandeau des prochaines heures défile horizontalement ; le pictogramme de nuit (croissant) apparaît après le coucher du soleil.
- [ ] Les jours suivants montrent min, barre, max ; le nombre de jours suit le réglage (1 à 7).
- [ ] Ajouter une deuxième ville (Montréal) : les flèches ‹ › passent de l'une à l'autre ; la ville affichée est gardée après redémarrage de Tidy.
- [ ] Passer en °F : températures en °F et vent en mph.
- [ ] Couper le Wi-Fi puis cliquer « Actualiser » : message « Pas de connexion… » en pied de widget, les dernières données restent affichées. Rebrancher : retour normal au plus tard 5 minutes après (ou tout de suite avec « Actualiser »).
- [ ] Laisser tourner une heure : l'heure du relevé en pied de widget avance (actualisation toutes les 30 minutes), processeur à 0 % entre deux.

## Calendrier
- [ ] Widget posé sans agenda : message expliquant d'ajouter une adresse ICS ; aucune connexion réseau.
- [ ] Google Agenda : Paramètres › ton agenda › « Adresse secrète au format iCal » ; la coller dans Réglages › Ajouter un agenda. L'agenda se charge, ses événements apparaissent.
- [ ] Outlook (outlook.com ou Microsoft 365) : Paramètres › Calendrier › Calendriers partagés › Publier un calendrier › lien ICS ; même essai. Vérifier les heures des réunions (fuseaux Windows du type « Romance Standard Time »).
- [ ] Une adresse `webcal://…` est acceptée ; une adresse `file://` ou du texte quelconque est refusée avec un message.
- [ ] Une adresse publique qui rend une page de connexion au lieu d'un agenda : message « Cette adresse ne donne pas un agenda ICS… ».
- [ ] Les réglages ne montrent que l'hôte de l'adresse (calendar.google.com), jamais l'adresse complète.
- [ ] Vue Mois : aujourd'hui en lime, pastilles de couleur sous les jours avec événements, clic sur un jour → liste de ses événements ; ‹ › changent de mois ; « Aujourd'hui » revient au mois courant.
- [ ] Vue Agenda : prochains jours groupés (« Aujourd'hui », « Demain », « ven. 27 novembre »…), nombre de jours réglable.
- [ ] Deux agendas de couleurs différentes : les pastilles et barres prennent la couleur de chaque agenda ; désactiver un agenda le retire de l'affichage.
- [ ] Événements récurrents : un rendez-vous hebdomadaire reste à la même heure avant et après le changement d'heure (fin mars, fin octobre) ; une occurrence supprimée dans Google n'apparaît pas ; une occurrence déplacée apparaît à sa nouvelle heure.
- [ ] Événement sur plusieurs jours : visible chaque jour, pas le lendemain de la fin.
- [ ] Rappel (10 minutes par défaut) : créer un événement dans 12 minutes, attendre la mise à jour de l'agenda (au plus 15 minutes, ou retirer et remettre l'agenda) : une notification Windows « Dans 10 min, à HH:MM » arrive une seule fois. Rappel « Aucun » : pas de notification.
- [ ] Wi-Fi coupé : bandeau d'erreur par agenda avec « Réessayer », événements déjà chargés gardés.

## Chat IA
- [ ] Widget posé sans clé : explication (où créer une clé sur console.anthropic.com) et champ pour la coller ; aucune connexion à api.anthropic.com.
- [ ] Coller une clé `sk-ant-…` et « Enregistrer » : « Clé enregistrée ». Dans le Gestionnaire d'identifiants de Windows (Panneau de configuration › Gestionnaire d'identifiants › Informations d'identification Windows), une entrée `anthropic-api-key.Tidy` existe.
- [ ] La clé n'apparaît ni dans `<données>\desktop.json` / `config.json` ni dans les journaux (chercher `sk-ant` dans le dossier des données).
- [ ] Texte qui n'est pas une clé : message « Ce n'est pas une clé de l'API Claude ».
- [ ] Question simple : la réponse s'affiche au fil de l'eau ; l'interface reste fluide pendant le flux.
- [ ] Entrée envoie, Maj+Entrée va à la ligne.
- [ ] Invites : « Traduire » puis un texte français → traduction anglaise ; « Résumer », « Rédiger », « Expliquer le code » donnent le résultat attendu ; l'invite choisie apparaît au-dessus du message envoyé.
- [ ] Une deuxième question qui renvoie à la première (« et en une phrase ? ») : Claude se souvient de l'échange.
- [ ] « Copier » sous une réponse : le texte est dans le presse-papiers (coller dans le Bloc-notes).
- [ ] « Arrêter » pendant une longue réponse : le flux s'arrête, « Réponse arrêtée » s'affiche, le texte reçu reste.
- [ ] Corbeille (« Effacer la conversation ») : la conversation disparaît.
- [ ] Clé erronée (une vraie forme `sk-ant-…` mais révoquée) : « Clé de l'API refusée… ».
- [ ] Modèle inconnu dans les réglages (`claude-inconnu`) : « Modèle inconnu… » ; revenir au modèle vide utilise `claude-sonnet-5-5`.
- [ ] Wi-Fi coupé puis envoi : message d'erreur réseau, pas de blocage.
- [ ] Réglages › « Supprimer la clé » : l'entrée disparaît du Gestionnaire d'identifiants, le widget revient à l'écran sans clé.
- [ ] Redémarrer Tidy : la conversation n'est pas gardée (voulu), la clé l'est.
