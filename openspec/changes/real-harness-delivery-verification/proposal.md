## Why

The user-flow test of the authoring surface ran in a sandbox with no harness
detected, so the four real harness projections were exercised for the first
time on the operator's own machine — where the two delivery bugs of this
branch (`${PLUGIN_ROOT}` vs `${CLAUDE_PLUGIN_ROOT}`, the MCP stub the scaffold
never wrote) surfaced as live Errors in opencode and Claude. A clean, complete
real-harness verification across claude, codex, opencode and antigravity is
the pending gate.

## Tasks

- [ ] 1.1 `make install` from this branch (binário com os fixes: stub do MCP
      executável, `${CLAUDE_PLUGIN_ROOT}` no envelope do claude, path
      resolvido no opencode)
- [ ] 1.2 Limpar o estado do teste: `uze remove hello -m` (drift que bloquear,
      `uze doctor` mostra o quê e por quê); sobra de `~/uze/boo-market`,
      `~/boo`, entradas vendor, se houver
- [ ] 1.3 As 4 verticals do Lab com harnesses reais, mundo sintético, zero
      internet: `python3 conformance/lab.py --harness <claude|codex|opencode|antigravity>`
      — skills, agents, MCP e remoção, um check nomeado por resultado
- [ ] 1.4 O que falhar vira fix com o loop `--sandbox` (seconds, não o run
      completo), guiado pelo skill `conformance-debug`; `verdict.json` é a
      evidência
- [ ] 1.5 Deferidos desta sessão, em ordem: S1 (description como scalar YAML
      duplo-quoted + check recusando bloco truncado), resíduos do detach
      (generated trees órfãs, `.git/uze-write.lock`), truncamento do output
      de `update`, seção `Delivery` vazia no install

## Queu e onde a evidência já está

- O review completo do PR (20 achados, arquivo:linha) foi entregue nesta
  sessão; o fluxo de usuário que achou os bugs de projeção está no transcript
  (subagente autor-persona, mundo `/tmp/uze-user-test`).
- `make lab-replay` cobre o replay; `conformance-stability.yml` é o gate
  noturno de promoção.
