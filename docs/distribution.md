# Distribution

Comment produire, signer et publier Tidy sous Windows et sous macOS.

## Windows

La CI (`.github/workflows/build.yml`) compile `tidy.exe` en release à chaque push sur
`main` (artefact `tidy-windows` de l'onglet *Actions*) et le joint à la Release GitHub
sur chaque tag `v*` :

```sh
git tag v0.2.0 && git push origin v0.2.0
```

L'exe n'est pas signé : à la première ouverture, SmartScreen affiche « Windows a
protégé votre ordinateur ». *Informations complémentaires* puis *Exécuter quand même*.
Une signature Authenticode (certificat de signature de code, idéalement EV) retire cet
avertissement ; elle se fait avec `signtool sign /fd SHA256 /tr <horodateur> /td SHA256
tidy.exe`, à ajouter dans la CI quand un certificat existera (secret du dépôt).

### Installation

Pas d'installateur à part : l'exe s'installe lui-même, sans droits d'administrateur.

- `tidy.exe --install` (ou le bouton de l'assistant, de l'Accueil, de la page
  Options) copie l'exe dans `%LOCALAPPDATA%\Programs\Tidy\tidy.exe`, y pose
  `tidy.ico`, crée `Tidy.lnk` dans le menu Démarrer
  (`%APPDATA%\Microsoft\Windows\Start Menu\Programs`) et l'entrée
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Tidy` (« Applications et
  fonctionnalités » : nom, version, éditeur, icône, taille, date,
  `UninstallString` = `"…\tidy.exe" --uninstall`). Le démarrage avec Windows et
  l'entrée « Créer une boîte » du menu du bureau pointent ensuite vers la copie
  installée. Tidy installé est relancé.
- **Mise à jour** : lancer `--install` depuis un exe plus récent (ou de même version :
  les builds de la CI gardent le même numéro) arrête Tidy (le bureau verrouille la
  boîte privée), remplace la copie installée et la relance. Une version installée plus
  récente n'est remplacée qu'avec `--install --force`. Double-cliquer sur un exe
  téléchargé alors que la copie installée tourne ouvre la page Options de cet exe, qui
  propose la mise à jour.
- **Désinstallation** : « Applications et fonctionnalités » > Tidy > Désinstaller, ou
  `tidy.exe --uninstall`, ou Options > Désinstaller. Le dossier de données
  (`%APPDATA%\Tidy\data` : boîte privée, dispositions, réglages, journaux) reste.

## macOS

```sh
sh tools/bundle_macos.sh      # dist/Tidy.app (LSUIElement : ni Dock ni ⌘Tab)
sh tools/dmg_macos.sh         # dist/Tidy-<version>.dmg (Tidy.app + raccourci Applications)
```

Sans certificat, l'app est signée **ad hoc** : elle se lance sur le Mac qui l'a
construite ; ailleurs, Gatekeeper la bloque (clic droit > Ouvrir, ou *Réglages
Système > Confidentialité et sécurité > Ouvrir quand même*), et macOS peut redemander
les autorisations après chaque build (l'identité change).

### Signature avec un certificat Developer ID

1. Adhérer à l'Apple Developer Program, puis créer un certificat **Developer ID
   Application** (Xcode > Settings > Accounts > Manage Certificates, ou
   developer.apple.com) ; il s'installe dans le trousseau.
2. Vérifier son nom : `security find-identity -v -p codesigning`.
3. Construire et signer (exécution renforcée `--options runtime` et horodatage,
   exigés par la notarisation) :

   ```sh
   export TIDY_SIGN_IDENTITY="Developer ID Application: Prénom Nom (ABCDE12345)"
   sh tools/bundle_macos.sh
   ```

Tidy n'a pas besoin d'*entitlements* : pas de JIT, pas de bibliothèque tierce chargée
à l'exécution, pas d'Apple Events (les notifications passent par `osascript`).

### Notarisation

1. Créer un mot de passe d'app sur appleid.apple.com, puis enregistrer un profil dans
   le trousseau (une fois) :

   ```sh
   xcrun notarytool store-credentials tidy-notary \
     --apple-id "toi@example.com" --team-id ABCDE12345 --password "abcd-efgh-ijkl-mnop"
   ```

2. Construire, signer, notariser et agrafer le ticket au `.dmg` :

   ```sh
   TIDY_SIGN_IDENTITY="Developer ID Application: …" TIDY_NOTARY_PROFILE=tidy-notary \
     sh tools/dmg_macos.sh
   ```

3. Vérifier : `spctl -a -vvv -t install dist/Tidy-<version>.dmg` et, après copie dans
   Applications, `spctl -a -vvv /Applications/Tidy.app` (« source=Notarized Developer
   ID »). En cas de refus : `xcrun notarytool log <id> --keychain-profile tidy-notary`.

### Autorisations demandées par macOS

| Accès | Pourquoi | Texte (Info.plist) |
|---|---|---|
| Bureau | les boîtes montrent les éléments du bureau | `NSDesktopFolderUsageDescription` |
| Documents, Téléchargements | la recherche rapide les parcourt | `NSDocumentsFolderUsageDescription`, `NSDownloadsFolderUsageDescription` |
| Notifications | pomodoro, rappels (via `osascript`, au nom d'« Éditeur de script ») | aucun |

Ni Accessibilité ni enregistrement de l'écran : les raccourcis globaux passent par
l'API Carbon (`global-hotkey`), qui n'en demande pas. L'écran des autorisations est
dans l'assistant de premier lancement et dans Options > Autorisations de macOS, avec
un bouton vers *Réglages Système > Confidentialité et sécurité > Fichiers et
dossiers*.

### Désinstallation

`/Applications/Tidy.app/Contents/MacOS/tidy --uninstall` (ou Options > Désinstaller)
arrête Tidy, rend les icônes du Finder et le fond d'écran d'origine et retire
l'ouverture de session ; mettre ensuite Tidy.app à la corbeille. Les données restent
dans `~/Library/Application Support/Tidy/`.
