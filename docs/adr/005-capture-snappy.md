# ADR 005 · Capture d'écran déléguée à Snappy

Statut : Acceptée (24/11/2026)

## Décision

L'original contient un outil de capture. Tidy ne le refait pas : la tuile « Capture » des outils rapides lance Snappy (`snappy --region`), ou propose de l'installer s'il manque.

## Conséquences

- Aucun code en double, aucune autorisation « Enregistrement de l'écran » pour Tidy.
