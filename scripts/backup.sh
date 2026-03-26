#!/usr/bin/env bash
set -euo pipefail

RCLONE="/home/deck/sd/apps/cartouche/bin/rclone"
REMOTE="ludusavi-1763680370"

SRC1="/run/media/deck/SteamDeck-SD/cartouche-backup"
DST1="Machines/SteamDeck/cartouche-backup"

SRC2="/run/media/deck/SteamDeck-SD/linux-games"
DST2="Machines/SteamDeck/games"

"$RCLONE" copy "$SRC1" "${REMOTE}:${DST1}" --progress
"$RCLONE" copy "$SRC2" "${REMOTE}:${DST2}" --progress
