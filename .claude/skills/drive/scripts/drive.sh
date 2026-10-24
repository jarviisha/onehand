#!/usr/bin/env bash
# Act on the app session.sh started.
#
#   drive.sh click X Y | move X Y | scroll X Y N   pointer, in output pixels
#   drive.sh type TEXT                             keys into the focused field
#   drive.sh key ARGS…                             wtype arguments, e.g. -k Escape
#   drive.sh shot NAME                             screenshot to $DRIVE_ROOT/NAME.png
#   drive.sh alive                                 is the app still running?
set -euo pipefail
root=${DRIVE_ROOT:-/tmp/onehand-drive}
export WAYLAND_DISPLAY; WAYLAND_DISPLAY=$(cat "$root/display")
case ${1:-} in
    click | move) "$root/vpclick" "$2" "$3" "$1" ;;
    scroll) "$root/vpclick" "$2" "$3" scroll "${4:-3}" ;;
    # Each wtype is a keyboard of its own, made and dropped; a click sent
    # before the window has settled from it can be lost, so wait a little.
    type) wtype "$2"; sleep 0.3 ;;
    key) shift; wtype "$@"; sleep 0.3 ;;
    shot) grim -o HEADLESS-1 "$root/$2.png" && echo "$root/$2.png" ;;
    alive) kill -0 "$(cat "$root/app.pid")" 2>/dev/null && echo alive || { echo dead; grep -v MESA "$root/app.log" | tail -20; } ;;
    *) sed -n '2,9p' "$0"; exit 2 ;;
esac
