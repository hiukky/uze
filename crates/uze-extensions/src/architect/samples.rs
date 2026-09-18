//! The diagrams this proof of concept ships: uze's own architecture,
//! written as the Mermaid an architecture document would hold.
//!
//! Static on purpose. What is being proven is that Mermaid source becomes
//! a readable, navigable diagram in cells; where the source comes from —
//! a fenced block in a project's docs, a graph derived from its manifests
//! — is the next question, and an easier one.

pub struct Sample {
    pub name: &'static str,
    pub source: &'static str,
}

pub const SAMPLES: [Sample; 5] = [
    Sample {
        name: "Crate layering",
        source: LAYERING,
    },
    Sample {
        name: "Install pipeline",
        source: INSTALL_PIPELINE,
    },
    Sample {
        name: "System context",
        source: C4_CONTEXT,
    },
    Sample {
        name: "Containers",
        source: C4_CONTAINERS,
    },
    Sample {
        name: "uze install",
        source: INSTALL_SEQUENCE,
    },
];

const LAYERING: &str = r#"flowchart TD
  subgraph presentation [Presentation · src/]
    cli[CLI<br/>main.rs · clap]
    tui[Workspace TUI<br/>src/ui · ratatui]
    shim[Runtime PATH shim<br/>src/shim.rs]
  end
  ext[uze-extensions<br/>describes, never draws]
  keys[uze-keys<br/>actions · scopes]
  theme[uze-theme<br/>tokens · symbols]
  terminal[uze-terminal<br/>PTY server + protocol]
  app[uze-application<br/>UzeApplication facade]
  subgraph core [uze-core · domain]
    package[package<br/>acquisition · trust · store]
    capability[capability<br/>skill · hook]
    delivery[delivery<br/>router · engine · receipts]
    project[project<br/>lock · context · worktrees]
    machine[machine<br/>home · subprocess · PATH]
  end
  subgraph integrations [uze-integrations]
    registry[registry<br/>composition root]
    verticals[harness verticals<br/>one module per harness]
    shared[shared<br/>process · path helpers]
  end
  git[uze-git<br/>the one Git transport]
  store[(Store · ~/.uze)]

  cli --> app
  tui --> app
  tui --> ext
  tui --> keys
  tui --> terminal
  cli --> theme
  tui --> theme
  shim -.->|sanctioned| machine
  app --> package
  app --> delivery
  app --> project
  app --> registry
  registry --> verticals
  verticals --> shared
  verticals -.->|IntegrationPort| delivery
  delivery --> capability
  delivery --> machine
  package --> git
  project --> git
  package --> store
"#;

const INSTALL_PIPELINE: &str = r#"flowchart LR
  install([uze install]) --> manifest[agents.yaml]
  manifest --> resolve[Resolve marketplaces]
  resolve -->|commit + digest| lock[agents.lock]
  resolve --> acquire[Acquire bytes]
  acquire --> trust{Trusted?}
  trust -- no --> refuse[Refuse]
  trust -- yes --> store[(Store)]
  store --> engine[Engine]
  engine --> router{Router}
  router -->|native| native[Native plugin]
  router -->|generated| generated[Generated package]
  router -->|adapter| adapter[Safe adaptation]
  router -.->|none| unsupported[Unsupported]
  native & generated & adapter --> receipts[(Receipts)]
  receipts ==> context[Reconcile AGENTS.md]
"#;

const C4_CONTEXT: &str = r#"C4Context
  title System context for uze
  Person(dev, "Developer", "Installs plugins once and runs agents side by side")
  Person(agent, "Coding agent", "Launched by uze in a checkout of its own")
  System(uze, "uze", "Compatibility and distribution layer for agentic tooling")
  System_Ext(harness, "Harnesses", "The coding-agent CLIs uze delivers into")
  System_Ext(market, "Marketplaces", "Git repositories holding marketplace.json and plugins")
  SystemDb_Ext(repo, "Project repository", "agents.yaml, agents.lock, AGENTS.md")
  Rel(dev, uze, "Installs, launches, reviews")
  Rel(uze, harness, "Delivers capabilities natively")
  Rel(uze, market, "Clones and resolves", "Git")
  Rel(uze, repo, "Reconciles context")
  Rel(agent, uze, "Names and delivers work", "uze agent")
  Rel(harness, agent, "Runs")
  Rel(agent, repo, "Commits on its branch")
"#;

const C4_CONTAINERS: &str = r#"C4Container
  title Containers of uze
  Person(dev, "Developer", "Works from a terminal")
  System_Boundary(uze, "uze") {
    Container(cli, "CLI", "Rust, clap", "Machine and project commands")
    Container(tui, "Workspace TUI", "Rust, ratatui", "Spaces, agents, panes and the code surface")
    Container(app, "Application", "uze-application", "Orchestrates add, install, remove, update, context")
    Container(core, "Core", "uze-core", "Store, Engine, Router, IntegrationPort")
    Container(integrations, "Integrations", "uze-integrations", "One vertical per harness")
    Container(terminal, "Terminal server", "uze-terminal", "Owns the PTYs so a pane outlives its client")
    ContainerDb(store, "Store", "Filesystem", "Installed package bytes and typed receipts")
  }
  System_Ext(harness, "Harnesses", "The coding-agent CLIs uze delivers into")
  System_Ext(git, "Git", "Marketplaces and the project's own history")
  Rel(dev, cli, "Runs")
  Rel(dev, tui, "Works in")
  Rel(cli, app, "Calls")
  Rel(tui, app, "Reads models from")
  Rel(tui, terminal, "Attaches to", "versioned protocol")
  Rel(app, core, "Drives")
  Rel(app, integrations, "Composes", "registry")
  Rel(integrations, core, "Implements IntegrationPort")
  Rel(core, store, "Owns")
  Rel(core, git, "Reads and writes", "uze-git")
  Rel(integrations, harness, "Projects plugins, hooks, context")
  Rel(terminal, harness, "Hosts", "PTY")
"#;

const INSTALL_SEQUENCE: &str = r#"sequenceDiagram
  actor Dev as Developer
  participant CLI as uze CLI
  participant App as Application
  participant Core as Core · Store
  participant Int as Integration
  participant H as Harness config
  Dev->>CLI: uze install
  CLI->>App: install(project)
  App->>Core: resolve agents.yaml
  Core->>Core: clone marketplace, verify digest
  Core-->>App: packages + agents.lock
  loop each detected harness
    App->>Int: plan(capabilities)
    Int-->>App: native | generated | adapter
    App->>Int: apply(plan)
    Int->>H: write owned artifact
    Int-->>Core: typed receipt
  end
  Note over App: reconcile AGENTS.md
  App-->>CLI: report
  CLI-->>Dev: what changed
"#;
