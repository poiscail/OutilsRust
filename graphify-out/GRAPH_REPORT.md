# Graph Report - OutilsRust  (2026-11-17)

## Corpus Check
- 10 files · ~14,713 words
- Verdict: corpus is large enough that graph structure adds value.
- Unclassified: 1 file(s) not represented in the graph (top: (none) 1)

## Summary
- 415 nodes · 892 edges · 21 communities (20 shown, 1 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 7 edges (avg confidence: 0.65)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `504a8878`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- main.rs
- coffre.rs
- tri.rs
- Jeu
- OutilsRust : tous les outils réécrits en Rust, dans un seul programme
- lanceur.rs
- carte
- outils
- .ecran_principal
- Erreur
- scan_steam
- reg
- fil_saisie
- partie
- CoffreUi
- Sauvegarde
- generer_mot_de_passe
- traducteur.rs
- Traducteur
- .dialogues
- Dialogue

## God Nodes (most connected - your core abstractions)
1. `traduire_contenu()` - 20 edges
2. `CoffreUi` - 18 edges
3. `R` - 17 edges
4. `Erreur` - 16 edges
5. `Tri` - 16 edges
6. `Bloc` - 14 edges
7. `traduire_blocs()` - 14 edges
8. `fil_saisie()` - 13 edges
9. `Jeu` - 12 edges
10. `carte()` - 12 edges

## Surprising Connections (you probably didn't know these)
- `App` --references--> `CoffreUi`  [INFERRED]
  src/main.rs → src/coffre.rs
- `App` --references--> `Lanceur`  [INFERRED]
  src/main.rs → src/lanceur.rs
- `App` --references--> `Traducteur`  [INFERRED]
  src/main.rs → src/traducteur.rs
- `App` --references--> `Tri`  [INFERRED]
  src/main.rs → src/tri.rs
- `deplacer()` --calls--> `w()`  [INFERRED]
  src/tri.rs → src/lanceur.rs

## Import Cycles
- None detected.

## Communities (21 total, 1 thin omitted)

### Community 0 - "main.rs"
Cohesion: 0.12
Nodes (5): ANCIENS, App, CREATE_NO_WINDOW, main(), racine()

### Community 1 - "coffre.rs"
Cohesion: 0.05
Nodes (13): DELAI_PRESSE_PAPIERS, DELAI_VERROU, ecrire_atomique(), GRIS, KDF_DEFAUT, LONGUEUR_MIN_MAITRE, ouvre_un_coffre_python(), RAPIDE (+5 more)

### Community 2 - "tri.rs"
Cohesion: 0.08
Nodes (19): apercu(), categorie(), categories_et_exclusions(), chemin_libre(), chemin_libre_ne_ecrase_pas(), DEFAUT, developper(), dossier_test() (+11 more)

### Community 3 - "Jeu"
Cohesion: 0.40
Nodes (6): epic_et_manuel_et_fivem(), Jeu, jeu_manuel(), Lanceur, scan_epic(), scan_fivem()

### Community 4 - "OutilsRust : tous les outils réécrits en Rust, dans un seul programme"
Cohesion: 0.22
Nodes (7): Graphe, Liste (ordre de réécriture : du plus simple au plus risqué), OutilsRust : tous les outils réécrits en Rust, dans un seul programme, Stack, Compiler, OutilsRust, État

### Community 5 - "lanceur.rs"
Cohesion: 0.08
Nodes (22): ATTENTE, BLEU, CARTE, CH, chemin_processus(), CW, dossier_test(), GRIS (+14 more)

### Community 6 - "carte"
Cohesion: 0.22
Nodes (10): Action, Actualiser, Ajouter, Dossier, Lancer, Retirer, SteamDabord, carte() (+2 more)

### Community 8 - ".ecran_principal"
Cohesion: 0.19
Nodes (11): Action, Copier, Exporter, Maitre, Modifier, Nouveau, Remplir, Supprimer (+3 more)

### Community 9 - "Erreur"
Cohesion: 0.19
Nodes (14): aad(), aller_retour_et_conflit(), changer_et_renforcer(), Coffre, deriver_cle(), dossier(), en_tete_altere_ou_invalide(), Erreur (+6 more)

### Community 10 - "scan_steam"
Cohesion: 0.50
Nodes (5): fichiers(), kv(), kv_tous(), scan_steam(), trouver_jaquette()

### Community 11 - "reg"
Cohesion: 0.25
Nodes (8): ouvrir(), reg(), reg_dword(), reg_texte(), steam_de_ce_pc(), steam_racine(), w(), deplacer()

### Community 12 - "fil_saisie"
Cohesion: 0.18
Nodes (12): fenetre_cible(), fil_saisie(), Morceau, Texte, Touche, mot_cle_site(), restaurer(), Retour (+4 more)

### Community 13 - "partie"
Cohesion: 0.36
Nodes (6): assurer_steam(), Etat, maintenant(), Partage, partie(), statut()

### Community 14 - "CoffreUi"
Cohesion: 0.23
Nodes (6): CoffreUi, copier(), effacer_si_inchange(), ouvrir(), poser(), session_verrouillee()

### Community 15 - "Sauvegarde"
Cohesion: 0.40
Nodes (3): Sauvegarde, tri_des_jeux(), trier()

### Community 16 - "generer_mot_de_passe"
Cohesion: 0.67
Nodes (3): force_et_generateur(), generer_mot_de_passe(), tirage()

### Community 17 - "traducteur.rs"
Cohesion: 0.05
Nodes (86): agent(), aller_retour_crlf_bom_et_cp1252(), ANNULE, ass_hms(), ass_vers_html(), AUTO, BALISE, balises_intactes() (+78 more)

### Community 18 - "Traducteur"
Cohesion: 0.11
Nodes (16): Apercu, appdata(), config_chemin(), ecrire_config(), glossaire_chemin(), lire_config(), Moteur, DeepL (+8 more)

### Community 19 - ".dialogues"
Cohesion: 0.21
Nodes (6): activer(), champ_mdp(), hex(), reduire(), verr_maj(), maj()

### Community 20 - "Dialogue"
Cohesion: 0.40
Nodes (5): Dialogue, Confirmer, Entree, Maitre, FormEntree

## Knowledge Gaps
- **16 isolated node(s):** `outils`, `MauvaisMotDePasse`, `ModifieAilleurs`, `Maitre`, `Confirmer` (+11 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 112 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **1 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `App` connect `main.rs` to `tri.rs`, `Traducteur`, `Jeu`, `CoffreUi`?**
  _High betweenness centrality (0.065) - this node is a cross-community bridge._
- **What connects `outils`, `MauvaisMotDePasse`, `ModifieAilleurs` to the rest of the system?**
  _16 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `main.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.12280701754385964 - nodes in this community are weakly interconnected._
- **Why does `CoffreUi` connect `CoffreUi` to `main.rs`, `coffre.rs`, `.ecran_principal`, `Erreur`, `fil_saisie`, `.dialogues`, `Dialogue`?**
  _High betweenness centrality (0.062) - this node is a cross-community bridge._
- **Should `coffre.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.048726467331118496 - nodes in this community are weakly interconnected._
- **Why does `Tri` connect `tri.rs` to `main.rs`?**
  _High betweenness centrality (0.049) - this node is a cross-community bridge._
- **Should `tri.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.08156028368794327 - nodes in this community are weakly interconnected._