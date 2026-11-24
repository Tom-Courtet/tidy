# ADR 007 · Recherche : index maison et Spotlight

Statut : Acceptée (24/11/2026)

## Décision

Index en mémoire (`nucleo`, FSEvents) sur les applis, le bureau, Documents et Téléchargements (dossiers réglables), puis « Plus de résultats » via Spotlight (`NSMetadataQuery`) pour le reste du disque. Banc d'essai : moins de 50 ms sur 500 000 entrées.

## Conséquences

- Instantané et léger sur l'essentiel ; pas d'accès complet au disque requis.
