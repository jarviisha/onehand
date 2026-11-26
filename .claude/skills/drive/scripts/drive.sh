#!/usr/bin/env bash
# Act on the app session.sh started.
#
#   drive.sh click X Y | move X Y | scroll X Y N   pointer, in output pixels
#   drive.sh hover X Y NAME [click]                rest there (pressing first), shot as it rests
#   drive.sh type TEXT                             keys into the focused field
#   drive.sh key ARGS…                             wtype arguments, e.g. -k Escape
#   drive.sh shot NAME                             screenshot to $DRIVE_ROOT/NAME.png
#   drive.sh size W [H]                            the window's size in output pixels: W at
#                                                  least 720 and H 480, the app's least; H 1000
#   drive.sh matrix NAME [STEPS]                   NAME-W-P.png at each width, light then dark
#   drive.sh alive                                 is the app still running? exit 1 if not
set -euo pipefail
root=${DRIVE_ROOT:-/tmp/onehand-drive}
export WAYLAND_DISPLAY; WAYLAND_DISPLAY=$(cat "$root/display")
usage() { sed -n '2,12p' "$0"; exit 2; }
here=$(cd "$(dirname "$0")" && pwd)
# vpclick places a point on a 1600x1000 extent stretched over the output,
# whatever its size: an output pixel is scaled to it, so a coordinate read
# off a screenshot lands at any width.
at() {
    local w=1600 h=1000
    if [ -f "$root/size" ]; then read -r w h <"$root/size"; fi
    echo $((($1 * 1600 + w / 2) / w)) $((($2 * 1000 + h / 2) / h))
}
# Stop, with the log's tail, once the app is gone.
alive_or_stop() { "$here/drive.sh" alive >/dev/null || { "$here/drive.sh" alive || true; exit 1; }; }
case ${1:-} in
    click | move) read -r x y <<<"$(at "$2" "$3")"; "$root/vpclick" "$x" "$y" "$1" ;;
    scroll) read -r x y <<<"$(at "$2" "$3")"; "$root/vpclick" "$x" "$y" scroll "${4:-3}" ;;
    # The pointer kept alive past a tooltip's delay, and the shot taken
    # while it rests: once it is gone nothing counts as hovered.
    hover)
        read -r x y <<<"$(at "$2" "$3")"
        "$root/vpclick" "$x" "$y" "${5:-move}" 2500 &
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
        (( $2 >= 720 && ${3:-1000} >= 480 )) || usage
        swaymsg -s "/run/user/$(id -u)/sway-ipc.$(id -u).$(cat "$root/sway.pid").sock" \
            output HEADLESS-1 mode "$2x${3:-1000}" >/dev/null
        echo "$2 ${3:-1000}" >"$root/size"
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
            # A step that fails stops the matrix, saying its line.
            if [ -n "${3:-}" ]; then
                # shellcheck disable=SC2016 # expanded by the inner bash
                bash -eE -o pipefail -c 'trap '\''echo "step failed: line $LINENO of $0" >&2'\'' ERR; . "$0"' "$(realpath "$3")"
            fi
            alive_or_stop
            # Widest first, so the steps' layout is the first one shot.
            for w in $(printf '%s\n' "${widths[@]}" | sort -rn); do
                "$DRIVE" size "$w"
                "$DRIVE" shot "$2-$w-$palette"
                alive_or_stop
            done
        done
        "$DRIVE" size 1600 ;;
    alive)
        if kill -0 "$(cat "$root/app.pid")" 2>/dev/null; then echo alive; else
            echo dead; grep -v MESA "$root/app.log" | tail -20; exit 1
        fi ;;
    *) usage ;;
esac
