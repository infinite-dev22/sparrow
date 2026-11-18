#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LUCIDE="$ROOT/icons/lucide"
TABLER="$ROOT/icons/tabler"
DARTS="$ROOT/icons/dart-flutter"

mkdir -p "$LUCIDE" "$TABLER" "$DARTS"

# Pinned so the icon set doesn't change underneath you (bump deliberately).
LUCIDE_VER="1.64.0"
TABLER_VER="3.50.0"
SIMPLE_VER="16.36.0"

FAILED=()

fetch() {  # $1 = source URL, $2 = dest path
  echo "→ $2"
  if ! curl -fsSL --retry 3 --retry-delay 1 "$1" -o "$2"; then
    rm -f "$2"
    FAILED+=("$1")
  fi
}

# ── Lucide (24×24, stroke-2, currentColor) ──
LUCIDE_BASE="https://unpkg.com/lucide-static@${LUCIDE_VER}/icons"
for icon in \
  menu link radio-tower circle-alert plus x circle \
  triangle-alert circle-dot bug bug-play bug-off terminal unplug \
  play pause square step-forward \
  refresh-cw file files arrow-up arrow-down arrow-left arrow-right \
  chevron-down chevron-right chevron-left chevrons-down chevrons-up \
  chevrons-down-up git-branch git-fork git-merge network \
  folder folder-open folder-closed panel-left panel-right panel-bottom \
  panel-left-close panel-right-close panel-bottom-close \
  split-square-horizontal split-square-vertical \
  search search-x replace replace-all case-sensitive whole-word regex \
  settings puzzle keyboard palette file-text combine \
  maximize minimize circle-x save trash-2 undo-2 redo-2 \
  copy clipboard-paste scissors calendar clock bell filter \
  boxes plug smartphone list-tree zap circle-check info \
  external-link download upload eye eye-off lock unlock \
  more-horizontal more-vertical grip-vertical; do
  fetch "$LUCIDE_BASE/$icon.svg" "$LUCIDE/$icon.svg"
done

# ── Tabler (debug/step controls) ──
# Tabler has no step-over icon; Lucide's step-forward (above) is the stand-in.
TABLER_BASE="https://unpkg.com/@tabler/icons@${TABLER_VER}/icons/outline"
for icon in \
  player-play player-pause player-stop player-skip-forward \
  player-skip-back player-track-next player-track-prev \
  step-into step-out \
  bug bug-off; do
  fetch "$TABLER_BASE/$icon.svg" "$TABLER/$icon.svg"
done

# ── Dart / Flutter brand (Simple Icons, single-colour SVG) ──
SIMPLE_BASE="https://unpkg.com/simple-icons@${SIMPLE_VER}/icons"
fetch "$SIMPLE_BASE/dart.svg"    "$DARTS/dart_logo.svg"
fetch "$SIMPLE_BASE/flutter.svg" "$DARTS/flutter_logo.svg"
# The remaining Dart/Flutter-specific icons are hand-drawn by you and
# checked in alongside these.

if [ "${#FAILED[@]}" -gt 0 ]; then
  echo "✗ ${#FAILED[@]} download(s) failed:" >&2
  printf '  %s\n' "${FAILED[@]}" >&2
  exit 1
fi

echo "✓ Done. Drop the manual Dart/Flutter icons into $DARTS/"