# ADR 006 · Boîte privée : image APFS chiffrée et enveloppe de clés

Statut : Acceptée (24/11/2026)

## Décision

Le contenu de la boîte privée vit dans une image disque APFS chiffrée (`hdiutil`, sparsebundle) montée sans apparaître dans le Finder pendant le déverrouillage. Sa clé aléatoire est chiffrée deux fois par Tidy (Argon2id + XChaCha20-Poly1305) : avec le mot de passe (6 à 20 caractères) et avec une clé de récupération à noter. Derrière un trait `VaultBackend`.

## Conséquences

- Rien en clair sur le disque, applis et gros fichiers sans souci.
- Le code cryptographique de Tidy se limite à 32 octets, testé aller-retour.
- Windows demandera un autre `VaultBackend`.
