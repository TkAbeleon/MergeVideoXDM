#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PREFIX=${XDG_DATA_HOME:-"$HOME/.local/share"}
BIN_DIR=${XDG_BIN_HOME:-"$HOME/.local/bin"}
APP_DIR="$PREFIX/applications"
ICON_DIR="$PREFIX/icons/hicolor/scalable/apps"

mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR"
install -m 755 "$ROOT/mergevideoxdm" "$BIN_DIR/mergevideoxdm"
install -m 644 "$ROOT/mergevideoxdm.desktop" "$APP_DIR/mergevideoxdm.desktop"
install -m 644 "$ROOT/mergevideoxdm.svg" "$ICON_DIR/mergevideoxdm.svg"

printf '\nMergeVideoXDM a été installé pour l’utilisateur courant.\n'
printf 'Binaire : %s\n' "$BIN_DIR/mergevideoxdm"
printf 'Assure-toi que %s figure dans PATH pour lancer la commande depuis un terminal.\n' "$BIN_DIR"
printf 'FFmpeg et un portail XDG compatible (GTK/KDE) sont nécessaires à l’exécution.\n'
