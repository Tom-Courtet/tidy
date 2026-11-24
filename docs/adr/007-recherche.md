# ADR 007 · Recherche : index maison et Spotlight

Statut : Acceptée (24/11/2026)

## Décision

Index en mémoire (`nucleo`, FSEvents) sur les applis, le bureau, Documents et Téléchargements (dossiers réglables), puis « Plus de résultats » via Spotlight (`NSMetadataQuery`) pour le reste du disque. Banc d'essai : moins de 50 ms sur 500 000 entrées.

## Conséquences

- Instantané et léger sur l'essentiel ; pas d'accès complet au disque requis.


## À revoir pour Windows (ADR 001)

Spotlight n'existe pas sous Windows : l'index maison devient la source principale (dossiers réglables, surveillance par `ReadDirectoryChangesW` via `notify`), Windows Search en option pour le reste du disque.
