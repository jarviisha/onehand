#!/usr/bin/env bash
# Start or stop onehand in a headless sway session of its own.
#
#   session.sh up [BIN]   seed the data root once, start sway and the app
#                         (BIN defaults to the repo's target/debug/onehand)
#   session.sh restart    stop the app only, wipe tasks and conversations,
#                         reset the project, start the app again
#   session.sh down       stop the app and sway
#
# Everything lives under $DRIVE_ROOT (default /tmp/onehand-drive). Processes
# are stopped by the pids written there, never by name: the person's own
# onehand runs under the same name.
set -euo pipefail
root=${DRIVE_ROOT:-/tmp/onehand-drive}
here=$(cd "$(dirname "$0")" && pwd)
repo=$(git -C "$here" rev-parse --show-toplevel)
run=/run/user/$(id -u)
mkdir -p "$root"

stop() { # stop pid-file $1: TERM, then KILL if it is still there
    [ -f "$root/$1" ] || return 0
    local pid; pid=$(cat "$root/$1")
    kill "$pid" 2>/dev/null || true
    for _ in 1 2 3 4 5 6; do kill -0 "$pid" 2>/dev/null || break; sleep 0.5; done
    kill -9 "$pid" 2>/dev/null || true
    rm -f "$root/$1"
}

seed() {
    [ -e "$root/home/config.toml" ] && return 0
    mkdir -p "$root/home" "$root/ws" "$root/proj"
    git -C "$root/proj" init -q -b main
    printf '# demo\n' >"$root/proj/README.md"
    git -C "$root/proj" add README.md
    git -C "$root/proj" -c user.name=drive -c user.email=drive@localhost commit -qm init
    cat >"$root/home/config.toml" <<TOML
[[agents]]
name = "Mock Work"
command = "node"
args = ["$here/mock_work_agent.js"]

[[agents]]
name = "Mock UI"
command = "node"
args = ["$repo/crates/core/examples/mock_ui_agent.js"]
TOML
    printf 'recent_workspaces = ["%s"]\n' "$root/ws" >"$root/home/state.toml"
    cat >"$root/ws/onehand-workspace.toml" <<TOML
name = "Drive"
roots = ["$root/proj"]
active_root = 0

[checks]
"$root/proj" = "true"
TOML
}

build_pointer() {
    [ -x "$root/vpclick" ] && return 0
    wayland-scanner client-header "$here/virtual-pointer.xml" "$root/virtual-pointer.h"
    wayland-scanner private-code "$here/virtual-pointer.xml" "$root/virtual-pointer.c"
    gcc -O1 -I"$root" -o "$root/vpclick" "$here/vpclick.c" "$root/virtual-pointer.c" -lwayland-client
}

start_sway() {
    if [ -f "$root/sway.pid" ] && kill -0 "$(cat "$root/sway.pid")" 2>/dev/null; then return 0; fi
    printf 'output HEADLESS-1 resolution 1600x1000\ndefault_border none\n' >"$root/sway.conf"
    local before; before=$(ls "$run" | grep -E '^wayland-[0-9]+$' || true)
    env -u WAYLAND_DISPLAY -u DISPLAY WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 \
        WLR_RENDERER=pixman sway -c "$root/sway.conf" >"$root/sway.log" 2>&1 &
    echo $! >"$root/sway.pid"
    for _ in $(seq 20); do
        local now; now=$(ls "$run" | grep -E '^wayland-[0-9]+$' || true)
        local new; new=$(comm -13 <(echo "$before" | sort) <(echo "$now" | sort) | head -1)
        if [ -n "$new" ]; then echo "$new" >"$root/display"; return 0; fi
        sleep 0.25
    done
    echo "sway did not come up; see $root/sway.log" >&2; exit 1
}

start_app() {
    local bin=${1:-$repo/target/debug/onehand}
    # `cd` on its own line: with `cd … && app &`, the whole list goes to the
    # background and `$!` is a shell, not the app, so a stop misses the app.
    (
        cd "$repo"
        env -u DISPLAY RUST_BACKTRACE=1 WAYLAND_DISPLAY="$(cat "$root/display")" \
            ONEHAND_CONFIG_DIR="$root/home" ONEHAND_TELEGRAM_TOKEN= "$bin" >"$root/app.log" 2>&1 &
        echo $! >"$root/app.pid"
    )
    local sock="$run/sway-ipc.$(id -u).$(cat "$root/sway.pid").sock"
    for _ in $(seq 60); do
        swaymsg -s "$sock" -t get_tree 2>/dev/null | grep -q '"app_id": "onehand"' && return 0
        kill -0 "$(cat "$root/app.pid")" 2>/dev/null || { tail -5 "$root/app.log" >&2; exit 1; }
        sleep 0.5
    done
    echo "no onehand window; see $root/app.log" >&2; exit 1
}

case ${1:-} in
    up) seed; build_pointer; start_sway; stop app.pid; start_app "${2:-}" ;;
    restart)
        stop app.pid
        rm -rf "$root/home/tasks" "$root/home/conversations"
        git -C "$root/proj" checkout -q -- . && git -C "$root/proj" clean -qfd
        start_app "${2:-}" ;;
    down) stop app.pid; stop sway.pid ;;
    *) sed -n '2,13p' "$0"; exit 2 ;;
esac
