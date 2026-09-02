# Makefile for onehand — wraps the cargo workflows documented in CLAUDE.md.
#
# Usage:
#   make run                 # run with the current dir as the project root
#   make run ROOT=~/code/x   # run with a specific project root
#   make test T=changed_line # run tests matching a name substring
#   make smoke ACP_CMD="node examples/mock_terminal_agent.js"

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

# Formatting and linting stop at onehand's own crates.
#
# `vendor/gpui-terminal` is a workspace member, so a bare `cargo fmt` reformats
# it and `clippy --fix` rewrites it — hundreds of lines of churn on upstream
# code, none of it a change onehand meant to make. The vendor's whole value is
# that its diff against `zortax/gpui-terminal@51f0292` is exactly the patches we
# wrote and nothing else (CLAUDE.md "Gotchas"). Its own lint warnings are
# upstream's and stay put.
OURS := -p onehand -p onehand-core

.DEFAULT_GOAL := help

.PHONY: help run release-run build release check test fmt fmt-check clippy lint smoke desktop clean

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*## "}; {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

run: ## Run the app (ROOT=/path/to/project seeds the workspace root)
	$(CARGO) run -- $(ROOT)

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
