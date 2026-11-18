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
| Personnaliser | couleurs, polices importées (`%APPDATA%\Outils\polices`), image de fond (6 fonds japonais inclus dans l’exe, déposés dans `fonds\` au lancement) et rotation auto, fond d'écran Windows, effets (pétales, pluie, neige, lucioles) |
| Espace disque | carte en blocs des dossiers (clic pour entrer), doublons (taille puis SHA-256), corbeille |
| Sauvegardes jeux | zips horodatés des dossiers de sauvegarde, détection (Saved Games, My Games, LocalLow, Minecraft), restauration, auto si modifié (`%APPDATA%\Outils\sauvegardes.json`) |
| Convertisseur | images en lot (PNG, JPG, WebP, BMP, ICO, redimensionnement) ; vidéos/sons via ffmpeg (MP4, MKV, WebM, GIF, MP3, M4A, OGG, FLAC, WAV) |
| Notes | notes colorées, post-its toujours devant, rappels avec notification (`%APPDATA%\Outils\notes.json`) |
| Réseau | ping en continu (graphe, gigue, pertes), débit de la carte, test de vitesse (Cloudflare), connexions par appli |
| Discord | statut Rich Presence (jeu en cours, serveur Minecraft) avec un ID d'application ; webhook : serveur allumé/éteint, joueurs |
| Capture | Ctrl+Alt+S zone, W fenêtre, P écran, C pipette, M règle, R replay ; éditeur (flèche, cadre, surligneur, flou) ; vidéo et replay des dernières secondes via ffmpeg |
| Mixeur audio | volume général et par appli, sortie par défaut, Ctrl+Alt+A sortie suivante, profils appliqués au lancement d'un jeu |

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
