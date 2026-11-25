# Règles du projet Outils

## Versions : seulement quand je décide de publier
- Ne jamais changer les numéros de version tout seul (`Cargo.toml`, `mobile/Cargo.toml`, `linux/Cargo.toml`)
  en ajoutant des fonctions ou en corrigeant des bugs.
- Les nouveautés s'écrivent dans une section `## Prochaine version` en haut de `NOUVEAUTES.md`,
  sans numéro.
- Les builds de test gardent la version actuelle.
- Quand je dis que je veux publier, et seulement à ce moment-là :
  1. Me proposer les nouveaux numéros (PC, téléphone, Linux) et attendre mon accord.
  2. Renommer `## Prochaine version` avec le numéro et le titre.
  3. Changer les versions, construire, mettre les fichiers dans `release/` :
     - `Outils.exe` ;
     - `Outils-<version téléphone>.apk` ;
     - `Outils-linux-<version Linux>.tar.gz`.
  4. Donner les liens et les étapes de publication.
- Le tag de la release GitHub est la version PC.

## Idées
- Quand je choisis des numéros dans une liste d'idées, ne pas me reproposer ensuite les idées que je n'ai pas choisies.
