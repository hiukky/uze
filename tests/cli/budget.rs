//! What the performance budget looks like from outside the process: the
//! real `uze` binary, an isolated home, and claims about the machine it
//! leaves behind rather than about how long it took. The ceilings
//! themselves are held in-process, by
//! `uze_application::application::performance_tests`, where a run is
//! not dominated by process start-up and scheduler noise.

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use uze_testkit::temp::TestEnvironment;

fn uze_bin() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_uze"))
}

/// Every file under `root` with its size and modification time.
fn tree_state(root: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
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

/// A read-only command run again with nothing changed must leave every
/// file under `UZE_HOME` exactly as it found it. Before this held, every
/// command re-recorded each harness and rewrote every catalogue on its way
/// in — synced writes that said what the files already said.
#[test]
fn a_warm_read_only_command_writes_nothing_under_uze_home() {
    let env = TestEnvironment::isolated();
    for command in [&["status"][..], &["doctor"][..], &["plugin", "list"][..]] {
        env.run_ok(uze_bin(), command);
        let before = tree_state(&env.uze_home);
        env.run_ok(uze_bin(), command);
        assert_eq!(
            before,
            tree_state(&env.uze_home),
            "`uze {}` rewrote UZE_HOME on a warm run",
            command.join(" ")
        );
    }
}

/// A marketplace registered by URL is listed from its cached catalogue:
/// the repository can be gone and the listing still answers, which is the
/// only way a listing can be fast — a clone is the alternative, and it
/// was the cost of every refresh of the management screen.
#[test]
fn a_marketplace_registered_by_url_is_listed_without_its_repository() {
    let env = TestEnvironment::isolated();
    let market = env.root().join("remote-market");
    let plugin = uze_testkit::fixtures::canonical("flow");
    copy_tree(&plugin, &market.join("plugins/flow"));
    fs::write(
        market.join("marketplace.json"),
        r#"{"name":"remote","plugins":[{"name":"flow","source":"./plugins/flow"}]}"#,
    )
    .unwrap();
    uze_testkit::git::commit_everything_in(&market);
    env.run_ok(
        uze_bin(),
        &["market", "add", &format!("file://{}", market.display())],
    );
    fs::remove_dir_all(&market).unwrap();

    let list = env.run_ok(uze_bin(), &["market", "list"]);
    let stdout = String::from_utf8_lossy(&list.stdout);
    let row = stdout
        .lines()
        .find(|line| line.contains("remote"))
        .unwrap_or_else(|| panic!("the marketplace must still be listed: {stdout}"));
    assert!(
        row.contains("1 plugin"),
        "the catalogue must come from the cache, with its plugin count: {row}"
    );
    let inspect = env.run_ok(uze_bin(), &["market", "inspect", "remote"]);
    let detail = String::from_utf8_lossy(&inspect.stdout);
    assert!(
        detail.contains("Plugins") && detail.lines().any(|line| line.trim() == "1"),
        "inspecting the marketplace reads the cached catalogue: {detail}"
    );
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
