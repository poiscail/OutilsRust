#!/bin/sh
# Installe Outils pour l'utilisateur courant (pas besoin d'être administrateur).
# Pour désinstaller : ./installer.sh --retirer
set -e
cd "$(dirname "$0")"
BIN="$HOME/.local/bin"
APPS="$HOME/.local/share/applications"
ICONES="$HOME/.local/share/icons/hicolor/512x512/apps"
if [ "$1" = "--retirer" ]; then
    rm -f "$BIN/outils" "$APPS/outils.desktop" "$ICONES/outils.png"
    echo "Outils est désinstallé (tes réglages restent dans ~/.config/outils)."
    exit 0
fi
mkdir -p "$BIN" "$APPS" "$ICONES"
install -m 755 outils "$BIN/outils"
install -m 644 outils.png "$ICONES/outils.png"
sed "s|@BIN@|$BIN/outils|" outils.desktop > "$APPS/outils.desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" 2>/dev/null || true
echo "✔ Outils est installé : cherche « Outils » dans le menu des applications, ou tape « outils » dans un terminal."
