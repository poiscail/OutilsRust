# OutilsRust

Tous mes outils Windows réécrits en Rust, dans un seul programme (`Outils.exe`, interface egui).
Plan, liste et graphe : [PLAN.md](PLAN.md).

## État

| Outil | Rust |
|-------|------|
| Tri Téléchargements | fait (mêmes règles `regles.json`) |
| Lanceur Jeux | fait (même `%APPDATA%\LanceurJeux\data.json`) |
| Coffre | fait (même `%APPDATA%\CoffreMotsDePasse\coffre.json`) |
| Traducteur | fait (même `%APPDATA%\TraducteurSousTitres.json` et glossaire ; Google/DeepL, ffmpeg pour les vidéos) |
| Presse-papiers | fait (même `%APPDATA%\GestionnairePressePapiers`, raccourci de `parametres.json`) |
| Serveur Minecraft | fait (même `%APPDATA%\GestionServeurMinecraft.json` ; veille, playit, modpacks, kit amis) |
| Moniteur Jeu | fait (même `MoniteurJeu\reglages.ini` et `historique\` ; mode jeu, FPS ETW en admin, overlay) |
| Optimiseur Windows | fait (nettoyage, confidentialité, perf, confort ; applis ; démarrage/arrière-plan ; admin requis pour HKLM/services) |
| Accueil | disques, état du serveur, du tri, du coffre, du minuteur et des mises à jour |
| Renommer | renommage en lot (rechercher/remplacer, préfixe, suffixe, numérotation), aperçu et conflits |
| Minuteur | Pomodoro (travail / pause), son et notification à la fin |
| Personnaliser | couleurs, polices importées (`%APPDATA%\Outils\polices`), image de fond (6 fonds japonais inclus dans l’exe, déposés dans `fonds\` au lancement) et rotation auto, fond d'écran Windows, pétales |

## Options (onglet Personnaliser)

- Notifications Windows (fichier rangé, joueur connecté, nettoyage fini, Pomodoro).
- Lancer avec Windows, icône dans la zone de notification (fermer = ranger), raccourci `Ctrl+Alt+O`.
- Vérifier les mises à jour au démarrage : télécharge l'exe de la dernière release GitHub et relance.

## Compiler

Toolchain `stable-x86_64-pc-windows-gnu` + MinGW (WinLibs, pour `dlltool`/`as`) dans le PATH :

```
cargo test
cargo build --release
```

L'exe est dans `target\release\Outils.exe`.
