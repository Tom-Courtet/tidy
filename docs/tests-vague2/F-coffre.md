# Tests manuels Windows · lot F (boîte privée)

À faire sur le PC Windows 10/11, avec `tidy.exe` de la CI. **Premier passage sur des données d'essai** : dossier de données et bureau factices, puis seulement après sur le vrai bureau.

```bat
set TIDY_DATA_DIR=%TEMP%\tidy-essai
set TIDY_DESKTOP_DIR=%USERPROFILE%\Documents\bureau-essai
mkdir %TIDY_DESKTOP_DIR%
tidy.exe
```

Mettre dans le faux bureau : un petit `.txt`, une photo, un PDF de plus de 50 Mo, un dossier avec des sous-dossiers, un fichier au nom accentué (`Relevé été.pdf`).

## Création

- [ ] Menu ☰ d'une boîte : l'entrée « Boîte privée » est sous « Nouvelle boîte ». Elle ouvre l'assistant (fenêtre au premier plan).
- [ ] Mot de passe de 5 caractères, puis de 21 : refusé avec un message clair. Deux mots de passe différents : refusé.
- [ ] Étape 2 : la clé de récupération (`XXXX-XXXX-XXXX-XXXX-XXXX-XXXX`) s'affiche ; « Créer » reste grisé tant que « Je l'ai notée » n'est pas coché. **Noter la clé.**
- [ ] Après « Créer » : une boîte « Boîte privée » apparaît, déverrouillée, vide (« Glisse ici des éléments du bureau »).
- [ ] Le menu ☰ d'une autre boîte ne propose plus « Boîte privée » ; la page Boîtes de la fenêtre principale montre la carte « Boîte privée » avec « Le coffre existe ».
- [ ] Dans `%TIDY_DATA_DIR%\vault\` : `vault.json`, `index.tv`, `objects\`. Ouvrir `vault.json` : ni le mot de passe ni la clé n'y sont (seulement des sels et des enveloppes en hexadécimal). Le mot de passe n'apparaît ni dans `config.json` ni dans `logs\`.

## Ajouter (ne jamais perdre un fichier)

- [ ] Glisser le `.txt` du bureau (Explorateur) dans la boîte privée : confirmation « Chiffrer cet élément… » expliquant que l'original part à la corbeille. Annuler : rien ne change.
- [ ] Confirmer : l'élément apparaît dans la boîte privée ; l'original n'est plus sur le bureau mais **dans la Corbeille** (le restaurer pour vérifier qu'il est intact, puis le remettre à la corbeille).
- [ ] Glisser un élément d'une **autre boîte** de Tidy vers la boîte privée : même confirmation, même résultat ; l'élément disparaît de l'autre boîte.
- [ ] Glisser le dossier (avec sous-dossiers) et le PDF de 50 Mo : pastille « Chiffrement… » pendant l'opération, puis les deux apparaissent avec leur taille.
- [ ] Glisser un fichier venu d'ailleurs (Documents, clé USB) : message « Seuls les éléments du bureau… » ; le fichier n'a pas bougé.
- [ ] Ajouter un élément portant le même nom qu'un élément du coffre : il devient « nom (2) ».
- [ ] Dans `vault\objects\` : des fichiers aux noms aléatoires ; aucun nom d'origine visible ; ouvrir un `.tv` dans le Bloc-notes : illisible.

## Verrouiller, déverrouiller

- [ ] Menu ☰ de la boîte privée : « Verrouiller ». L'écran de verrouillage s'affiche dans la boîte (cadenas, champ mot de passe, « Déverrouiller », « Mot de passe oublié ? »).
- [ ] Cliquer dans le champ puis taper : le texte arrive bien (masqué). Mauvais mot de passe : « Mot de passe incorrect », le champ se vide. Bon mot de passe (Entrée ou bouton) : contenu de retour en moins de 2 s.
- [ ] Glisser un élément dans la boîte verrouillée : « Déverrouille d'abord la boîte privée », rien ne bouge.
- [ ] Quitter Tidy (zone de notification > Quitter) puis relancer : la boîte privée est verrouillée.

## Ouvrir, modifier, remettre sur le bureau

- [ ] Double-clic sur le `.txt` : il s'ouvre dans le Bloc-notes depuis `%TIDY_DATA_DIR%\vault\open.noindex\…`. Le modifier, enregistrer, fermer le Bloc-notes.
- [ ] Verrouiller : le dossier `open.noindex` est vide. Déverrouiller, rouvrir le `.txt` : la modification est là.
- [ ] Ouvrir un fichier dans Word (ou une appli qui le garde ouvert), puis verrouiller sans fermer l'appli : message « N fichier(s) encore ouvert(s)… ». Fermer l'appli, déverrouiller : la copie a été reprise (les modifications enregistrées entre-temps sont dans le coffre) et `open.noindex` est vide.
- [ ] Double-clic sur le dossier : il s'ouvre dans l'Explorateur (copie déchiffrée). Y ajouter un fichier, verrouiller, déverrouiller, remettre le dossier sur le bureau : le nouveau fichier y est.
- [ ] Clic droit sur un élément de la boîte privée : « Ouvrir », « Renommer », « Remettre sur le bureau », « Mettre à la corbeille ». Renommer (F2 aussi) : nom changé, conservé après verrouillage.
- [ ] « Remettre sur le bureau » sur le PDF : il réapparaît sur le bureau, identique (l'ouvrir), avec sa date de modification d'origine ; il n'est plus dans le coffre. Avec un fichier du même nom déjà sur le bureau : « nom (2) ».
- [ ] « Mettre à la corbeille » sur un élément du coffre : confirmation, puis l'élément est dans la Corbeille (en clair).

## Mot de passe oublié, changement

- [ ] Verrouiller, « Mot de passe oublié ? » : fenêtre avec la clé de récupération et un nouveau mot de passe. Clé fausse : « Clé de récupération incorrecte ». Clé saisie en minuscules avec des espaces : acceptée ; la boîte est déverrouillée ; l'ancien mot de passe ne marche plus, le nouveau oui.
- [ ] Menu ☰ > « Changer le mot de passe… » : ancien faux refusé ; puis changement réussi ; verrouiller et vérifier.

## Verrouillage automatique

- [ ] Page Boîtes, carte « Boîte privée » : verrouillage automatique à 1 min. Déverrouiller, ne plus toucher clavier ni souris : verrouillée au bout d'une minute environ (au plus 5 s de plus), message « verrouillée automatiquement ».
- [ ] En tapant dans une autre appli pendant plus d'une minute : la boîte reste déverrouillée.
- [ ] Win+L puis revenir : la boîte est verrouillée (option « Verrouiller avec la session »). Désactiver l'option : elle reste ouverte.
- [ ] Mise en veille (Démarrer > Marche/Arrêt > Mettre en veille) puis réveil : verrouillée (option « Verrouiller à la mise en veille »).

## Opacité

- [ ] Carte « Boîte privée » : opacité verrouillée à 30 %, puis 0 %. Boîte verrouillée : presque invisible, puis invisible ; **au survol, elle réapparaît** entièrement, puis s'efface quand la souris s'en va. Déverrouillée : toujours opaque.
- [ ] À 0 % : on peut encore la survoler et cliquer dedans (à vérifier particulièrement, la fenêtre étant transparente).

## Supprimer la boîte privée

- [ ] Menu ☰ > « Supprimer la boîte » (ou carte > « Supprimer… ») : fenêtre demandant le mot de passe et expliquant que tout revient sur le bureau. Mot de passe faux : refusé, rien ne bouge.
- [ ] Bon mot de passe : tous les éléments reviennent sur le bureau (contenu intact, dossiers compris), la boîte disparaît, `%TIDY_DATA_DIR%\vault\` n'existe plus.
- [ ] Supprimer la boîte privée depuis la liste de la page Boîtes (icône corbeille de sa carte) : la boîte disparaît mais le coffre reste ; menu ☰ > « Boîte privée » la recrée, verrouillée, avec son contenu.

## Robustesse

- [ ] Pendant l'ajout du PDF de 50 Mo, tuer `tidy.exe` (Gestionnaire des tâches). Relancer : le PDF est toujours sur le bureau (ou déjà dans le coffre, et alors dans la corbeille) ; jamais perdu. Au déverrouillage, aucun message d'erreur.
- [ ] Fichier ouvert depuis le coffre et modifié, puis tuer `tidy.exe` : au déverrouillage suivant, la modification est dans le coffre et `open.noindex` est vide.
- [ ] Modifier un octet d'un fichier de `vault\objects\` (éditeur hexadécimal) : l'ouvrir ou le remettre sur le bureau donne « Le coffre est illisible ou a été modifié », rien n'est écrit sur le bureau, les autres éléments restent utilisables.
