//! Acceptance A8: runtime shim active — UZE's own shims dir precedes the
//! real executable on PATH, yet internal harness resolution must reach the
//! real binary and never recurse into the shim.

use std::path::Path;

use uze::UzeHome;
use uze_testkit::fake_harness::FakeHarness;
use uze_testkit::temp::TestEnvironment;

use crate::util::uze_bin;

/// A poisoned shim: it records every invocation (so a recursion is provable)
/// and answers with a bogus version. Sits in UZE's own shims dir, ahead of
/// the real-looking fake on PATH — the exact hazard after `uze setup`.
fn poison_shim(env: &TestEnvironment, name: &str, poison_version: &str) -> std::path::PathBuf {
    let shims = UzeHome::at(&env.uze_home).shims_dir();
    let marker = env.root().join(format!("shim-invoked-{name}"));
    let script = format!(
        "#!/bin/sh\necho 'poison' >> '{}'\necho '{poison_version}'\n",
        marker.display()
    );
    let path = shims.join(name);
    create_executable(&path, &script);
    path
}

fn create_executable(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, script).unwrap();
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[cfg(unix)]
#[test]
fn runtime_shim_active_internal_calls_resolve_real_executable_without_recursion() {
    let env = TestEnvironment::isolated();

    // Real-looking harness binaries (the "real" side of the boundary).
    for (name, version) in [
        ("claude", "9.9.9 (Real Claude)"),
        ("codex", "codex-cli 9.9.9"),
        ("opencode2", "opencode2 v9.9.9"),
        ("agy", "agy 9.9.9"),
    ] {
        let _ = FakeHarness::new(&env.fake_bin, name)
            .version_line(version)
            .build();
    }
    // Poisoned UZE shims for every vendor, first on PATH.
    for (name, poison) in [
        ("claude", "POISON (should never be read) 0.0.1"),
        ("codex", "codex-cli POISON"),
        ("opencode", "opencode2 vPOISON"),
        ("agy", "1.0.0-POISON"),
    ] {
        poison_shim(&env, name, poison);
    }

    // Run the real uze with PATH = shims : fake_bin : system bins. The
    // shim scripts must stay ahead of everything else (the real hazard),
    // but `/bin/sh` must remain reachable for the fake executables.
    let shims = UzeHome::at(&env.uze_home).shims_dir();
    let path = format!(
        "{}:{}:/usr/bin:/bin",
        shims.display(),
        env.fake_bin.display()
    );
    let output = env
        .command(uze_bin())
        .env("PATH", &path)
        .args(["doctor"])
        .output()
        .expect("uze doctor must run");
    assert!(
        output.status.success(),
        "doctor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("POISON"),
        "doctor must never read the poisoned shim output, got: {stdout}"
    );
    // The fake "real" binaries must have been invoked: detection went
    // through them, not the shims (doctor's text does not print versions).
    for name in ["claude", "codex", "opencode2", "agy"] {
        let log = env
            .fake_bin
            .join(".invocations")
            .join(format!("{name}.log"));
        assert!(
            std::fs::read_to_string(&log).is_ok(),
            "the real-looking fake {name} was never invoked — doctor fell back to the shim"
        );
    }
    for name in ["claude-code", "codex", "opencode", "antigravity"] {
        assert!(
            stdout.contains(name) && stdout.contains("detected: true"),
            "doctor must detect {name} through the real-looking fake, got: {stdout}"
        );
    }

    // No recursion: none of the shims ever ran.
    for name in ["claude", "codex", "opencode", "agy"] {
        let marker = env.root().join(format!("shim-invoked-{name}"));
        assert!(
            !marker.exists(),
            "shim {name} was invoked — internal harness resolution recursed into the runtime shim"
        );
    }
}
