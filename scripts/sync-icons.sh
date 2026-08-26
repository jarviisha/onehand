#!/usr/bin/env bash
set -euo pipefail

# Refetch every checked-in SVG from its pinned upstream.
#
# The manifest holds brand marks only: UI glyphs come from gpui-component's own
# bundled set, reached through its `IconName` enum, so there is nothing here to
# fetch for them. That is why this script knows one provider.

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "$script_dir/.." && pwd)"
icon_dir="$repo_dir/assets/icons"
manifest="$icon_dir/manifest.toml"

value_from_manifest() {
    local key="$1"
    sed -n "s/^${key} = \"\(.*\)\"$/\1/p" "$manifest"
}

simple_icons_version="$(value_from_manifest simple_icons_version)"

if [[ -z "$simple_icons_version" ]]; then
    echo "icon manifest is missing a source version" >&2
    exit 1
fi

sync_tmp="$(mktemp -d "${TMPDIR:-/tmp}/onehand-icons.XXXXXX")"
cleanup() {
    rm -rf -- "$sync_tmp"
}
trap cleanup EXIT

staged_dir="$sync_tmp/staged"
mkdir "$staged_dir"

awk '
    /^\[icons\]$/ { in_icons = 1; next }
    in_icons && /^\[/ { in_icons = 0 }
    in_icons && /^[a-z0-9-]+[[:space:]]*=/ {
        line = $0
        gsub(/[[:space:]"]/, "", line)
        split(line, pair, "=")
        print pair[1] "\t" pair[2]
    }
' "$manifest" | while IFS=$'\t' read -r local_name source_spec; do
    provider="${source_spec%%:*}"
    upstream_name="${source_spec#*:}"
    output="$staged_dir/${local_name}.svg"

    case "$provider" in
        simple-icons)
            curl -fsSL --retry 3 --max-time 30 \
                "https://raw.githubusercontent.com/simple-icons/simple-icons/${simple_icons_version}/icons/${upstream_name}.svg" \
                -o "$output"
            ;;
        *)
            echo "unknown icon provider: $provider" >&2
            echo "UI glyphs come from gpui-component's IconName, not from here." >&2
            exit 1
            ;;
    esac

done

# Publish only after every source file has been downloaded.
cp "$staged_dir"/*.svg "$icon_dir/"

echo "Synced brand marks from Simple Icons ${simple_icons_version}."
