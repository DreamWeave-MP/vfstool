#!/usr/bin/env bash
set -euo pipefail

VFSTOOL="cargo run --manifest-path "$(dirname "$0")/../Cargo.toml" --"
CONFIG="$HOME/.config/openmw"
OUT="$(dirname "$0")/../target/test_output"
ARCH="$HOME/.config/openmw/Mods/Architecture"
PPC="$HOME/.config/openmw/Mods/Performance,Patches,Consistency"
NPCS="$HOME/.config/openmw/Mods/NPCs"

mkdir -p "$OUT"

echo "==> conflicts (yaml)"
$VFSTOOL --config "$CONFIG" conflicts --format yaml \
  > "$OUT/conflicts.yaml"

echo "==> conflicts (json)"
$VFSTOOL --config "$CONFIG" conflicts --format json \
  > "$OUT/conflicts.json"

echo "==> conflicts (toml)"
$VFSTOOL --config "$CONFIG" conflicts --format toml \
  > "$OUT/conflicts.toml"

echo "==> shadowed (yaml)"
$VFSTOOL --config "$CONFIG" shadowed --format yaml \
  > "$OUT/shadowed.yaml"

echo "==> shadowed (json)"
$VFSTOOL --config "$CONFIG" shadowed --format json \
  > "$OUT/shadowed.json"

echo "==> shadowed (toml)"
$VFSTOOL --config "$CONFIG" shadowed --format toml \
  > "$OUT/shadowed.toml"

echo "==> which: Patch for Purists.esm"
$VFSTOOL --config "$CONFIG" which "Patch for Purists.esm" \
  > "$OUT/which_patch_for_purists.txt"

echo "==> which: bookart/barbarian_c_comberry.dds"
$VFSTOOL --config "$CONFIG" which "bookart/barbarian_c_comberry.dds" \
  > "$OUT/which_barbarian_c_comberry.txt"

echo "==> stats"
$VFSTOOL --config "$CONFIG" stats \
  > "$OUT/stats.txt"

echo "==> diff: GlowintheDahrk 00 Core vs 01 Hi Res Window Texture Replacer (yaml)"
$VFSTOOL --config "$CONFIG" diff \
  "$ARCH/GlowintheDahrk/00 Core" \
  "$ARCH/GlowintheDahrk/01 Hi Res Window Texture Replacer" \
  --format yaml \
  > "$OUT/diff_gitd_00_vs_01.yaml"

echo "==> diff: PatchforPurists vs UnofficialMorrowindOfficialPluginsPatched (yaml)"
$VFSTOOL --config "$CONFIG" diff \
  "$PPC/PatchforPurists" \
  "$PPC/UnofficialMorrowindOfficialPluginsPatched" \
  --format yaml \
  > "$OUT/diff_pfp_vs_umop.yaml"

echo "==> diff: Mackon's Humanoid Heads vs ExpressiveEyesforMacKomsHeads (yaml)"
$VFSTOOL --config "$CONFIG" diff \
  "$NPCS/Mackon's Humanoid Heads" \
  "$NPCS/ExpressiveEyesforMacKomsHeads" \
  --format yaml \
  > "$OUT/diff_mackom_vs_expressive_eyes.yaml"

echo ""
echo "All done. Output in $OUT/"
ls -lh "$OUT/"
