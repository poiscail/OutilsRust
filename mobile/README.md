# Outils sur téléphone (Android)

Les jeux et les outils du quotidien d'Outils, sur Android. **Le code est partagé avec la version PC** :
`src/lib.rs` inclut directement `../src/minijeux*.rs`, `ludique.rs`, `quotidien2.rs` et `quotidien3.rs`.
Ce qui n'existe que sur Windows y est entouré de `#[cfg(windows)]`, et de petits modules (`outil`,
`creatif`, `systeme`, `telephone`, `minecraft`, `shims/rfd`) remplacent ceux du PC.

Adaptations au tactile :
- grandes tuiles, favoris (appui long) et récents ;
- manette tactile et pavé de chiffres qui envoient de vraies touches aux jeux ;
- clavier à l'écran ⌨ pour tous les champs de texte ;
- zoom automatique quand un plateau pensé pour un écran de PC est plus large que le téléphone.

## Compiler l'APK

Il faut le SDK Android (platforms;android-34, build-tools;34.0.0), le NDK (r28) et `cargo-apk` :

```bash
rustup target add aarch64-linux-android
cargo install cargo-apk
export ANDROID_HOME=/chemin/vers/sdk ANDROID_NDK_ROOT=/chemin/vers/android-ndk-r28c
export CARGO_APK_RELEASE_KEYSTORE=/chemin/vers/outils.jks CARGO_APK_RELEASE_KEYSTORE_PASSWORD=…
cargo apk build --release      # → target/release/apk/Outils.apk
```

Pour essayer sur PC sans téléphone : `cargo run --example bureau`.

La clé de signature (`outils.jks`) n'est pas dans le dépôt : il faut toujours la même pour que les
mises à jour s'installent par-dessus l'ancienne version.
