.DEFAULT_GOAL := help

CARGO ?= cargo
UZE_BIN ?= target/debug/uze
RELEASE_BIN ?= target/release/uze
INSTALL_ARGS ?= --force

.PHONY: help build release install install-wsl-lab playground-lab run test test-acceptance test-conformance test-real-harness docs-harness-matrix check fmt lint coverage version clean changelog lab-image lab-run lab-replay

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


# --- Harness Conformance Lab (Python, Real Harness + Synthetic World) ---
# The Lab runs the per-harness verticals in the disposable Docker image
# (`conformance-harness:latest`): real harness binary + synthetic provider,
# zero Internet, zero tokens. HARNESS selects one harness id
# (antigravity | claude | codex | opencode).
HARNESS ?= antigravity
LAB_IMAGE ?= conformance-harness:latest

lab-image: ## Build the Lab harness image (installs channel-latest harnesses).
	docker build -f conformance/Dockerfile -t $(LAB_IMAGE) .

lab-run: ## Run the isolation vertical for $(HARNESS) (3x clean is the gate).
	python3 conformance/lab.py --harness $(HARNESS)

lab-replay: ## Replay the most recent recorded TUI session (rendered correctly, ANSI intact).
	@watch="$${LAB_REPLAY:-$$(ls -t /tmp/harness-conformance/*/run*/tui.typescript 2>/dev/null | head -n 1)}"; \
	recent="$$(ls -dt /tmp/harness-conformance/*/run* 2>/dev/null | head -n 1)"; \
	if [ -z "$$watch" ] || [ ! -f "$$watch" ]; then \
		echo "no recorded TUI session found under /tmp/harness-conformance — run the lab first:"; \
		echo "  make lab-run HARNESS=antigravity|claude|codex|opencode"; \
		if [ -n "$$recent" ]; then \
			if [ -f "$$recent/verdict.json" ]; then \
				echo "  (most recent run dir: $$recent)"; \
			else \
				harness="$$(basename "$$(dirname "$$recent")")"; \
				echo "  ($$recent did not complete — re-run: make lab-run HARNESS=$$harness)"; \
			fi; \
		fi; \
		exit 1; \
	fi; \
	echo "replaying $$watch"; \
	scriptreplay --timing "$${watch%.typescript}.timing" "$$watch"
test-real-harness: ## Run L2 probes that need real vendor binaries (skip cleanly when absent).
	$(CARGO) test -p uze --test integrations real_codex_dogfood -- --ignored 2>/dev/null || \
	$(CARGO) test -p uze --test integrations real_codex_dogfood
