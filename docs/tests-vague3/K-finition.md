# Tests manuels Windows · lot K (finition)

L'installation touche le vrai système (dossier `%LOCALAPPDATA%\Programs\Tidy`, menu
Démarrer, registre de l'utilisateur) : c'est son but. Les données, elles, restent celles
de Tidy (`%APPDATA%\Tidy\data`) ; pour un essai à part, lancer chaque commande avec
`$env:TIDY_DATA_DIR = "$env:TEMP\tidy-k"` (même valeur partout, sinon l'exe installé ne
voit pas l'agent).

Préparer : `tidy.exe` de la CI dans Téléchargements, une boîte avec quelques icônes
rangées, un widget Pomodoro (pour l'identifiant des notifications), une boîte privée
avec un fichier, un fond d'écran posé par Tidy, « Lancer Tidy avec Windows » activé.
Journaux : `<données>\logs\cli.log.*` (installation, désinstallation).

## Installation
- [ ] Lancer `tidy.exe` depuis Téléchargements : l'Accueil montre la carte « Installation » (« Tidy tourne depuis l'endroit où tu l'as téléchargé… »). Options > Installation aussi.
- [ ] Nouveau dossier de données (premier lancement) : l'assistant propose l'installation à l'étape 2.
- [ ] « Installer Tidy » : Tidy s'arrête (boîtes et icône disparaissent), puis une boîte « Tidy est installé dans C:\Users\…\AppData\Local\Programs\Tidy » s'ouvre et Tidy redémarre (fenêtre principale ouverte).
- [ ] `%LOCALAPPDATA%\Programs\Tidy` contient `tidy.exe` et `tidy.ico` ; l'exe de Téléchargements est toujours là.
- [ ] Menu Démarrer : « Tidy » avec le logo ; il lance Tidy (ou ouvre la fenêtre principale s'il tourne).
- [ ] Paramètres > Applications > Applications installées (ou Applications et fonctionnalités) : « Tidy », version 0.1.0, éditeur « Tidy », icône, taille, date du jour.
- [ ] `reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v Tidy` : pointe vers `…\Programs\Tidy\tidy.exe --background`.
- [ ] Clic droit sur le bureau > (Afficher plus d'options) > « Créer une boîte » : marche ; `reg query "HKCU\Software\Classes\DesktopBackground\Shell\TidyCreateBox\command"` pointe vers la copie installée.
- [ ] Options > Installation : « Tidy 0.1.0 est installé dans … » ; plus de carte sur l'Accueil.
- [ ] Redémarrer la session : Tidy démarre seul, depuis la copie installée (Gestionnaire des tâches > Détails > colonne « Ligne de commande »).
- [ ] En ligne de commande : `.\tidy.exe --install` depuis Téléchargements donne le même résultat (boîte de message à la fin).

## Mise à jour
- [ ] Tidy installé en marche, double-cliquer sur un `tidy.exe` plus récent (ou le même) dans Téléchargements : une fenêtre s'ouvre sur Options, carte « Installation » avec « Mettre à jour » (ou « Remplacer la copie installée »).
- [ ] Cliquer : Tidy s'arrête, la copie installée est remplacée (date de modification de `tidy.exe`), Tidy redémarre ; boîtes, widgets, réglages intacts.
- [ ] Boîte privée **déverrouillée** avec un fichier ouvert puis modifié, puis mise à jour : la boîte privée est verrouillée au redémarrage et la modification est gardée (le fichier est rechiffré).
- [ ] Une copie installée plus récente : `tidy.exe --install` depuis un exe plus ancien refuse (« Une version plus récente… ») ; `--install --force` remplace.
- [ ] Tidy qui ne s'arrête pas (cas rare) : message « Tidy tourne encore… », rien n'est remplacé.

## Désinstallation
- [ ] Applications installées > Tidy > Désinstaller (Windows demande confirmation) : une boîte « Désinstallation de Tidy » liste ce qui a été fait et le dossier des données, qui reste.
- [ ] Plus aucun processus `tidy.exe` (Gestionnaire des tâches).
- [ ] Les icônes rangées sont revenues à leur place sur le bureau ; les icônes du bureau sont visibles (même si elles avaient été masquées par un double-clic).
- [ ] Le fond d'écran d'origine est revenu.
- [ ] La boîte privée n'est pas supprimée : `<données>\vault\` est toujours là, aucun fichier en clair dans `vault\open.noindex\`.
- [ ] Plus d'entrée « Tidy » dans les applications installées, plus de raccourci dans le menu Démarrer, plus de « Créer une boîte » dans le menu du bureau, plus de valeur `Tidy` dans `…\CurrentVersion\Run`.
- [ ] `reg query HKCU\Software\Classes\AppUserModelId\Tidy.Desktop` : introuvable.
- [ ] Quelques secondes après : `%LOCALAPPDATA%\Programs\Tidy` a disparu (vide puis retiré) ; un fichier qu'on y aurait mis soi-même reste, avec le dossier.
- [ ] Options > Désinstaller… (confirmation) depuis la fenêtre principale : même résultat.
- [ ] `tidy.exe --uninstall` lancé depuis Téléchargements alors que Tidy est installé : retire aussi la copie installée ; l'exe de Téléchargements reste.
- [ ] Réinstaller ensuite : les boîtes, la boîte privée (mot de passe inchangé), les dispositions et les réglages sont retrouvés.

## Options complètes
- [ ] Options > Disposition des boîtes : désactiver « Aimanter les boîtes » : une boîte déplacée près d'une autre ne s'y colle plus ; réactiver, régler l'écart à 30 pt : deux boîtes voisines restent à 30 pt.
- [ ] « Taille en nombre d'icônes » : redimensionner une boîte : sa taille s'arrondit à des icônes entières.
- [ ] « Enrouler les boîtes au bord de l'écran » : une boîte collée au bord s'enroule quand la souris s'en va, se déroule au survol.
- [ ] Options > À propos : version, dossier des données, journaux (les boutons ouvrent l'Explorateur), « Licences tierces » se déplie.
- [ ] Langue anglaise : les nouvelles cartes et le menu de l'icône sont traduits.

## Menu de l'icône de notification
- [ ] « Rechercher… » ouvre le lanceur de recherche.
- [ ] « Widgets… » ouvre la fenêtre principale sur la page Widgets.
- [ ] Fonds d'écran > « Fond suivant » : actif seulement quand la playlist est utilisée ; passe au fond suivant. « Gérer les fonds d'écran… » ouvre la page.
- [ ] « Verrouiller la boîte privée » : la boîte privée déverrouillée se verrouille.
- [ ] Dispositions (sous-menu), « Ouvrir Tidy… », « Quitter Tidy » : inchangés.
