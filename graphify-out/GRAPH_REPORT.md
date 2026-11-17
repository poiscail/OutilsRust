# Graph Report - OutilsRust  (2026-11-17)

## Corpus Check
- 6 files · ~2,393 words
- Verdict: corpus is large enough that graph structure adds value.
- Unclassified: 1 file(s) not represented in the graph (top: (none) 1)

## Summary
- 78 nodes · 129 edges · 8 communities (5 shown, 3 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 1 edges (avg confidence: 0.5)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- main.rs
- Tri
- Regles
- tri.rs
- OutilsRust : tous les outils réécrits en Rust, dans un seul programme
- ranger_deplace_sans_ecraser
- Color32
- outils

## God Nodes (most connected - your core abstractions)
1. `Tri` - 16 edges
2. `Regles` - 10 edges
3. `ranger()` - 8 edges
4. `surveiller()` - 8 edges
5. `App` - 5 edges
6. `categorie()` - 5 edges
7. `apercu()` - 5 edges
8. `ranger_deplace_sans_ecraser()` - 5 edges
9. `dossier_test()` - 4 edges
10. `regles()` - 4 edges

## Surprising Connections (you probably didn't know these)
- `App` --references--> `Tri`  [INFERRED]
  src/main.rs → src/tri.rs

## Import Cycles
- None detected.

## Communities (8 total, 3 thin omitted)

### Community 0 - "main.rs"
Cohesion: 0.12
Nodes (5): ANCIENS, App, CREATE_NO_WINDOW, main(), racine()

### Community 1 - "Tri"
Cohesion: 0.20
Nodes (3): developper(), heure(), Tri

### Community 2 - "Regles"
Cohesion: 0.22
Nodes (9): apercu(), categorie(), chemin_libre(), deplacer(), extension(), noms(), ranger(), Regles (+1 more)

### Community 4 - "OutilsRust : tous les outils réécrits en Rust, dans un seul programme"
Cohesion: 0.22
Nodes (7): Graphe, Liste (ordre de réécriture : du plus simple au plus risqué), OutilsRust : tous les outils réécrits en Rust, dans un seul programme, Stack, Compiler, OutilsRust, État

### Community 5 - "ranger_deplace_sans_ecraser"
Cohesion: 0.50
Nodes (5): categories_et_exclusions(), chemin_libre_ne_ecrase_pas(), dossier_test(), ranger_deplace_sans_ecraser(), regles()

## Knowledge Gaps
- **1 isolated node(s):** `outils`
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 27 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **3 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Tri` connect `Tri` to `main.rs`, `Regles`, `tri.rs`?**
  _High betweenness centrality (0.287) - this node is a cross-community bridge._
- **What connects `outils` to the rest of the system?**
  _1 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `main.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.12280701754385964 - nodes in this community are weakly interconnected._
- **Why does `App` connect `main.rs` to `Tri`?**
  _High betweenness centrality (0.172) - this node is a cross-community bridge._
- **Why does `Regles` connect `Regles` to `Tri`, `tri.rs`, `ranger_deplace_sans_ecraser`?**
  _High betweenness centrality (0.040) - this node is a cross-community bridge._