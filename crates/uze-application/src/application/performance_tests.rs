//! Budget tests for every command `src/command_performance.rs` classifies
//! `Budgeted`, and for the one read the management screens are made of.
//!
//! Each test times a command's warm path — a fresh `UzeApplication`, the
//! way a new invocation is — against a world where anything expensive is
//! visible: the one harness sleeps half a second per detection probe, and
//! the registered marketplace's repository is deleted once the world is
//! built, so a listing that clones cannot answer at all. The ceiling is
//! the best of three runs: it states what the path *can* do, which is
//! what a regression changes, without failing on a scheduler hiccup.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use super::*;
use uze_core::{
    exposure::{ExposureMechanism, ExposurePlan},
    integration::HarnessDetection,
    project::Resource,
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    trust::AlwaysTrust,
};

/// Half of what `specs/cli-performance/spec.md` promises a person (50 ms,
/// release build, their machine): these run unoptimized, in-process, and
/// still measure single-digit milliseconds on a 4-vCPU WSL VM, so the
/// ceiling is set where a regression shows rather than where the promise
/// breaks.
const BUDGET: Duration = Duration::from_millis(25);
/// Any live probe on a warm path costs this, so one is enough to fail the
/// budget on its own, not only the probe counter.
const PROBE_DELAY: Duration = Duration::from_millis(500);
const ATTEMPTS: usize = 3;
const MARKETPLACE: &str = "budget-market";
const PLUGIN: &str = "flow";

struct SlowProbeIntegration {
    probes: Arc<AtomicUsize>,
}

impl IntegrationPort for SlowProbeIntegration {
    fn id(&self) -> &'static str {
        "budget-harness"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities::default()
    }

    fn detect(&self) -> HarnessDetection {
        self.probes.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(PROBE_DELAY);
        HarnessDetection {
            present: true,
            version: Some("1.0.0".to_owned()),
        }
    }

    fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Unsupported,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::Unsupported {
                rationale: "the budget harness delivers nothing".to_owned(),
            },
            evidence: "budget".to_owned(),
        }
    }
}

/// A machine with the official plugin, one Git marketplace with one plugin
/// installed from it, and a project declaring that plugin — every
/// budgeted command has something to read.
struct World {
    root: PathBuf,
    home: UzeHome,
    project: PathBuf,
    probes: Arc<AtomicUsize>,
}

impl World {
    fn build(label: &str) -> Self {
        let root = uze_testkit::temp::scratch(label);
        let home = UzeHome::at(root.join("uze"));
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("AGENTS.md"), "# Budget project\n").unwrap();

        let market = root.join("market");
        let plugin_dir = market.join("plugins").join(PLUGIN);
        copy_tree(&uze_testkit::fixtures::canonical(PLUGIN), &plugin_dir);
        fs::write(
            market.join(uze_core::workspace::MARKETPLACE_MANIFEST_NAME),
            serde_json::json!({
                "name": MARKETPLACE,
                "plugins": [{ "name": PLUGIN, "source": format!("./plugins/{PLUGIN}") }],
            })
            .to_string(),
        )
        .unwrap();
        uze_testkit::git::commit_everything_in(&market);

        let world = Self {
            root,
            home,
            project,
            probes: Arc::new(AtomicUsize::new(0)),
        };
        let app = world.app();
        app.ensure_default_plugins().unwrap();
        app.marketplace()
            .add(&format!("file://{}", market.display()))
            .unwrap();
        app.project()
            .add(PLUGIN, MARKETPLACE, &world.project, &AlwaysTrust)
            .unwrap();
        // From here on the marketplace's repository does not exist: every
        // answer about it below is the cache's, or nothing.
        fs::remove_dir_all(&market).unwrap();
        world
    }

    /// A fresh application, the way each CLI invocation constructs one:
    /// no in-process memo, only what is on disk.
    fn app(&self) -> UzeApplication {
        UzeApplication::new(
            self.home.clone(),
            vec![Box::new(SlowProbeIntegration {
                probes: self.probes.clone(),
            })],
        )
    }

    /// Times `operation` on fresh applications after one untimed warm-up,
    /// and holds the best run to the budget and the warm runs to zero
    /// probes.
    fn within_budget<T>(&self, label: &str, operation: impl Fn(&UzeApplication) -> T) {
        let _ = operation(&self.app());
        let probes_before = self.probes.load(Ordering::SeqCst);
        let best = (0..ATTEMPTS)
            .map(|_| {
                let started = Instant::now();
                let app = self.app();
                let _ = operation(&app);
                started.elapsed()
            })
            .min()
            .expect("at least one attempt");
        eprintln!("{label}: best of {ATTEMPTS} warm runs {best:?}");
        assert_eq!(
            self.probes.load(Ordering::SeqCst),
            probes_before,
            "{label}: a warm run probed the harness"
        );
        assert!(
            best < BUDGET,
            "{label}: best of {ATTEMPTS} warm runs took {best:?}, budget is {BUDGET:?}"
        );
    }

    /// For a mutation, which cannot be repeated in one world: one timed run
    /// on a fresh application, zero probes. The caller takes the best over
    /// several worlds.
    fn timed_once<T>(&self, label: &str, operation: impl FnOnce(&UzeApplication) -> T) -> Duration {
        let probes_before = self.probes.load(Ordering::SeqCst);
        let started = Instant::now();
        let app = self.app();
        let _ = operation(&app);
        let elapsed = started.elapsed();
        assert_eq!(
            self.probes.load(Ordering::SeqCst),
            probes_before,
            "{label}: probed the harness"
        );
        elapsed
    }
}

