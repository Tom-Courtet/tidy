# ADR 013 · Boîte privée : un coffre chiffré fichier par fichier, le même sur les deux OS

Statut : Proposée (26/11/2026, lot F). Remplace la décision de l'ADR 006 (image APFS).

## Contexte

L'ADR 006 prévoyait une image disque APFS chiffrée (`hdiutil`) : elle n'existe pas sous Windows, la cible prioritaire (ADR 001). Les équivalents Windows ne conviennent pas : BitLocker et les VHDX chiffrés manquent sur Windows Famille, VeraCrypt ou un système de fichiers virtuel demandent un pilote. Le cahier des charges exige un vrai chiffrement (Argon2id + XChaCha20-Poly1305), aucun fichier en clair après le verrouillage et un verrouillage automatique. Exigence du propriétaire : **ne jamais perdre un fichier**.

## Décision

**Un seul backend, `tidy_vault::FileVault`, sous Windows comme sous macOS**, derrière le trait `VaultBackend`. Chaque fichier est chiffré séparément dans le dossier de données :

```text
<données>/vault/vault.json        en-tête : réglages d'Argon2id, clé enveloppée deux fois
<données>/vault/index.tv          index chiffré : noms, tailles, dates, objets
<données>/vault/objects/<id>.tv   un objet chiffré par fichier
<données>/vault/open.noindex/     copies en clair des fichiers ouverts
```

- **Clé maîtresse** : 32 octets aléatoires (aléa du système), jamais écrits en clair. Elle est **enveloppée deux fois** par XChaCha20-Poly1305, avec une clé dérivée par **Argon2id** (64 Mio, 3 passes, sel de 16 octets) : du mot de passe (6 à 20 caractères), et d'une **clé de récupération** de 120 bits (24 caractères, base 32 de Crockford, `XXXX-XXXX-…`) affichée une seule fois à la création. Elle remplace le courriel de récupération de l'original : pas de serveur. Changer de mot de passe ou le réinitialiser par la clé de récupération ne réécrit que l'enveloppe.
- **Fichiers** : XChaCha20-Poly1305 par morceaux d'un Mio. Nonce de chaque morceau = préfixe aléatoire propre au fichier ‖ numéro du morceau ‖ drapeau « dernier » ; données associées = identifiant de l'objet. Un morceau modifié, permuté, retiré, ou un fichier tronqué, prolongé ou échangé avec un autre est refusé au déchiffrement.
- **Noms et index chiffrés** : l'index (JSON) est chiffré par la clé maîtresse ; les objets portent des identifiants aléatoires. Sur le disque, on ne voit que le nombre et la taille approximative des fichiers.
- **Écritures atomiques** (fichier voisin, `fsync`, renommage) pour l'en-tête et l'index.
- **Ouvrir** un fichier le déchiffre dans `open.noindex/<jeton>/` (dossier de données de Tidy, pas `%TEMP%` partagé ; le suffixe `.noindex` l'écarte de Spotlight), puis l'ouvre avec l'appli par défaut. Au verrouillage, les copies modifiées sont **rechiffrées** (nouvel objet vérifié, puis index, puis suppression de l'ancien), puis chaque copie est **écrasée de zéros et supprimée** (ce sont des copies créées par Tidy, pas des fichiers de l'utilisateur).
- Crates RustCrypto `argon2` 0.5, `chacha20poly1305` 0.10, `zeroize` 1.8 (MIT ou Apache-2.0), `getrandom` 0.4 (déjà présent par `uuid`).

### Ne jamais perdre un fichier

Ordre des opérations :

| Opération | Étapes | Si une étape échoue |
|---|---|---|
| Ajouter (glisser du bureau) | chiffrer → relire et **tout déchiffrer en comparant à l'original** → écrire l'index → **corbeille** de l'original (`DesktopPlatform::trash`) | l'original reste en place ; si la corbeille refuse, l'ajout est annulé |
| Remettre sur le bureau | déchiffrer dans un nom de passage → renommer à sa place → retirer de l'index → supprimer les objets | le coffre n'a pas changé ; la copie partielle (de Tidy) est retirée |
| Fichier ouvert modifié | nouvel objet vérifié → index → ancien objet supprimé → copie effacée | la copie en clair reste (signalée) et sera reprise |
| Supprimer la boîte privée | mot de passe, puis chaque élément rendu au bureau comme ci-dessus, puis suppression des fichiers propres au coffre (vide) | les éléments non rendus restent dans le coffre |

Un objet que l'index ne cite pas vient toujours d'une opération interrompue dont l'original existe encore : il est supprimé au déverrouillage suivant. Un index illisible bloque le déverrouillage sans rien supprimer. Les copies en clair laissées par un plantage sont reprises au déverrouillage suivant (contenu comparé, modifications rechiffrées, élément remis au coffre s'il en était sorti), puis effacées. Tout est testé (`crates/tidy-vault`, 22 tests : aller-retour, mauvais mot de passe, clé de récupération, gros fichier, fichier altéré, index altéré, pannes simulées pendant un ajout, corbeille refusée, plantage avec des fichiers ouverts).

