# ADR 010 · Un exécutable, plusieurs processus

Statut : Acceptée (24/11/2026)

## Décision

Comme Snappy et Tappy : l'agent (`tidy`) tourne en permanence et reste léger ; le bureau (`--desktop`) est surveillé et relancé par l'agent ; la fenêtre principale (`--manager`) rend sa mémoire en se fermant. Dialogue en JSON ligne par ligne sur un socket Unix (TCP local sous Windows), avec abonnement aux événements.

## Conséquences

- Mesuré (release) : agent 14 Mo, 0 % de processeur au repos ; fenêtre principale ≈ 110 Mo tant qu'elle est ouverte.
- Un plantage du bureau n'emporte ni l'agent ni le Finder.
