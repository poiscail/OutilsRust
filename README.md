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
| Planificateur | tâches à heure fixe : sauvegardes des jeux, nettoyage, serveur Minecraft (démarrer, arrêter, redémarrer, sauvegarder, commande), programme, notification, veille/arrêt du PC |
| Téléphone | serveur web local ouvert par QR code : télécommande (son, musique, verrouiller, veille, serveur Minecraft), fichiers à récupérer, envoi de photos vers le PC |
| Ne pas déranger | pendant les jeux, sur une plage horaire ou à la main : mode Ne pas déranger de Windows, notifications d'Outils coupées, son des applis choisies baissé |
| Démarrage PC | durée des derniers démarrages et ce qui les a ralentis (journal Diagnostics-Performance, admin) |
| Promos Steam | prix de la liste de souhaits (profil public) et de jeux ajoutés, alerte en promo ou sous un prix voulu, vérification toutes les 6 h |
| Mods | par jeu : activer / désactiver chaque mod (dossier `_desactives`), profils, ajout par glisser-déposer |
| Mises à jour logiciels | `winget upgrade` : liste, mise à jour d'un logiciel, de la sélection ou de tout |
| Santé des disques | état Windows, température, usure, heures, erreurs (SMART en admin) ; alerte toutes les 12 h |
| Coffre de fichiers | dossier ou fichier chiffré en `.coffre` (zip + AES-256-GCM par blocs, Argon2id) |
| Viseur | croix / point / cercle au centre de l'écran, transparent aux clics, Ctrl+Alt+X |
| Macros | enregistrement clavier/souris (arrêt Ctrl+Alt+F12), lecture Ctrl+Alt+F1-F11, répétitions, vitesse, Échap pour arrêter |
| Traduction instantanée | Ctrl+Alt+T sur un texte sélectionné : bulle de traduction (Google), clic pour copier |
| Économiseur d'écran | fonds + effets + horloge ; installé comme `%APPDATA%\Outils\Outils.scr` |

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