/// Holds the best of several runs to the budget.
fn assert_best_within_budget(label: &str, runs: &[Duration]) {
    let best = runs.iter().min().expect("at least one run");
    eprintln!("{label}: best of {} runs {best:?}", runs.len());
    assert!(
        *best < BUDGET,
        "{label}: best of {} runs took {best:?}, budget is {BUDGET:?}",
        runs.len()
    );
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Every file under a directory with its length and modification time —
/// what a "writes nothing" claim is checked against.
fn tree_state(root: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut out = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if let Ok(meta) = fs::symlink_metadata(&path) {
                out.push((path, meta.len(), meta.modified().unwrap()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn bootstrap_meets_the_budget_and_writes_nothing_when_warm() {
    let world = World::build("budget-bootstrap");
    world.within_budget("ensure_default_plugins", |app| app.ensure_default_plugins());
    let before = tree_state(world.home.root());
    world.app().ensure_default_plugins().unwrap();
    assert_eq!(
        before,
        tree_state(world.home.root()),
        "a warm bootstrap must leave every file under UZE_HOME untouched"
    );
}

#[test]
fn status_meets_the_budget() {
    let world = World::build("budget-status");
    world.within_budget("status", |app| app.health().status(&world.project));
}

/// The agent's own surface: the read path a naming call takes before its
/// one ref rename, answered from the project's own files plus one Git call.
#[test]
fn the_agent_surface_meets_the_budget() {
    let world = World::build("budget-agent-surface");
    world.within_budget("agent task name", |app| {
        // Refused (this is the primary checkout, which owns no task), which
        // is the same read path a successful naming takes before its one
        // ref rename.
        app.workspace().name_task(&world.project, "fix/budget").ok()
    });
}

#[test]
fn doctor_meets_the_budget() {
    let world = World::build("budget-doctor");
    world.within_budget("doctor", |app| app.health().report());
}

#[test]
fn plugin_list_and_inspect_meet_the_budget() {
    let world = World::build("budget-plugin-reads");
    world.within_budget("plugin list", |app| app.plugins().list());
    world.within_budget("plugin inspect", |app| app.plugins().inspect(PLUGIN));
}

#[test]
fn market_list_and_inspect_meet_the_budget_without_the_repository() {
    let world = World::build("budget-market-reads");
    world.within_budget("market list", |app| app.marketplace().list());
    world.within_budget("market inspect", |app| {
        app.marketplace().inspect(MARKETPLACE)
    });
    let listed = world.app().marketplace().plugins().unwrap();
    assert!(
        listed.iter().any(|plugin| plugin.marketplace == MARKETPLACE
            && plugin.name == PLUGIN
            && plugin.installed),
        "the marketplace's catalogue must still be answered after its repository is gone: {listed:?}"
    );
}

#[test]
fn context_reads_and_reconcile_meet_the_budget() {
    let world = World::build("budget-context");
    world.within_budget("context inspect", |app| {
        app.context().inspect(&world.project)
    });
    world.within_budget("context plan", |app| app.context().plan(&world.project));
    let reconciles: Vec<Duration> = (0..ATTEMPTS)
        .map(|_| {
            world.timed_once("context reconcile", |app| {
                app.context().reconcile(&world.project).unwrap()
            })
        })
        .collect();
    assert_best_within_budget("context reconcile", &reconciles);
}

#[test]
fn removals_meet_the_budget() {
    // Each removal empties what the next one needs, so every attempt is a
    // world of its own.
    let (mut remove, mut plugin_remove, mut market_remove) = (Vec::new(), Vec::new(), Vec::new());
    for attempt in 0..ATTEMPTS {
        let world = World::build(&format!("budget-removals-{attempt}"));
        // Warm every cache the removals read through.
        world.app().health().report();
        remove.push(world.timed_once("remove", |app| {
            app.project().remove(PLUGIN, &world.project).unwrap()
        }));
        plugin_remove
            .push(world.timed_once("plugin remove", |app| app.plugins().remove(PLUGIN).unwrap()));
        market_remove.push(world.timed_once("market remove", |app| {
            app.marketplace().remove(MARKETPLACE).unwrap()
        }));
    }
    assert_best_within_budget("remove", &remove);
    assert_best_within_budget("plugin remove", &plugin_remove);
    assert_best_within_budget("market remove", &market_remove);
}

/// The TUI's management screens are one read model; a refresh is one call.
#[test]
fn machine_snapshot_meets_the_budget() {
    let world = World::build("budget-snapshot");
    world.within_budget("machine snapshot", |app| {
        app.machine_snapshot(&world.project, 20)
    });
}
