<div align="center">

# uze

**Install once. Native everywhere.**

[![CI](https://img.shields.io/badge/CI-passing-8fd19e?style=flat-square&labelColor=1e1f20)](https://github.com/hiukky/uze/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.97%2B-7d97c9?style=flat-square&labelColor=1e1f20)](Cargo.toml)
[![License](https://img.shields.io/badge/license-Apache_2.0-A22136?style=flat-square&labelColor=1e1f20)](LICENSE)
[![Status](https://img.shields.io/badge/status-alpha-e0b567?style=flat-square&labelColor=1e1f20)](https://uze-hiukky.vercel.app)

A compatibility layer for agent tooling: install a plugin once, share one
project context, and every harness — Claude, Codex, OpenCode,
Antigravity — gets it through its own most native surface. Then run them
side by side, each agent in an isolated checkout of its own.

<p align="center">
  <img src="web/public/uze-demo.gif" alt="The uze terminal running a Claude Code agent and an OpenCode agent at once, each on its own branch in its own checkout" width="860" />
</p>

```sh
curl -fsSL https://uze.hiukky.com/i | sh
```

**[Full documentation →](https://uze-hiukky.vercel.app)**

</div>

## Roadmap

- [x] Harness management · Skills & MCP portability · Project context · Marketplace · TUI
- [x] Agent & hook portability · Native package delivery
- [x] Profiles · Environment maintenance · Terminal workspace with isolated agents
- [ ] Requirements & dependencies · Security & trust
- [ ] Packaged releases · Runtime context projection · Migration tooling · Ecosystem expansion

---

Built in Rust. Licensed under the Apache License 2.0.
Contributions are welcome under the rules in [CONTRIBUTING.md](CONTRIBUTING.md).
Security reports go through [SECURITY.md](SECURITY.md), never a public issue.

**Trademarks.** Claude and Claude Code, Codex, Gemini and Antigravity, and
OpenCode are trademarks or marks of their respective owners. uze is an
independent project and is not affiliated with, endorsed by, or sponsored by
any of them. Their names appear here only to identify the software uze
interoperates with.

Author: [Romullo Sousa (hiukky)](https://github.com/hiukky) · [Apache License 2.0](LICENSE)

<p align="center">
  <sub>Built with 🖤 by <a href="https://hiukky.com">Hiukky</a>
  <br/>
</p>
