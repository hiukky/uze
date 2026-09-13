.DEFAULT_GOAL := help

CARGO ?= cargo
UZE_BIN ?= target/debug/uze
RELEASE_BIN ?= target/release/uze
INSTALL_ARGS ?= --force

.PHONY: help build release install wsl-lab run test test-acceptance test-conformance test-installer harness-matrix check ci fmt lint deny msrv web audit secrets installer attributions attributions-check coverage version clean changelog release-notes lab-image lab-run lab-evidence lab-sandbox lab-experiment lab-matrix lab-replay python-fmt python-lint

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

release-notes: ## Preview the GitHub Release page for the latest tag (cliff.release.toml); set GITHUB_TOKEN for contributor handles.
	git-cliff --config cliff.release.toml --latest

# `--features telemetry` sits outside INSTALL_ARGS on purpose: a local
# install is a developer's own binary, and one built without the exporter
# ignores OTEL_EXPORTER_OTLP_ENDPOINT in silence — nothing reports that
# the traces are going nowhere. Release binaries stay lean; release.yml
# passes no features.
install: ## Force-rebuild (no version bump) and install/replace `uze`, with the OTLP exporter compiled in.
	$(CARGO) install --path . --bin uze --locked --features telemetry $(INSTALL_ARGS)

wsl-lab: ## Build the release here and deploy binary + playground plugin into the WSL distro named Lab.
	./playground/install-wsl-distro.sh Lab

run: build ## Run the debug binary; pass arguments with `ARGS="doctor"`.
	$(UZE_BIN) $(ARGS)

test: ## Run the default Rust unit and contract suite.
	$(CARGO) test --workspace --no-fail-fast

test-acceptance: ## Run the L3 acceptance suite (the release signal).
	$(CARGO) test -p uze --test acceptance

test-conformance: ## Run integration conformance + per-harness semantics.
	$(CARGO) test -p uze --test integrations

test-installer: ## Exercise install.sh offline against a synthetic release (Linux).
	sh tests/scripts/installer-test.sh

harness-matrix: ## Regenerate the docs harness matrix (used by lefthook's --check).
	$(CARGO) run --quiet --bin uze-harness-matrix

observe: ## Start the local Jaeger (UI on :16686, OTLP on :4318) and print the endpoint to trace against.
	@docker compose up -d
	@echo "Jaeger UI:  http://localhost:16686"
	@echo "Trace with: OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 cargo run --features telemetry --bin uze -- <command>"

observe-stop: ## Stop the local Jaeger and drop what it collected.
	@docker compose down

test-telemetry: ## Run the telemetry module's own tests with the OTLP exporter compiled in.
	cargo test --features telemetry --lib telemetry

fmt: ## Check formatting (cargo fmt --check).
	$(CARGO) fmt --check

lint: ## Lint with clippy, warnings denied.
	$(CARGO) clippy --all-targets -- -D warnings

deny: ## Audit dependency licences, advisories, bans and sources (cargo-deny).
	$(CARGO) deny check
	$(CARGO) deny --all-features check

attributions: ## Regenerate CREDITS.md from about.hbs + Cargo.lock (cargo-about).
	$(CARGO) about generate about.hbs -o CREDITS.md

attributions-check: ## Fail if CREDITS.md is stale relative to Cargo.lock.
	$(CARGO) about generate about.hbs -o /tmp/uze-credits-check.md
	diff -u CREDITS.md /tmp/uze-credits-check.md || \
		{ printf 'CREDITS.md is stale - run `make attributions` and commit.\n' >&2; exit 1; }

msrv: ## Build on the MSRV declared in Cargo.toml (needs `rustup toolchain install 1.97`).
	$(CARGO) +1.97 check --workspace --all-targets --locked

web: ## Typecheck and build the documentation site (needs bun).
	cd web && bun install --frozen-lockfile && bun run types:check && bun run build

audit: ## Check the dependency tree against the RustSec advisory database.
	$(CARGO) audit

secrets: ## Scan the whole history for credentials (needs gitleaks).
	gitleaks git --no-banner --redact=4 --config .gitleaks.toml

installer: ## Lint install.sh and exercise it offline against a synthetic release.
	shellcheck install.sh
	sh tests/scripts/installer-test.sh

python-fmt: ## Check Python formatting with ruff (conformance/).
	ruff format --check conformance/

python-lint: ## Lint Python with ruff (conformance/).
	ruff check conformance/

coverage: ## Run workspace tests with LLVM coverage (skips the one env-dependent test).
	cargo llvm-cov --workspace --summary-only --fail-under-lines 68 --fail-under-regions 69 -- --skip foreground_status_reports
	cargo llvm-cov report --lcov --output-path lcov.info

check: fmt lint deny test test-telemetry python-fmt python-lint ## Local proxy for the CI gate; also cargo-release's pre-release-hook.

# GitHub publishes no offline runner (`actions/runner` is for self-hosted and
# is still driven by GitHub), so the honest local mirror is the commands
# themselves. `ci` is every job in ci.yml except the Lab, which has `lab-run`,
# and Coverage, which has `coverage`. What no local run can reproduce: the
# release attestation (it needs GitHub's OIDC identity), the path filters that
# decide whether the Lab runs at all, and the rulesets.
ci: check msrv web audit attributions-check secrets installer coverage ## Every fast CI job, locally.


# --- Harness Conformance Lab (Python, Real Harness + Synthetic World) ---
# The Lab runs the per-harness verticals in the disposable Docker image
# (`conformance-harness:latest`): real harness binary + synthetic provider,
# zero Internet, zero tokens. HARNESS selects one harness id
# (antigravity | claude | codex | opencode).
JOURNEY ?= journeys/suites
JOURNEY_IMAGE ?= uze-journeys:latest

journey: build ## Run a product journey on this machine (JOURNEY=<spec>).
	python3 journeys/journey.py run $(JOURNEY)

journey-probe: build ## Open a journey's world and leave it up to inspect by hand.
	python3 journeys/journey.py probe $(JOURNEY)

journey-image: ## Build the pinned journey runtime image (tmux, git, python).
	docker build -f journeys/Dockerfile -t $(JOURNEY_IMAGE) journeys/

journey-docker: build journey-image ## Run a journey inside the pinned container, against this build.
	$(CARGO) build --locked -p uze-testkit --bin uze-fake-harness
	mkdir -p journeys/.evidence
	docker run --rm --init \
		--user "$$(id -u):$$(id -g)" -e HOME=/tmp/journey-home \
		-v "$(CURDIR)/journeys:/journeys:ro" \
		-v "$(CURDIR)/target/debug/uze:/usr/local/bin/uze:ro" \
		-v "$(CURDIR)/target/debug/uze-fake-harness:/usr/local/bin/uze-fake-harness:ro" \
		-v "$(CURDIR)/journeys/.evidence:/evidence" \
		$(JOURNEY_IMAGE) run /journeys/$(patsubst journeys/%,%,$(JOURNEY))

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
