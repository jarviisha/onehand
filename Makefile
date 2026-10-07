# Makefile for onehand — wraps the project's cargo workflows.
#
# Usage:
#   make run                 # run with the current dir as the project root
#   make run ROOT=~/code/x   # run with a specific project root
#   make test T=changed_line # run tests matching a name substring
#   make smoke ACP_CMD="node examples/mock_terminal_agent.js"
#   make dev                 # a debug build beside the app in use, on ~/onehand-dev

CARGO ?= cargo
ROOT  ?=
T     ?=

# Extra arguments for clippy — CI passes `-- -D warnings` through here.
#
# Not `RUSTFLAGS=-D warnings`, which is the usual spelling and is wrong for this
# workspace: cargo applies RUSTFLAGS to every crate it compiles, dependencies
# included, so a warning in somebody else's 600-crate graph would fail our lint
# run. Passing the flag after `--` scopes it to the crates clippy is linting.
CLIPPY_EXTRA ?=

# Where `make dev` keeps its data root: config, conversations, tasks, state.
#
# A second onehand cannot share the root of the one in use: the instance lock
# refuses it, and one that got past would write over the other's state. The
# first run seeds `config.toml` from the root in use, so the agents are there;
# the Telegram token is not copied and its variable is cleared, since two
# bridges polling one bot each see half its messages.
DEV_HOME ?= $(HOME)/onehand-dev
LIVE_CONFIG := $(or $(XDG_CONFIG_HOME),$(HOME)/.config)/onehand/config.toml

# Formatting and linting stop at onehand's own crates.
#
# `vendor/gpui-terminal` is a workspace member, so a bare `cargo fmt` reformats
# it and `clippy --fix` rewrites it — hundreds of lines of churn on upstream
# code, none of it a change onehand meant to make. The vendor's whole value is
# that its diff against `zortax/gpui-terminal@51f0292` is exactly the patches we
# wrote and nothing else. Its own lint warnings are
# upstream's and stay put.
OURS := -p onehand -p onehand-core -p onehand-plugin-api -p onehand-plugin-host \
	-p onehand-terminal-ui -p onehand-connector-github -p onehand-remote-telegram \
	-p onehand-workbench-editor \
	-p onehand-workbench-files -p onehand-workbench-issues -p onehand-workbench-markdown -p onehand-workbench-neovim -p onehand-workbench-plugins

.DEFAULT_GOAL := help

.PHONY: help run dev release-run build release check test fmt fmt-check clippy lint smoke desktop clean

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*## "}; {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

run: ## Run the app (ROOT=/path/to/project seeds the workspace root)
	$(CARGO) run -- $(ROOT)

dev: ## Run a debug build beside the app in use, on a data root of its own (DEV_HOME)
	@mkdir -p "$(DEV_HOME)"
	@if [ ! -e "$(DEV_HOME)/config.toml" ] && [ -e "$(LIVE_CONFIG)" ]; then \
		cp "$(LIVE_CONFIG)" "$(DEV_HOME)/config.toml"; \
		echo "Seeded $(DEV_HOME)/config.toml from $(LIVE_CONFIG)"; \
	fi
	ONEHAND_CONFIG_DIR="$(DEV_HOME)" ONEHAND_TELEGRAM_TOKEN= $(CARGO) run -- $(ROOT)

release-run: ## Run the release build
	$(CARGO) run --release -- $(ROOT)

build: ## Debug build
	$(CARGO) build

release: ## Release build (LTO on; binary at target/release/onehand)
	$(CARGO) build --release

check: ## Fast type-check
	$(CARGO) check

test: ## Run tests (T=substring runs a subset, e.g. make test T=changed_line_only)
	$(CARGO) test $(T)

fmt: ## Format onehand's crates (never vendor/ — see OURS above)
	$(CARGO) fmt $(OURS)

fmt-check: ## Check formatting without writing
	$(CARGO) fmt $(OURS) --check

clippy: ## Lint onehand's crates with clippy
	# `--no-deps`: gpui-terminal is a path dependency *and* a workspace member,
	# so without it clippy reports the vendor's upstream warnings on every run
	# and a real one has nine to hide behind.
	$(CARGO) clippy $(OURS) --all-targets --no-deps $(CLIPPY_EXTRA)

lint: fmt-check clippy ## Formatting check + clippy

smoke: ## Headless ACP smoke test (ACP_CMD=… overrides the adapter)
	$(CARGO) run -p onehand-core --example acp_smoke

desktop: ## Install the desktop entry + app icon (needs `make release` first)
	./scripts/install-desktop.sh

clean: ## Remove build artifacts
	$(CARGO) clean
