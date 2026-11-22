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
| **Menu** | outils rangés par catégorie (Jeux, Minecraft, Fichiers, Système, Réseau, Quotidien, Créatif, Fun) avec recherche |
| Jeux | jeux gratuits (Epic, Steam, GOG) et sorties attendues, jeux à finir, succès Steam, roue « à quoi on joue », chrono speedrun, réflexes, sensibilité entre jeux, tests souris et clavier |
| Minecraft + | installer un serveur (Paper, Fabric, Vanilla), plugins et mods Modrinth, réglages `server.properties`, opérateurs, restauration à une date, TPS / mémoire et prégénération Chunky |
| Fichiers + | favoris, visionneuse, images en double, photos (date de prise de vue, EXIF/GPS), vidéo rapide (Discord, découpe, son), PDF, archives (zip et 7z intégrés, rar via Windows), gros fichiers oubliés, déplacement avec jonction, récupération (corbeille, Windows File Recovery), destructeur |
| Système + | infos du PC, processus, services, pilotes, batterie / alimentation / test RAM, vitesse des disques, écrans, bureaux virtuels, transparence (Ctrl+Alt+Page↑/↓), plantages, réparation SFC/DISM |
| Réseau + | téléchargement de vidéos (yt-dlp), bloqueur de pubs et de sites (hosts), DNS le plus rapide, mots de passe Wi-Fi, ports ouverts, coupures internet, Wake-on-LAN, surveillance de pages, liens courts |
| Quotidien + | tâches, Kanban, réveil (sort de veille), temps d'écran, mode concentration, liens, films et séries, livres et mangas, voix (mémos, lecture à voix haute Ctrl+Alt+L, sous-titres), mots de passe |
| Créatif + | éditeur d'images, mèmes, GIF, palette, icônes et dossiers colorés, enregistrement du son du PC, égaliseur (Equalizer APO) |
| Programmes externes | ffmpeg / ffprobe et yt-dlp téléchargés tout seuls en version portable dans `%APPDATA%\Outils\bin` au premier besoin ; rien à installer à la main (sauf Equalizer APO, un pilote, installé en un clic depuis l'égaliseur) |
| Fun + | dés et tirage au sort, 12 mini-jeux (serpent, casse-briques, démineur, fusion de nombres, blocs qui tombent, 4 en ligne et morpion contre l'ordi, paires, pendu, sudoku, raquettes, oiseau), quiz (seul ou à plusieurs) |
| Outils lui-même | rapport de plantage (un onglet qui plante n'arrête plus tout), journal unique, sauvegarde / restauration des réglages, synchro entre PC (dossier OneDrive/Drive), mode portable (`Outils-donnees` à côté de l'exe), onglets favoris (clic droit), palette Ctrl+Alt+K |
| Jeux ++ | overlay en jeu (FPS, CPU, GPU, Ctrl+Alt+H), statistiques de jeu, batterie de manette, compteur de clics et touches, test d'écran, mode stream (masque les onglets sensibles), scènes OBS automatiques, « ça tourne sur mon PC ? », mods du Workshop, nettoyage des jeux, remappage de touches par jeu |
| Minecraft ++ | plusieurs serveurs, carte web BlueMap, liste blanche et bannis avec les têtes, statistiques des joueurs, messages de bienvenue, carte des régions (régénération), ouverture des ports en UPnP et test de NAT, sauvegarde du monde avant de changer de version |
| Fichiers ++ | synchro clé USB au branchement, surveillance de dossiers, renommage d'épisodes de séries, recherche dans le contenu (texte, Word, Excel, PDF…), corbeille automatique, installateurs oubliés, vérification SHA-256, gestionnaire de téléchargements (pause / reprise) |
| Système ++ | test de stabilité (stress CPU + température), fenêtres en grille (Ctrl+Alt+pavé numérique) et toujours au-dessus (Ctrl+Alt+« + » du pavé numérique), points de restauration, antivirus Defender, couper internet à une appli, qui utilise la webcam / le micro, rappel de redémarrage Windows Update, démarrage retardé |
| Quotidien ++ | météo (aussi sur l'Accueil), agenda avec rappels, raccourcis de texte (« ;bjr » → texte complet partout), dictée vocale, eau et calories, presse-papiers PC ↔ téléphone |
| Créatif ++ | micro avec anti-bruit IA et changeur de voix (via VB-CABLE, installé en un clic), captures stylées, générateur de fonds d'écran (un nouveau chaque jour en option) |
| Outils lui-même + | 12 thèmes prêts, mes thèmes (garder, exporter, importer), dock flottant des favoris, mode mini toujours visible, centre de notifications (cloche), visite guidée, statistiques d'utilisation |
| Jeux +++ | temps de chargement des jeux, comparateur de prix entre boutiques, actus et patchs de mes jeux Steam, galerie des captures de jeux, amis Steam en ligne, générateur de pseudos |
| Minecraft +++ | règles du jeu et monde (heure, météo, spawn, difficulté, même serveur arrêté), sauvegarde du monde dans le cloud, redémarrage programmé avec annonces, page de statut web, coordonnées Nether, chat du serveur vers Discord, datapacks |
| Fichiers +++ | ranger les photos par date et par lieu, compresser des images, envoyer des fichiers à un autre PC, lien de partage temporaire, comparer deux dossiers, texte d'une image (OCR de Windows) |
| Système +++ | historique des périphériques, désinstalleur complet (avec les restes), alerte de surchauffe, éteindre quand… (téléchargement fini, PC inactif, programme fermé), luminosité et filtre de lumière bleue, polices, mémoire en veille, fiche du PC |
| Réseau ++ | qui est sur mon Wi-Fi, point d'accès Wi-Fi avec QR code, ping des serveurs de jeux par région, consommation internet par appli, mot de passe fuité |
| Quotidien +++ | calculatrice et convertisseur (unités, devises), actus RSS, comptes à rebours, emploi du temps et devoirs, radio en ligne |
| Créatif +++ | lecteur de musique (playlists, infos et pochettes), zone de l'écran en GIF, miniatures YouTube, éditeur de sous-titres, boîte à sons dans le micro (Ctrl+Maj+F1…F9), pixel art, métronome et boîte à rythmes |
| Fun ++ | échecs contre l'ordi, solitaire, mots mêlés, vitesse de frappe, dango clicker, jeux à deux sur le même PC, jeux en réseau local (4 en ligne, morpion) |
| Outils lui-même ++ | quoi de neuf après chaque mise à jour, profils Jeu / Travail / Nuit, recherche partout (réglages et fichiers du PC), raccourcis clavier vers n'importe quel outil, sauvegarde des réglages sur clé USB, mode grand écran 📺 à la manette |
| Jeux ++++ | minuteurs de cooldown avec alerte vocale, optimisation au lancement d'un jeu, journal de jeu, comparateur de configs, jeux gratuits qui expirent bientôt, profils de mods Minecraft |
| Fichiers ++++ | copier une construction Minecraft d'un monde à l'autre, calculatrice redstone, coffre de photos privées, convertisseur de documents (images ↔ PDF, Office → PDF), renommer les photos par date, doublons de musique, fichiers récents, corbeille de sécurité |
| Système ++++ | alerte programme gourmand, pilotes à jour, bilan du PC sur 100, test de vitesse programmé, partage de dossier sur le réseau, changer de région pour un jeu (WARP), ping dans l'overlay |
| Créatif ++++ | traducteur vocal, éditeur vidéo simple, memes vidéo (MP4 / GIF), bannières et logos (Discord, YouTube, Twitch), stickers et émojis Discord, correcteur d'orthographe partout (Ctrl+Alt+Y), commandes vocales |
| Fun +++ | jeux de cartes (huit américain, bataille), bataille navale (ordi ou réseau), snake à deux (même clavier ou réseau), dames 10×10, sudoku du jour, quiz drapeaux et capitales, course de frappe en réseau |
| Recherche de fichiers | tout le PC en quelques millisecondes (Ctrl+Alt+F) : index des noms en mémoire, lecture de la table NTFS + journal USN en administrateur, sinon parcours parallèle + surveillance des dossiers ; filtres ext:, type:, taille:, date:, dossier:, !, jokers, recherche floue |
| Appli Android | `mobile/` : jeux et outils du quotidien partagés avec le PC, plus Mon PC (télécommande, état, jeux, fichiers du PC, envoi de photos, presse-papiers, Minecraft, notifications, synchro agenda/devoirs), Wi-Fi (signal, appareils, débit, ping), fichiers du téléphone (espace, doublons, rangement, renommage, fichiers et applis louches, corbeille), lampe, boussole, sonomètre, podomètre, scanner QR, labyrinthe, face à face, badges |
| PC ↔ téléphone ++ | manette, pavé tactile et clavier, écran du PC, webcam et micro, traduction par l'appareil photo, sonnerie et fichiers vers le téléphone, accès à distance chiffré (ntfy.sh) ; profils de performance par jeu, FPS avant / après mise à jour |

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
