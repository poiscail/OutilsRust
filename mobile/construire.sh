#!/usr/bin/env bash
# Assemble l'appli Android : bibliothèque Rust + service Java + icône + manifeste → APK signé.
# Variables : ANDROID_HOME, ANDROID_NDK_ROOT, CLE (fichier .jks), MOT_DE_PASSE.
set -euo pipefail
cd "$(dirname "$0")"

SDK="${ANDROID_HOME:?ANDROID_HOME manquant}"
NDK="${ANDROID_NDK_ROOT:?ANDROID_NDK_ROOT manquant}"
OUTILS="$SDK/build-tools/34.0.0"
JAR="$SDK/platforms/android-34/android.jar"
CLE="${CLE:?CLE manquante (fichier .jks)}"
MDP="${MOT_DE_PASSE:?MOT_DE_PASSE manquant}"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
IFS=. read -r MA MI PA <<<"$VERSION"
CODE=$((16777216 + MA * 65536 + MI * 256 + PA)) # même formule que cargo-apk : les mises à jour s'enchaînent
B=target/apk
rm -rf "$B" && mkdir -p "$B/classes" "$B/dex" "$B/res" "$B/lib/arm64-v8a"

echo "▶ Rust ($VERSION)"
CLANG="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
export CC_aarch64_linux_android="$CLANG/aarch64-linux-android26-clang"
export AR_aarch64_linux_android="$CLANG/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CLANG/aarch64-linux-android26-clang"
cargo build --release --lib --target aarch64-linux-android
cp target/aarch64-linux-android/release/liboutils_mobile.so "$B/lib/arm64-v8a/"
"$CLANG/llvm-strip" "$B/lib/arm64-v8a/liboutils_mobile.so"

echo "▶ Java"
javac -nowarn --release 11 -cp "$JAR" -d "$B/classes" $(find android/java -name '*.java')
"$OUTILS/d8" --release --min-api 26 --lib "$JAR" --output "$B/dex" $(find "$B/classes" -name '*.class')

echo "▶ Ressources et manifeste"
sed -e "s/@VERSION@/$VERSION/" -e "s/@CODE@/$CODE/" android/AndroidManifest.xml > "$B/AndroidManifest.xml"
"$OUTILS/aapt2" compile --dir android/res -o "$B/res.zip"
"$OUTILS/aapt2" link -I "$JAR" --manifest "$B/AndroidManifest.xml" -o "$B/base.apk" "$B/res.zip" --auto-add-overlay

echo "▶ APK"
cp "$B/dex/classes.dex" "$B/"
(cd "$B" && zip -q base.apk classes.dex lib/arm64-v8a/liboutils_mobile.so)
"$OUTILS/zipalign" -f -p 4 "$B/base.apk" "$B/aligne.apk"
"$OUTILS/apksigner" sign --ks "$CLE" --ks-pass "pass:$MDP" --out "$B/Outils.apk" "$B/aligne.apk"
"$OUTILS/apksigner" verify "$B/Outils.apk"
echo "✔ $B/Outils.apk ($VERSION, code $CODE)"
