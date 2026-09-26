//! Budget tests for every command `src/command_performance.rs` classifies
//! `Budgeted`, and for the one read the management screens are made of.
//!
//! Its own test target, and that is the whole point: a budget is a claim
//! about a path, and a wall clock inside the crate's lib-test binary
//! measures the path *plus* the 186 sibling tests sharing its thread pool,
//! several of which spawn Git. That reads as a regression on a loaded
//! machine and passes on an idle one. Cargo runs test targets one after
//! another, so a target holding nothing but these measures what it claims
//! to.
//!
//! What each test asserts is exact and the same everywhere: a warm path
//! takes no live harness probe and no reach for the Git binary. The clock
//! is a backstop beside those — see `BUDGET`.
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
        Arc, Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use uze_application::application::UzeApplication;
use uze_core::{
    UzeHome,
    capability::Resource,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::{HarnessDetection, IntegrationPort},
    router::{CompatibilityRoute, HarnessCapabilities},
    trust::AlwaysTrust,
};

/// A backstop, not the claim.
///
/// What these tests exist to catch is a warm path going *cold* — a live
/// harness probe, a clone, a `rev-parse` per installed package. Every one
/// of those is a discrete event, and both are counted exactly below: zero
/// probes and zero reaches for the Git binary. That is the claim, and it
/// is the same on every machine.
///
/// The clock cannot be. These run unoptimized, and the same warm read
/// measures 8 ms and 40 ms on one idle 4-vCPU VM and several times that on
/// a CI macOS runner — so a tight ceiling here fails on the machine rather
/// than on the code, which is the one thing a gate must never do. It is
/// set instead where *something expensive happened*: below one probe
/// (`PROBE_DELAY`), so a path that goes cold in a way the counters somehow
/// miss still fails, and far enough above the work that a slow runner
/// does not.
///
/// `specs/cli-performance/spec.md` promises a person 50 ms on a release
/// build. That promise is not what is measured here and never was.
const BUDGET: Duration = Duration::from_millis(250);
/// Any live probe on a warm path costs this, so one is enough to fail the
/// budget on its own, not only the probe counter.
const PROBE_DELAY: Duration = Duration::from_millis(500);
const ATTEMPTS: usize = 3;

/// Held for the length of every timed run, so that within this target one
/// budget is measured at a time.
///
/// A dedicated target keeps the rest of the workspace off the clock; this
/// keeps these ten off each other's. A thread waiting here is blocked, not
/// spinning, so what it costs the measurement is nothing.
static METER: Mutex<()> = Mutex::new(());

/// The lock, taken whether or not a previous holder panicked: a poisoned
/// meter means an earlier budget failed, which the run already reports.
fn meter() -> MutexGuard<'static, ()> {
    METER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
/// Counts the spans `uze-git` opens, one per invocation of the Git binary.
#[derive(Clone, Default)]
struct GitCalls(Arc<AtomicUsize>);

impl<S> tracing_subscriber::Layer<S> for GitCalls
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _context: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if attrs.metadata().name() == "git" {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// How many times `body` reached for the Git binary.
fn git_calls<T>(body: impl FnOnce() -> T) -> (T, usize) {
    use tracing_subscriber::layer::SubscriberExt;
    let calls = GitCalls::default();
    let subscriber = tracing_subscriber::registry().with(calls.clone());
    let out = tracing::subscriber::with_default(subscriber, body);
    (out, calls.0.load(Ordering::SeqCst))
}

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

    fn exposure_plan(&self, _resource: &Resource) -> ExposurePlan {
        ExposurePlan {
            route: CompatibilityRoute::Unsupported,
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
        uze_testkit::fixtures::copy_tree(&uze_testkit::fixtures::canonical(PLUGIN), &plugin_dir);
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
            .add(
                PLUGIN,
                MARKETPLACE,
                &world.project,
                &AlwaysTrust,
                &uze_application::NoNameCollisionAuthority,
            )
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
        let measuring = meter();
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
        let (_, calls) = git_calls(|| operation(&self.app()));
        eprintln!("{label}: best of {ATTEMPTS} warm runs {best:?}, {calls} git call(s)");
        assert_eq!(
            self.probes.load(Ordering::SeqCst),
            probes_before,
            "{label}: a warm run probed the harness"
        );
        // The exact half of the budget. A warm read answers from what is
        // on this disk; reaching for Git is how one of these paths has
        // gone cold before — "one `rev-parse` per installed package put
        // the machine snapshot over its budget" — and a count says so on
        // every machine, which a clock does not.
        assert_eq!(
            calls, 0,
            "{label}: a warm read reached for the Git binary {calls} time(s)"
        );
        drop(measuring);
        assert_within_budget(label, best, ATTEMPTS);
    }

    /// For a mutation, which cannot be repeated in one world: one timed run
    /// on a fresh application, zero probes. The caller takes the best over
    /// several worlds.
    fn timed_once<T>(&self, label: &str, operation: impl FnOnce(&UzeApplication) -> T) -> Duration {
        let _measuring = meter();
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
    let best = *runs.iter().min().expect("at least one run");
    eprintln!("{label}: best of {} runs {best:?}", runs.len());
    assert_within_budget(label, best, runs.len());
}

/// The budget, held wherever the clock is measuring this code.
///
/// It is not, under `cargo llvm-cov`: every binary is instrumented and the
/// whole workspace runs at once, so the number describes the run rather
/// than the path — the coverage job timed this same `doctor` at 117 ms
/// where an ordinary run of it measures under two, instrumented included.
/// The budget is a claim about a path, so it is asserted where the
/// measurement is of one: both `Test` jobs run these tests uninstrumented,
/// on Linux and on macOS, and `make test` does the same locally.
///
/// What actually catches a warm path going cold — that it took no live
/// probe — is asserted by the caller either way, instrumented or not.
fn assert_within_budget(label: &str, best: Duration, runs: usize) {
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        eprintln!("{label}: instrumented for coverage, so the clock is not held to the budget");
        return;
    }
    assert!(
        best < BUDGET,
        "{label}: best of {runs} runs took {best:?}, budget is {BUDGET:?}"
    );
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
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
        // Refused (no record names this claim), which is the same read
        // path a successful naming takes before its one ref rename.
        app.workspace()
            .name_task(
                uze_core::conversation::Claim {
                    id: "budget",
                    cwd: &world.project,
                },
                "fix/budget",
            )
            .ok()
    });
}

