#!/usr/bin/env bash
# Act on the app session.sh started.
#
#   drive.sh click X Y | move X Y | scroll X Y N   pointer, in output pixels
#   drive.sh hover X Y NAME [click]                rest there (pressing first), shot as it rests
#   drive.sh type TEXT                             keys into the focused field
#   drive.sh key ARGS…                             wtype arguments, e.g. -k Escape
#   drive.sh shot NAME                             screenshot to $DRIVE_ROOT/NAME.png
#   drive.sh size W [H]                            the window's size, in output pixels (H 1000)
#   drive.sh matrix NAME [STEPS]                   NAME-W-P.png at each width, in each palette
#   drive.sh alive                                 is the app still running?
set -euo pipefail
root=${DRIVE_ROOT:-/tmp/onehand-drive}
export WAYLAND_DISPLAY; WAYLAND_DISPLAY=$(cat "$root/display")
usage() { sed -n '2,11p' "$0"; exit 2; }
here=$(cd "$(dirname "$0")" && pwd)
case ${1:-} in
    click | move) "$root/vpclick" "$2" "$3" "$1" ;;
    scroll) "$root/vpclick" "$2" "$3" scroll "${4:-3}" ;;
    # The pointer kept alive past a tooltip's delay, and the shot taken
    # while it rests: once it is gone nothing counts as hovered.
    hover)
        "$root/vpclick" "$2" "$3" "${5:-move}" 2500 &
        sleep 1.8
        grim -o HEADLESS-1 "$root/$4.png" && echo "$root/$4.png"
        wait ;;
    # Each wtype is a keyboard of its own, made and dropped; a click sent
    # before the window has settled from it can be lost, so wait a little.
    type) wtype "$2"; sleep 0.3 ;;
    key) shift; wtype "$@"; sleep 0.3 ;;
    shot) grim -o HEADLESS-1 "$root/$2.png" && echo "$root/$2.png" ;;
    # The window fills the headless output, so the output's mode is its
    # size; it changes live, and the app lays out again within a second.
    size)
        [[ ${2:-} =~ ^[0-9]+$ && ${3:-1000} =~ ^[0-9]+$ ]] || usage
        swaymsg -s "/run/user/$(id -u)/sway-ipc.$(id -u).$(cat "$root/sway.pid").sock" \
            output HEADLESS-1 mode "$2x${3:-1000}" >/dev/null
        sleep 1 ;;
    # One view in both palettes and at each width of $DRIVE_WIDTHS. A
    # palette starts the app again, so the view is reached anew by STEPS, a
    # bash file of `$DRIVE …` lines run at the wide size, where its
    # coordinates were read; only then is the window narrowed.
    matrix)
        [ -n "${2:-}" ] || usage
        [ -z "${3:-}" ] || [ -f "$3" ] || { echo "no steps file $3" >&2; exit 2; }
        export DRIVE="$here/drive.sh"
        read -ra widths <<<"${DRIVE_WIDTHS:-720 1600}"
        for palette in light dark; do
            "$here/session.sh" palette "$palette"
            "$DRIVE" size 1600
            sleep 1
            if [ -n "${3:-}" ]; then bash "$3"; fi
            # Widest first, so the steps' layout is the first one shot.
            for w in $(printf '%s\n' "${widths[@]}" | sort -rn); do
                "$DRIVE" size "$w"
                "$DRIVE" shot "$2-$w-$palette"
            done
            "$DRIVE" alive >/dev/null || { "$DRIVE" alive; exit 1; }
        done
        "$DRIVE" size 1600 ;;
    alive) kill -0 "$(cat "$root/app.pid")" 2>/dev/null && echo alive || { echo dead; grep -v MESA "$root/app.log" | tail -20; } ;;
    *) usage ;;
esac
