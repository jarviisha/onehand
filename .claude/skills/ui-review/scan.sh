#!/usr/bin/env bash
# Prints every line a change adds to the UI crates that breaks a token rule or asks for a look.
# Usage: scan.sh [base]  (base defaults to the merge-base with main; the working tree is included)
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
base=${1:-$(git merge-base main HEAD)}
paths=(crates/app/src plugins/builtin crates/plugin-host/src)

# file:line: text, for each line added since `base`, tracked or new.
added() {
  git diff -U0 --no-color "$base" -- "${paths[@]}" | awk '
    /^\+\+\+ / { file = substr($0, 7); next }
    /^@@/ { split($3, a, ","); line = substr(a[1], 2) + 0; next }
    /^\+/ { if (file ~ /\.rs$/) print file ":" line ": " substr($0, 2); line++ }
  '
  git ls-files --others --exclude-standard -- "${paths[@]}" | grep '\.rs$' |
    while read -r f; do awk -v f="$f" '{ print f ":" NR ": " $0 }' "$f"; done
}

lines=$(added | grep -vE '^[^:]*(/tests\.rs|/tests/|guards\.rs):' | grep -vE '^[^:]+:[0-9]+: *//' || true)
found=0
check() { # label, pattern, optional pattern of lines to leave out
  local hits
  hits=$(grep -E "$2" <<<"$lines" | { if [ -n "${3:-}" ]; then grep -vE "$3"; else cat; fi; } || true)
  if [ -n "$hits" ]; then
    printf '\n== %s\n%s\n' "$1" "$hits"
    found=1
  fi
}

check "colour literal (theme.rs owns colours)" \
  '"#[0-9a-fA-F]{3,8}"|[^a-z_](rgb|rgba|hsla)\(' '^crates/app/src/theme\.rs:'
check "unnamed length (name it in a const)" \
  '[^a-z_](px|rems)\( *[0-9]*\.?[0-9]*[1-9]' '^[^:]+:[0-9]+: *(pub(\([^)]*\))? )?(const|static) '
check "spacing off the steps (0p5 1 1p5 2 3 4 6 8)" \
  '\.(p|px|py|pt|pb|pl|pr|m|mx|my|mt|mb|ml|mr|gap|gap_x|gap_y)_(2p5|3p5|5|7|9|10|11|12|16|20|24|32|40|48|56|64|72|80|96)\(\)'
check "type off the scale" \
  '\.text_(2xl|3xl|4xl|5xl|6xl)\(\)|\.font_(bold|extrabold|black|light|extralight|thin)\(\)|\.text_size\( *(gpui::)?rems\('
check "gpui shadow (use lift)" \
  '\.shadow_(2xs|xs|sm|md|lg|xl|2xl)\(\)'
check "fixed corner (use the theme's)" \
  '\.rounded_(xs|sm|md|lg|xl|2xl|3xl)\(\)'
check "round shape: dots and badges only" \
  '\.rounded_full\(\)'
check "thinned token: a hover or a fill made by alpha" \
  '\.(opacity|alpha)\('
check "button outside the wrapper, or a refusal without its cursor" \
  'Button::new\(|\.disabled\('
check "outline: a Tag's alone, no button draws an edge" \
  '\.outline\(\)'
check "accent as ink: the selected fill, not a hue" \
  '(text_color|border_color)\([^)]*\.accent\)'

if [ "$found" = 0 ]; then
  echo "scan: nothing in the lines added since ${base:0:9}"
fi
