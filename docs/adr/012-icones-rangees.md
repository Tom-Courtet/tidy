# ADR 012 · Icônes rangées : déplacées hors de l'écran (piste 1)

Statut : Acceptée (24/11/2026)

## Décision

Sous Windows, l'icône d'un élément rangé dans une boîte est **déplacée hors de l'écran** sur le bureau de l'Explorateur (piste 1 de PLAN § 1 bis), au lieu de modifier le fichier (attribut caché) ou de redessiner tout le bureau.

- Accès aux icônes par l'API du Shell (COM : `IShellWindows` → `IShellBrowser` → `IFolderView`), méthode documentée par Raymond Chen : chemin complet de chaque icône, position, déplacement par `SelectAndPositionItems`. Ni injection, ni lecture de la mémoire de l'Explorateur.
- **Journal** `icon-stash.json` : la position d'origine est écrite (atomiquement) **avant** tout déplacement. La « disposition automatique » de l'Explorateur, qui empêcherait de placer les icônes, est coupée si besoin puis rétablie quand plus rien n'est masqué.
- Les icônes reviennent : en quittant, avec `tidy --restore-desktop`, avec `tidy --uninstall`, quand le bureau perd l'agent, et au démarrage suivant après un plantage (synchronisation avec les boîtes).
- Logique dans `tidy-core::icon_stash` (testée avec un faux bureau), accès système dans `tidy-platform/windows/icons.rs`.

## Conséquences

- Aucun fichier touché. Si l'Explorateur remet une icône à l'écran (redémarrage), la synchronisation suivante la renvoie, en gardant sa position d'origine.
- Éléments du bureau public (`C:\Users\Public\Desktop`) : pas encore pris en compte par les boîtes (vague 1).
- macOS : pas d'équivalent (le Finder ne se pilote pas ainsi) ; les icônes rangées restent visibles derrière les boîtes en mode superposé.
- Vague 1 : la synchronisation doit être relancée à chaque changement du contenu des boîtes.