/// `config notification` is one `config.toml` key read or written — a
/// choice a person makes standing at a prompt, never a read of the machine.
#[test]
fn notification_choice_meets_the_budget() {
    let world = World::build("budget-notification");
    world.within_budget("config notification", |app| {
        app.notifications()
            .set_agent_finished(uze_application::Chime::OutOfSight)
            .unwrap();
        assert_eq!(
            app.notifications().agent_finished().unwrap(),
            uze_application::Chime::OutOfSight
        );
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
fn market_host_meets_the_budget() {
    let root = uze_testkit::temp::scratch("budget-market-host");
    let home = UzeHome::at(root.join("uze"));
    let application = || UzeApplication::new(home.clone(), Vec::new());

    let runs: Vec<Duration> = (0..ATTEMPTS)
        .map(|_| {
            let _measuring = meter();
            let app = application();
            let started = Instant::now();
            app.marketplace()
                .define_host("work", "https://git.acme.io")
                .unwrap();
            app.marketplace().set_default_host("work").unwrap();
            assert!(!app.marketplace().hosts().unwrap().is_empty());
            app.marketplace().remove_host("work").unwrap();
            started.elapsed()
        })
        .collect();
    assert_best_within_budget("market host", &runs);
    let _ = fs::remove_dir_all(root);
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

/// Linking and unlinking are a registry write and a dropped cache entry.
/// `link` also asks Git what repository the checkout is, which is one local
/// call — so neither has any excuse to leave the budget.
///
/// Built on its own world because `World::build` deletes the marketplace's
/// repository, and a link must point at one that exists.
#[test]
fn market_link_and_unlink_meet_the_budget() {
    let root = uze_testkit::temp::scratch("budget-market-link");
    let home = UzeHome::at(root.join("uze"));
    let market = root.join("market");
    let plugin_dir = market.join("plugins").join(PLUGIN);
    uze_testkit::fixtures::copy_tree(&uze_testkit::fixtures::canonical(PLUGIN), &plugin_dir);
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

    let application = || UzeApplication::new(home.clone(), Vec::new());
    application()
        .marketplace()
        .add(&format!("file://{}", market.display()))
        .unwrap();

    let links: Vec<Duration> = (0..ATTEMPTS)
        .map(|_| {
            let _measuring = meter();
            let app = application();
            let started = Instant::now();
            app.marketplace().link(MARKETPLACE, &market).unwrap();
            started.elapsed()
        })
        .collect();
    assert_best_within_budget("market link", &links);

    let unlinks: Vec<Duration> = (0..ATTEMPTS)
        .map(|_| {
            let app = application();
            app.marketplace().link(MARKETPLACE, &market).unwrap();
            let _measuring = meter();
            let fresh = application();
            let started = Instant::now();
            fresh.marketplace().unlink(MARKETPLACE).unwrap();
            started.elapsed()
        })
        .collect();
    assert_best_within_budget("market unlink", &unlinks);

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn context_reads_and_reconcile_meet_the_budget() {
    let world = World::build("budget-context");
    world.within_budget("agent context inspect", |app| {
        app.context().inspect(&world.project)
    });
    world.within_budget("agent context plan", |app| {
        app.context().plan(&world.project)
    });
    let reconciles: Vec<Duration> = (0..ATTEMPTS)
        .map(|_| {
            world.timed_once("agent context reconcile", |app| {
                app.context().reconcile(&world.project).unwrap()
            })
        })
        .collect();
    assert_best_within_budget("agent context reconcile", &reconciles);
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
