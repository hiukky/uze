#!/bin/sh
set -eu

: "${UZE_E2E_SKILL_PROOF:?set a per-run skill proof}"
: "${UZE_E2E_MCP_PROOF:?set a per-run MCP proof}"

workspace=/work/project
source=/work/source/plugin-first-conformance
config_root="${XDG_CONFIG_HOME:-$HOME/.config}"

mkdir -p "$HOME" "$UZE_HOME" "$workspace" "$config_root/opencode" /work/source
cp -a /opt/opencode-runtime/. "$config_root/opencode/"
cp -a /opt/uze-fixtures/plugin-first-conformance "$source"

# Materialize one external package for this disposable run. The source fixture
# baked into the image remains untouched, while both capability proofs are
# generated outside the prompts the model receives.
export source UZE_E2E_SKILL_PROOF UZE_E2E_MCP_PROOF
node <<'NODE'
const fs = require("fs");
const path = require("path");
const root = process.env.source;
const replace = (relative, from, to) => {
  const file = path.join(root, relative);
  fs.writeFileSync(file, fs.readFileSync(file, "utf8").replaceAll(from, to));
};
replace("skills/uze-plugin-first/SKILL.md", "__UZE_SKILL_PROOF__", process.env.UZE_E2E_SKILL_PROOF);
for (const name of ["mcp.json", ".mcp.json"]) {
  replace(name, "__UZE_MCP_FIXTURE_BINARY__", "/usr/local/bin/uze-mcp-conformance-fixture");
}
const config = {
  "$schema": "https://opencode.ai/config.json",
  provider: {
    "uze-gateway": {
      npm: "@ai-sdk/openai-compatible",
      name: "UZE Conformance LiteLLM",
      options: { baseURL: "http://gateway:4000/v1", apiKey: "not-required-inside-isolated-lab" },
      models: { "uze-conformance": { name: "UZE Conformance" } },
    },
  },
};
fs.writeFileSync(
  path.join(process.env.HOME, ".config/opencode/opencode.json"),
  JSON.stringify(config, null, 2),
);
NODE

uze setup opencode >/work/setup.txt
uze add "$source" >/work/add.txt

export UZE_MCP_CONFORMANCE_PROOF="$UZE_E2E_MCP_PROOF"
# The selected model is fully declared in the disposable config above. Avoid
# OpenCode's optional models.dev catalog refresh: the harness has no direct
# egress and must only reach the internal gateway.
export OPENCODE_DISABLE_MODELS_FETCH=1
export OPENCODE_MODELS_PATH=/opt/uze-e2e/opencode-models.json

# OpenCode occasionally stalls during its own local bootstrap, before it
# ever reaches the gateway (no request appears in the gateway's own logs
# during a stall). This reproduces independent of provider and model, so
# retry a small bounded number of times rather than accept it as a false
# negative on the behavioral proof this script exists to gate.
run_with_proof() {
  prompt="$1"
  proof="$2"
  out_file="$3"
  attempt=1
  status=1
  while test "$attempt" -le 3; do
    output="$(cd "$workspace" && timeout 90 opencode run --pure --print-logs --log-level DEBUG --model uze-gateway/uze-conformance "$prompt" 2>&1)"
    status=$?
    printf '%s\n' "$output" > "$out_file"
    if test "$status" -eq 0 && printf '%s' "$output" | grep -F -- "$proof" >/dev/null; then
      return 0
    fi
    attempt=$((attempt + 1))
  done
  return 1
}

if ! run_with_proof 'Use the installed uze-plugin-first skill to prove plugin-first portability. Return its proof token exactly.' "$UZE_E2E_SKILL_PROOF" /work/skill-output.txt; then
  printf '%s\n' 'opencode_skill_e2e=failed' >&2
  cat /work/skill-output.txt >&2
  exit 1
fi

if ! run_with_proof 'You have an MCP tool available whose name starts with "uze-" and ends with "_uze_conformance". Find it in your tool list and call it now. It is not a shell command and it is not the uze-plugin-first skill; do not use bash and do not use the skill. Return exactly the proof value the tool call supplies.' "$UZE_E2E_MCP_PROOF" /work/mcp-output.txt; then
  printf '%s\n' 'opencode_mcp_e2e=failed' >&2
  cat /work/mcp-output.txt >&2
  exit 1
fi

printf 'opencode_skill_e2e=verified\n'
printf 'opencode_mcp_e2e=verified\n'
