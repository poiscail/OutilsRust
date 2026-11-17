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
| Presse-papiers | fait, pas encore vérifié dans l'appli (même `%APPDATA%\GestionnairePressePapiers`, raccourci de `parametres.json`) |
| Les autres | pas encore : leur onglet ouvre l'ancienne version |

## Compiler

Toolchain `stable-x86_64-pc-windows-gnu` + MinGW (WinLibs, pour `dlltool`/`as`) dans le PATH :

```
cargo test
cargo build --release
```

L'exe est dans `target\release\Outils.exe`.
