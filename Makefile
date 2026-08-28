.DEFAULT_GOAL := help

CARGO ?= cargo
UZE_BIN ?= target/debug/uze
RELEASE_BIN ?= target/release/uze
INSTALL_ARGS ?= --force

.PHONY: help build release install wsl-lab run test test-acceptance test-conformance test-installer harness-test harness-matrix check fmt lint coverage version clean changelog lab-image lab-run lab-evidence lab-sandbox lab-experiment lab-matrix lab-replay python-fmt python-lint

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

wsl-lab: ## Build the release here and deploy binary + playground plugin into the WSL distro named Lab.
	./playground/install-wsl-distro.sh Lab

run: build ## Run the debug binary; pass arguments with `ARGS="doctor"`.
	$(UZE_BIN) $(ARGS)

test: ## Run the default Rust unit and contract suite.
	$(CARGO) test --no-fail-fast

test-acceptance: ## Run the L3 acceptance suite (the release signal).
	$(CARGO) test -p uze --test acceptance

test-conformance: ## Run integration conformance + per-harness semantics.
	$(CARGO) test -p uze --test integrations

test-installer: ## Exercise install.sh offline against a synthetic release (Linux).
	sh tests/scripts/installer-test.sh

harness-test: ## L2 probes that need real vendor binaries (skip cleanly when absent).
	$(CARGO) test -p uze --test integrations real_codex_dogfood -- --ignored 2>/dev/null || \
	$(CARGO) test -p uze --test integrations real_codex_dogfood

harness-matrix: ## Regenerate the README harness matrix (used by lefthook's --check).
	$(CARGO) run --quiet --bin uze-harness-matrix

fmt: ## Check formatting (cargo fmt --check).
	$(CARGO) fmt --check

lint: ## Lint with clippy, warnings denied.
	$(CARGO) clippy --all-targets -- -D warnings

python-fmt: ## Check Python formatting with ruff (conformance/).
	ruff format --check conformance/

python-lint: ## Lint Python with ruff (conformance/).
	ruff check conformance/

check: fmt lint test python-fmt python-lint ## Local proxy for the CI gate; also cargo-release's pre-release-hook.


# --- Harness Conformance Lab (Python, Real Harness + Synthetic World) ---
# The Lab runs the per-harness verticals in the disposable Docker image
# (`conformance-harness:latest`): real harness binary + synthetic provider,
# zero Internet, zero tokens. HARNESS selects one harness id
# (antigravity | claude | codex | opencode).
HARNESS ?= antigravity
LAB_IMAGE ?= conformance-harness:latest

lab-image: ## Build the Lab harness image (installs channel-latest harnesses).
	docker build -f conformance/Dockerfile -t $(LAB_IMAGE) .

lab-run: ## Run the isolation vertical for $(HARNESS) (3x clean is the gate; gate enforced per ADR-035).
	python3 conformance/lab.py --harness $(HARNESS)

lab-evidence: ## Record the in-repo evidence summary for $(HARNESS) (ADR-035).
	python3 conformance/lab.py --harness $(HARNESS) --write-summary

lab-sandbox: ## Interactive sandbox for $(HARNESS): recorded TUI session (or shell with SHELL=1); -- cmd... for scripted commands.
	python3 conformance/lab.py --harness $(HARNESS) --sandbox $(if $(SHELL),--shell,)

lab-experiment: ## Run an experiment scenario outside the canonical suite (EXPERIMENT=vendor/name; optional VARIATION=spec).
	python3 conformance/lab.py --harness $(HARNESS) --experiment $(EXPERIMENT) $(if $(VARIATION),--variation $(VARIATION),)

lab-matrix: ## Cross-harness compatibility matrix over VARIANTS (default conformance/variants.json).
	python3 conformance/lab.py --matrix $(if $(VARIANTS),$(VARIANTS),conformance/variants.json) $(if $(HARNESSES),--harnesses $(HARNESSES),)

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
