#!/bin/sh
# Use this as SHELL for terminal_perf, or execute it inside Kitty/a Terminal dock.
set -eu
: "${PERF_PHASE:?set PERF_PHASE to a writable phase file, e.g. /tmp/terminal.phase}"
perf_script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
case "${PERF_WORKLOAD:-cursor}" in
    cursor) PERF_LUA="$perf_script_dir/terminal-perf.lua" ;;
    snacks)
        : "${PERF_SNACKS_RTP:?set PERF_SNACKS_RTP to the installed snacks.nvim directory}"
        PERF_LUA="$perf_script_dir/terminal-perf-snacks.lua"
        ;;
    seq) exec python3 "$perf_script_dir/terminal-perf-seq.py" ;;
    replay) exec python3 "$perf_script_dir/terminal-perf-replay.py" ;;
    echo) exec python3 "$perf_script_dir/terminal-perf-echo.py" ;;
    *) echo 'PERF_WORKLOAD must be cursor, snacks, seq, replay or echo' >&2; exit 2 ;;
esac
export PERF_LUA
exec "${PERF_NVIM:-nvim}" --clean -n -i NONE -c 'lua dofile(vim.env.PERF_LUA)'
