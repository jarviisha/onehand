#!/usr/bin/env bash
# Open the app as it is, on a data root of its own, with something on every
# surface: three scratch projects (one with uncommitted changes, one on a
# branch, one clean) and the Mock UI agent, whose one prompt plays every block
# kind, a permission and the question cards.
#
# The root is seeded once and then left alone, so conversations and layout
# survive between runs. Delete it to start over:
#   rm -rf "${SHOWCASE_HOME:-$HOME/onehand-showcase}"
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
home=${SHOWCASE_HOME:-$HOME/onehand-showcase}
ws=$home/workspace
projects=$home/projects

commit() {
    git -C "$1" -c user.name=showcase -c user.email=showcase@localhost \
        commit -q -m "$2"
}

if [ ! -e "$home/config.toml" ]; then
    mkdir -p "$ws" "$projects"

    for name in atlas-api dashboard-web infra; do
        dir=$projects/$name
        git init -q -b main "$dir"
        printf '# %s\n\nA scratch project for the onehand showcase.\n' "$name" >"$dir/README.md"
        git -C "$dir" add README.md
        commit "$dir" "init $name"
    done
    printf '\nRetry policy: exponential backoff, capped at 5 attempts.\n' >>"$projects/atlas-api/README.md"
    printf 'pub fn backoff() {}\n' >"$projects/atlas-api/backoff.rs"
    git -C "$projects/dashboard-web" checkout -q -b feat/charts

    cat >"$home/config.toml" <<EOF
[[agents]]
name = "Mock UI"
command = "node"
args = ["$repo/crates/core/examples/mock_ui_agent.js"]
EOF

    cat >"$home/state.toml" <<EOF
recent_workspaces = ["$ws"]
EOF

    cat >"$ws/onehand-workspace.toml" <<EOF
name = "Showcase"
roots = ["$projects/atlas-api", "$projects/dashboard-web", "$projects/infra"]
active_root = 0
EOF
    echo "Seeded $home"
fi

cd "$repo"
# The Telegram token is cleared for the same reason `make dev` clears it: two
# bridges polling one bot each see half its messages.
ONEHAND_CONFIG_DIR="$home" ONEHAND_TELEGRAM_TOKEN= exec cargo run
