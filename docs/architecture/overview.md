# Architecture

The system architecture is modeled in [LikeC4](https://likec4.dev/) (C4 model DSL)
under `docs/architecture/likec4/` — `specification.c4`, `model.c4`, `views.c4`.
This is the structured, diagrammable source of truth; keep it in sync with
reality as containers/components/relationships change (see
`openspec/config.yaml` rules).

No project package manager is set up yet, so there are no `arch:*` scripts.
Once one exists, add `arch:dev` / `arch:validate` / `arch:build` scripts
(see `openspec/config.yaml` context). Until then, use the raw commands:

```bash
bunx likec4@latest start docs/architecture/likec4
bunx likec4@latest validate docs/architecture/likec4
bunx likec4@latest build docs/architecture/likec4 -o docs/architecture/likec4/dist
```
