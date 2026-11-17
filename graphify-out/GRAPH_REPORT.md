# Graph Report - OutilsRust  (2026-11-17)

## Corpus Check
- 7 files · ~4,960 words
- Verdict: corpus is large enough that graph structure adds value.
- Unclassified: 1 file(s) not represented in the graph (top: (none) 1)

## Summary
- 161 nodes · 308 edges · 13 communities (11 shown, 2 thin omitted)
- Extraction: 98% EXTRACTED · 2% INFERRED · 0% AMBIGUOUS · INFERRED: 5 edges (avg confidence: 0.71)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `d60b310c`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- main.rs
- Tri
- tri.rs
- Jeu
- OutilsRust : tous les outils réécrits en Rust, dans un seul programme
- lanceur.rs
- carte
- outils
- Color32
- steam_pret
- scan_steam
- reg
- dossier_test

## God Nodes (most connected - your core abstractions)
1. `Tri` - 16 edges
2. `Jeu` - 12 edges
3. `carte()` - 12 edges
4. `Action` - 10 edges
5. `Regles` - 10 edges
6. `partie()` - 9 edges
7. `Lanceur` - 9 edges
8. `scan_steam()` - 8 edges
9. `ranger()` - 8 edges
10. `surveiller()` - 8 edges

## Surprising Connections (you probably didn't know these)
- `App` --references--> `Lanceur`  [INFERRED]
  src/main.rs → src/lanceur.rs
- `App` --references--> `Tri`  [INFERRED]
  src/main.rs → src/tri.rs
- `deplacer()` --calls--> `w()`  [INFERRED]
  src/tri.rs → src/lanceur.rs

## Import Cycles
- None detected.

## Communities (13 total, 2 thin omitted)

### Community 0 - "main.rs"
Cohesion: 0.12
Nodes (5): ANCIENS, App, CREATE_NO_WINDOW, main(), racine()

### Community 2 - "tri.rs"
Cohesion: 0.10
Nodes (17): apercu(), categorie(), categories_et_exclusions(), chemin_libre(), chemin_libre_ne_ecrase_pas(), DEFAUT, dossier_test(), extension() (+9 more)

### Community 3 - "Jeu"
Cohesion: 0.19
Nodes (15): assurer_steam(), epic_et_manuel_et_fivem(), Etat, Jeu, jeu_manuel(), Lanceur, maintenant(), ouvrir() (+7 more)

### Community 4 - "OutilsRust : tous les outils réécrits en Rust, dans un seul programme"
Cohesion: 0.22
Nodes (7): Graphe, Liste (ordre de réécriture : du plus simple au plus risqué), OutilsRust : tous les outils réécrits en Rust, dans un seul programme, Stack, Compiler, OutilsRust, État

### Community 5 - "lanceur.rs"
Cohesion: 0.11
Nodes (7): ATTENTE, CH, CW, IGNORER, JAQUETTES, PAS, TRIS

### Community 6 - "carte"
Cohesion: 0.14
Nodes (13): Action, Actualiser, Ajouter, Dossier, Lancer, Retirer, SteamDabord, carte() (+5 more)

### Community 8 - "Color32"
Cohesion: 0.29
Nodes (6): BLEU, CARTE, GRIS, SURVOL, TEXTE, VERT

### Community 9 - "steam_pret"
Cohesion: 0.40
Nodes (5): chemin_processus(), norm(), steam_dabord(), steam_pret(), tourne_dans()

### Community 10 - "scan_steam"
Cohesion: 0.50
Nodes (5): fichiers(), kv(), kv_tous(), scan_steam(), trouver_jaquette()

### Community 11 - "reg"
Cohesion: 0.40
Nodes (5): reg(), reg_dword(), reg_texte(), steam_de_ce_pc(), steam_racine()

### Community 12 - "dossier_test"
Cohesion: 0.50
Nodes (4): dossier_test(), jaquette_portrait_preferee(), surveiller(), vrai_jeu_compte_le_temps()

## Knowledge Gaps
- **3 isolated node(s):** `outils`, `Ajouter`, `Actualiser`
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 44 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **2 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Tri` connect `Tri` to `main.rs`, `tri.rs`?**
  _High betweenness centrality (0.122) - this node is a cross-community bridge._
- **What connects `outils`, `Ajouter`, `Actualiser` to the rest of the system?**
  _3 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `main.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.12280701754385964 - nodes in this community are weakly interconnected._
- **Why does `App` connect `main.rs` to `Tri`, `Jeu`?**
  _High betweenness centrality (0.103) - this node is a cross-community bridge._
- **Should `tri.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.10252100840336134 - nodes in this community are weakly interconnected._
- **Why does `Lanceur` connect `Jeu` to `main.rs`, `lanceur.rs`, `carte`?**
  _High betweenness centrality (0.086) - this node is a cross-community bridge._
- **Should `lanceur.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.10526315789473684 - nodes in this community are weakly interconnected._