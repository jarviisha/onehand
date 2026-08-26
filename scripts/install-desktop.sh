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

app_id="onehand"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "$script_dir/.." && pwd)"

icon_src="$repo_dir/assets/$app_id.svg"
binary="$repo_dir/target/release/$app_id"

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
Name=Onehand
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
