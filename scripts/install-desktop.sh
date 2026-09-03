#!/usr/bin/env bash
set -euo pipefail

# Install the desktop entry and the app icon, so the compositor has something to
# draw for onehand's windows.
#
# Nothing about a window carries a picture on Linux. The window announces a name
# and the desktop looks that name up among installed entries — Wayland matches
# the entry's file name, X11 matches `StartupWMClass` against `WM_CLASS`. All
# three of those and the window's own `app_id` are the string below, and they
# are compared literally.
#
# The icon is installed into the hicolor theme under that same name rather than
# pointed at by absolute path, so `Icon=` stays a lookup that keeps working when
# this checkout is moved or renamed. An absolute path is how the previous entry
# came to reference a directory that no longer holds this project.

# One name for everything: the entry, the icon lookup, the `StartupWMClass`, the
# binary cargo builds, the icon checked in beside it and the per-user config
# directory. `app_id` here must stay in step with the constant the window
# announces (`crates/app/src/shell.rs`) -- they are compared literally.
#
# A desktop identity is first-come-first-served: two apps announcing one name
# share an entry, an icon and a slot in the dock, and whichever installed last
# overwrote the other. So nothing else may install an entry under this name --
# the front end this one replaced did, and had to be taken off the machine
# before this one could have it.
app_id="onehand"
app_name="Onehand"
project="onehand"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "$script_dir/.." && pwd)"

icon_src="$repo_dir/assets/$project.svg"
binary="$repo_dir/target/release/$project"

data_dir="${XDG_DATA_HOME:-$HOME/.local/share}"
icon_dir="$data_dir/icons/hicolor/scalable/apps"
entry_dir="$data_dir/applications"

if [[ ! -f "$icon_src" ]]; then
    echo "missing icon: $icon_src" >&2
    exit 1
fi

# Checked rather than built: this script installs, and a desktop entry pointing
# at a binary that is not there is the failure it exists to avoid.
if [[ ! -x "$binary" ]]; then
    echo "no release binary at $binary" >&2
    echo "run \`cargo build --release\` first" >&2
    exit 1
fi

mkdir -p "$icon_dir" "$entry_dir"
install -m 0644 "$icon_src" "$icon_dir/$app_id.svg"

cat > "$entry_dir/$app_id.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=$app_name
GenericName=AI Coding Agent Host
Comment=Desktop GUI hosting AI coding agents over ACP
Exec=$binary %F
Icon=$app_id
Terminal=false
Categories=Development;IDE;
Keywords=agent;ai;acp;claude;coding;
StartupWMClass=$app_id
EOF
chmod 0644 "$entry_dir/$app_id.desktop"

# Both caches are advisory: the entry and the icon work without them, they just
# may not appear in a launcher until it rescans.
command -v update-desktop-database >/dev/null && update-desktop-database -q "$entry_dir" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qtf "$data_dir/icons/hicolor" 2>/dev/null || true

echo "Installed $entry_dir/$app_id.desktop"
echo "Installed $icon_dir/$app_id.svg"
