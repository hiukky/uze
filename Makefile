.DEFAULT_GOAL := help

CARGO ?= cargo
UZE_BIN ?= target/debug/uze
RELEASE_BIN ?= target/release/uze
INSTALL_ARGS ?= --force

.PHONY: help build release install install-wsl-lab playground-lab run test test-acceptance test-conformance test-real-harness docs-harness-matrix check fmt lint coverage version clean changelog

help: ## Show the available local-development targets.
	@awk 'BEGIN {FS = ":.*##"} /^[a-zA-Z0-9_.-]+:.*##/ { printf "  %-12s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

build: ## Build the debug UZE binary for local development.
	$(CARGO) build --locked --bin uze

release: ## Build the optimized UZE binary for a real local installation.
	$(CARGO) build --locked --release --bin uze

version: ## Print the single workspace version carried by the UZE binary.
	$(CARGO) run --quiet --bin uze -- --version

changelog: ## Regenerate CHANGELOG.md from Conventional Commits (git-cliff; see cliff.toml).
	git-cliff -o CHANGELOG.md

install: ## Force-rebuild (no version bump) and install/replace `uze` in Cargo's configured binary directory.
	$(CARGO) install --path . --bin uze --locked $(INSTALL_ARGS)

install-wsl-lab: ## Build release here and install it into the WSL distro named Lab.
	./playground/install-wsl-distro.sh Lab

playground-lab: install-wsl-lab ## Deploy the current binary and default plugin into Lab.

run: build ## Run the debug binary; pass arguments with `ARGS="doctor"`.
	$(UZE_BIN) $(ARGS)

test: ## Run the default Rust unit and contract suite.
	$(CARGO) test --no-fail-fast

test-acceptance: ## Run the L3 acceptance suite (the release signal).
	$(CARGO) test -p uze --test acceptance

test-conformance: ## Run integration conformance + per-harness semantics.
	$(CARGO) test -p uze --test integrations

test-real-harness: ## Run L2 probes that need real vendor binaries (skip cleanly when absent).
	$(CARGO) test -p uze --test integrations real_codex_dogfood -- --ignored 2>/dev/null || \
	$(CARGO) test -p uze --test integrations real_codex_dogfood

fmt: ## Format Rust sources.
	$(CARGO) fmt

lint: ## Run Clippy with warnings treated as errors.
	$(CARGO) clippy -- -D warnings

check: ## Run formatting, linting, and tests.
	$(CARGO) fmt --check
	$(CARGO) clippy -- -D warnings
	$(CARGO) test --no-fail-fast

docs-harness-matrix: ## Regenerate the deterministic harness×feature matrix in README.md.
	$(CARGO) run --quiet --bin uze-harness-matrix

coverage: ## Run coverage and enforce 65%→70%→90% roadmap (see CI).
	$(CARGO) llvm-cov --workspace --summary-only --fail-under-lines 64 --fail-under-regions 65 --html

clean: ## Remove local Cargo build artifacts.
	$(CARGO) clean
