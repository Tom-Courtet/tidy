# Tests manuels

Ce qui touche au vrai bureau ne se teste pas automatiquement. Toujours avec des données jetables, sauf mention contraire :

```sh
export TIDY_DATA_DIR=$(mktemp -d)
cargo build && target/debug/tidy
```

## Vague 0 : prototype du bureau

### Mode superposé (défaut, sans toucher au Finder)
- [ ] L'icône Tidy apparaît dans la barre des menus, pas dans le Dock ni dans ⌘Tab.
- [ ] La fenêtre principale s'ouvre au lancement ; « Ouvrir Tidy… » la ramène devant si elle est déjà ouverte.
- [ ] La « Boîte de démonstration » est en haut à droite de l'écran principal, **sous** les fenêtres des applis, **au-dessus** du fond d'écran.
- [ ] Les icônes du Finder restent cliquables partout sauf sous la boîte.
- [ ] La boîte se déplace par sa barre de titre, y compris vers un écran externe ; sa position survit à un redémarrage de Tidy.
- [ ] Débrancher l'écran externe puis le rebrancher : la boîte retrouve la position qu'elle avait avec lui.
- [ ] Changer de Space (bureau virtuel) : la boîte est présente sur chacun.
- [ ] Mission Control (F3) et « Afficher le bureau » (F11 ou geste) : la boîte ne s'envole pas.
- [ ] ⌃⌥H masque puis montre la boîte ; le menu « Masquer le bureau » aussi.
- [ ] Options > Langue > English : le menu de la barre des menus et la fenêtre passent en anglais.

### Robustesse
- [ ] `kill -9` du processus `tidy --desktop` : il revient en moins d'une seconde.
- [ ] Trois `kill -9` en moins d'une minute : il ne revient plus (journal : « n'insiste pas »).
- [ ] `kill -9` de l'agent : le bureau disparaît aussi.
- [ ] `tidy --quit` : tout s'arrête.

### Mode géré (⚠ modifie le Finder ; seulement avec ton accord)
Avant : noter `defaults read com.apple.finder CreateDesktop` (souvent « does not exist »).
- [ ] Options > Mode du bureau > Géré : la modale d'accord s'affiche ; « Annuler » ne change rien.
- [ ] « Gérer le bureau » : le Finder redémarre, ses icônes du bureau disparaissent, la boîte reste.
- [ ] Double-clic sur une zone vide du bureau : la boîte disparaît ; second double-clic : elle revient.
- [ ] `kill -9` de l'agent : le bureau s'arrête **et les icônes du Finder reviennent**.
- [ ] Relancer Tidy, repasser en géré, puis « Quitter Tidy » : les icônes reviennent.
- [ ] Repasser en géré, puis Diagnostic > « Rendre les icônes du Finder » : retour en superposé, icônes revenues.
- [ ] Après chaque essai, `defaults read com.apple.finder CreateDesktop` redonne la valeur notée au départ.
- [ ] Secours : `tidy --restore-finder` rend les icônes si un essai a mal tourné.

### Mesures (build release)
- [ ] `footprint <pid>` de l'agent et du bureau, avec 1 puis 2 écrans (voir README).
- [ ] Processeur au repos : `top -pid <pid>` ≈ 0 %.