### Dans Tidy

Le coffre vit dans le processus du bureau (`app/src/desktop/vault.rs`), sur un fil à part (Argon2id et le chiffrement des gros fichiers ne bloquent pas l'affichage). Une seule boîte `BoxKind::Private`. Mots de passe et clé de récupération ne quittent jamais ce processus : ils ne passent ni par l'IPC, ni par les journaux, ni par `config.json`, et sont effacés de la mémoire (`zeroize`) après usage. La fenêtre principale ne fait que demander au bureau d'ouvrir ses formulaires (`Request::Module { module: "vault" }`).

Verrouillage automatique (`Settings::vault`) : après N minutes sans clavier ni souris dans tout le système (`DesktopPlatform::idle_seconds` ; à défaut, sans interaction avec la boîte), à la mise en veille et au verrouillage de la session (`DesktopPlatform::watch_session` : `WTSRegisterSessionNotification` et `WM_POWERBROADCAST` sous Windows, notifications de `NSWorkspace` et `com.apple.screenIsLocked` sous macOS).

## Limites (connues et acceptées)

- **Copie en clair pendant l'ouverture d'un fichier** : un fichier ouvert existe en clair dans `open.noindex/` jusqu'au verrouillage. L'appli qui l'ouvre peut en garder des traces hors de Tidy (fichiers récents, miniatures, sauvegardes automatiques, `~$` de Word, liste de raccourcis de Windows). « Remettre sur le bureau » puis travailler sur le fichier est l'usage recommandé pour un long travail.
- **Écraser de zéros n'est pas un effacement sûr** sur un SSD (remappage des blocs) ni sur un disque avec instantanés (Time Machine local, points de restauration de Windows, OneDrive si le dossier de données y était). C'est une précaution, pas une garantie ; le chiffrement intégral du disque (BitLocker, FileVault) reste la vraie protection de ces copies.
- **L'original part à la corbeille, en clair** : le vider est laissé à l'utilisateur (jamais de suppression définitive, règle du projet). L'avertissement de l'ajout le dit.
- Un fichier encore ouvert par une appli au verrouillage (Word le bloque sous Windows) n'est pas effacé : il est signalé et repris au déverrouillage suivant.
- Les fichiers supprimés d'un dossier ouvert ne sont pas retirés du coffre (prudence) ; les fichiers ajoutés ou modifiés, si.
- Liens symboliques refusés (l'original reste). Pas d'accès simultané par deux processus.
- Le coffre ne cache ni le nombre ni la taille approximative des fichiers, et la clé maîtresse est en mémoire tant que la boîte est déverrouillée.

## Alternatives écartées

- **Image APFS** (ADR 006) : macOS seulement ; deux backends à maintenir et à tester.
- **VHDX + BitLocker** : absent de Windows Famille, demande les droits d'administrateur.
- **Volume chiffré par un pilote tiers** (VeraCrypt, Dokan, WinFsp) : pilote à installer, signature, surface d'attaque.
- **Crate `age`** : format éprouvé mais sans index chiffré ni accès par morceaux simple ; plus lourd que le besoin.
