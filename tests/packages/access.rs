//! How a marketplace is reached: anonymously when it is public, with the
//! operator's own credentials when it is private, always on the host its
//! identity names — against a forge on loopback, so nothing leaves this
//! machine.
//!
//! Every test here changes `HOME`, `PATH` and the agent socket, because
//! that is what the operator's credentials *are*; they run under the
//! testkit's environment lock, one at a time.

#![cfg(unix)]

use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
};

use uze_application::UzeApplication;
use uze_core::{UzeError, UzeHome};
use uze_testkit::{
    env::ProcessEnvGuard,
    forge::{Answer, FakeSsh, GitHttpServer, RecordingProxy},
    git::isolated_git_in,
};

const TOKEN: &str = "s3cret-t0ken";

/// A machine with a UZE home, an operator home, a forge root and an `ssh`
/// that serves that root. Nothing is reachable until a test says so.
struct World {
    root: PathBuf,
    forge: PathBuf,
    operator: PathBuf,
    home: UzeHome,
    ssh: FakeSsh,
    environment: ProcessEnvGuard<'static>,
}

impl World {
    fn new(label: &str) -> Self {
        let root = uze_testkit::temp::scratch(label);
        let forge = root.join("forge");
        let operator = root.join("operator");
        fs::create_dir_all(&forge).unwrap();
        fs::create_dir_all(operator.join("xdg")).unwrap();
        let ssh = FakeSsh::install(&root, &forge);
        let mut environment = uze_testkit::env::scope();
        environment
            .set("HOME", &operator)
            .set("XDG_CONFIG_HOME", operator.join("xdg"))
            .set("PATH", ssh.path())
            .remove("SSH_AUTH_SOCK")
            // The runner's own system config — `osxkeychain` on macOS — is
            // not this world's operator's.
            .set("GIT_CONFIG_NOSYSTEM", "1")
            .remove("GIT_CONFIG_GLOBAL")
            .remove("GIT_CONFIG_SYSTEM")
            .remove("http_proxy")
            .remove("https_proxy")
            .remove("HTTPS_PROXY")
            .remove("all_proxy")
            .remove("ALL_PROXY")
            .remove("no_proxy")
            .remove("NO_PROXY");
        Self {
            home: UzeHome::at(root.join("uze")),
            root,
            forge,
            operator,
            ssh,
            environment,
        }
    }

    fn application(&self) -> UzeApplication {
        UzeApplication::new(self.home.clone(), Vec::new())
    }

    /// A marketplace `mkt` offering `flow`, published bare at
    /// `<forge>/<path>`, which is where both the HTTP server and `ssh`
    /// find it.
    fn publish(&self, path: &str) {
        let work = self.root.join(format!("work-{}", path.replace('/', "-")));
        fs::create_dir_all(work.join("plugins/flow/skills/one")).unwrap();
        fs::write(
            work.join("plugins/flow/plugin.json"),
            r#"{"name":"flow","description":"d"}"#,
        )
        .unwrap();
        fs::write(
            work.join("plugins/flow/skills/one/SKILL.md"),
            "---\nname: one\ndescription: d\n---\n\nflow.\n",
        )
        .unwrap();
        fs::write(
            work.join("marketplace.json"),
            r#"{"name":"mkt","plugins":[{"name":"flow","source":"./plugins/flow"}]}"#,
        )
        .unwrap();
        uze_testkit::git::commit_everything_in(&work);
        let bare = self.forge.join(path);
        fs::create_dir_all(bare.parent().unwrap()).unwrap();
        isolated_git_in(
            &work,
            &["clone", "--quiet", "--bare", ".", bare.to_str().unwrap()],
        );
    }

    /// The operator's own Git config, as `~/.gitconfig`.
    fn operator_config(&self, text: &str) {
        fs::write(self.operator.join(".gitconfig"), text).unwrap();
    }

    /// A `store` credential helper scoped to `base`, holding the token.
    fn credential_for(&self, base: &str) {
        let (scheme, authority) = base.split_once("://").unwrap();
        fs::write(
            self.operator.join(".git-credentials"),
            format!("{scheme}://user:{TOKEN}@{authority}\n"),
        )
        .unwrap();
        self.operator_config(&format!("[credential \"{base}\"]\n\thelper = store\n"));
    }

    fn with_agent(&mut self) {
        let socket = self.root.join("agent.sock");
        self.environment.set("SSH_AUTH_SOCK", socket);
    }

    /// Every byte UZE wrote, searched for the operator's token.
    fn assert_no_token_in_uze_home(&self) {
        fn walk(path: &Path) {
            let Ok(entries) = fs::read_dir(path) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path);
                } else if let Ok(bytes) = fs::read(&path) {
                    assert!(
                        !String::from_utf8_lossy(&bytes).contains(TOKEN),
                        "the token reached {}",
                        path.display()
                    );
                }
            }
        }
        walk(self.home.root());
    }

    fn mirror(&self) -> PathBuf {
        self.home.marketplace_cache_dir().join("mkt/repo")
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[test]
fn a_public_marketplace_is_reached_with_no_credential_at_all() {
    let world = World::new("access-public");
    world.publish("team/plugins");
    let server = GitHttpServer::start(&world.forge, None, Answer::Git);

    let registration = world
        .application()
        .marketplace()
        .register(&server.url("team/plugins"))
        .unwrap();
    world
        .application()
        .marketplace()
        .install_plugin("flow@mkt", &uze_core::trust::AlwaysTrust)
        .unwrap();

    assert_eq!(registration.identity, server.url("team/plugins"));
    assert!(world.ssh.calls().is_empty(), "ssh was never needed");
}

#[test]
fn a_private_marketplace_is_reached_through_a_url_scoped_credential_helper() {
    let world = World::new("access-helper");
    world.publish("team/plugins");
    let server = GitHttpServer::start(&world.forge, Some(("user", TOKEN)), Answer::Git);
    world.credential_for(&server.url(""));

    world
        .application()
        .marketplace()
        .register(&server.url("team/plugins"))
        .unwrap();
    world
        .application()
        .marketplace()
        .install_plugin("flow@mkt", &uze_core::trust::AlwaysTrust)
        .unwrap();

    assert!(world.ssh.calls().is_empty());
    world.assert_no_token_in_uze_home();
}

#[test]
fn a_redirect_never_carries_the_credential_to_another_host() {
    let world = World::new("access-redirect");
    world.publish("team/plugins");
    let elsewhere = GitHttpServer::start(&world.forge, Some(("user", TOKEN)), Answer::Git);
    let asked = GitHttpServer::start(
        &world.forge,
        None,
        Answer::RedirectTo(elsewhere.url("").trim_end_matches('/').to_owned()),
    );
    // A helper answering for any host: only the refusal to follow a
    // redirect keeps it from answering for the one it was sent to.
    fs::write(
        world.operator.join(".git-credentials"),
        format!(
            "{}\n",
            elsewhere
                .url("")
                .replacen("://", &format!("://user:{TOKEN}@"), 1)
                .trim_end_matches('/')
        ),
    )
    .unwrap();
    world.operator_config("[credential]\n\thelper = store\n");

    let refused = world
        .application()
        .marketplace()
        .register(&asked.url("team/plugins"));

    assert!(refused.is_err(), "the redirect was followed: {refused:?}");
    assert!(
        !elsewhere
            .requests()
            .iter()
            .any(|request| request.ends_with("[authorized]")),
        "the other host received the credential: {:?}",
        elsewhere.requests()
    );
}

#[test]
fn a_private_marketplace_is_reached_over_ssh_and_ssh_is_tried_first_next_time() {
    let mut world = World::new("access-ssh");
    world.publish("team/plugins");
    let server = GitHttpServer::start(&world.forge, Some(("user", TOKEN)), Answer::Git);
    world.with_agent();

    world
        .application()
        .marketplace()
        .register(&server.url("team/plugins"))
        .unwrap();
    let asked_before = server.requests().len();
    world
        .application()
        .marketplace()
        .install_plugin("flow@mkt", &uze_core::trust::AlwaysTrust)
        .unwrap();

    let calls = world.ssh.calls();
    assert!(!calls.is_empty(), "the fetch went over ssh");
    assert!(
        calls.iter().all(
            |call| call.contains("BatchMode=yes") && call.contains("StrictHostKeyChecking=yes")
        ),
        "ssh never prompts and never trusts an unknown host: {calls:?}"
    );
    assert_eq!(
        server.requests().len(),
        asked_before,
        "the remembered transport was tried first, so HTTPS was not asked again"
    );
    let remembered = fs::read_to_string(world.mirror().join("transport.json")).unwrap();
    assert!(
        remembered.contains("git@127.0.0.1:team/plugins.git"),
        "{remembered}"
    );
}

#[test]
fn ssh_answers_when_the_https_port_is_closed() {
    let mut world = World::new("access-closed-port");
    world.publish("team/plugins");
    world.with_agent();
    let url = format!("http://127.0.0.1:{}/team/plugins", closed_port());

    world.application().marketplace().register(&url).unwrap();

    assert!(!world.ssh.calls().is_empty());
}

#[test]
fn no_transport_with_access_is_one_error_naming_each_and_records_nothing() {
    let world = World::new("access-none");
    world.publish("team/plugins");
    let server = GitHttpServer::start(&world.forge, Some(("user", TOKEN)), Answer::Git);

    let error = world
        .application()
        .marketplace()
        .register(&server.url("team/plugins"))
        .unwrap_err();

    let message = error.to_string();
    assert!(
        matches!(error, UzeError::RepositoryAccessRefused { .. }),
        "{message}"
    );
    for label in ["https (anonymous)", "https (credentials)", "ssh"] {
        assert!(message.contains(label), "{label} missing from: {message}");
    }
    assert!(
        message.contains("Permission denied (publickey)"),
        "each transport says what refused it, not Git's advice: {message}"
    );
    assert!(
        world.application().marketplace().list().unwrap().len() == 1,
        "only the built-in marketplace is registered"
    );
}

#[test]
fn a_forge_answering_with_a_login_page_is_a_refusal_not_a_repository() {
    let world = World::new("access-login-page");
    let server = GitHttpServer::start(&world.forge, None, Answer::LoginPage);

    let error = world
        .application()
        .marketplace()
        .register(&server.url("team/plugins"))
        .unwrap_err();

    assert!(
        matches!(error, UzeError::RepositoryAccessRefused { .. }),
        "{error}"
    );
}

#[test]
fn an_offline_machine_is_reported_offline_after_asking_ssh_once() {
    let mut world = World::new("access-offline");
    world.with_agent();

    let error = world
        .application()
        .marketplace()
        .register("https://nothing-here.invalid/team/plugins")
        .unwrap_err();

    assert!(
        matches!(error, UzeError::RepositoryOffline { .. }),
        "{error}"
    );
    assert_eq!(
        world.ssh.calls().len(),
        1,
        "ssh is still asked once — its own config may name a host DNS does not — \
         and the credentialed HTTPS attempt is skipped"
    );
    assert!(error.to_string().contains("skipped"), "{error}");
}

#[test]
fn an_ssh_host_alias_dns_does_not_know_is_reached_over_ssh() {
    let mut world = World::new("access-ssh-alias");
    world.publish("team/plugins");
    world.with_agent();

    let registration = world
        .application()
        .marketplace()
        .register("git@forge-alias.test:team/plugins.git")
        .unwrap();

    assert_eq!(
        registration.identity,
        "https://forge-alias.test/team/plugins"
    );
    assert!(
        world
            .ssh
            .calls()
            .iter()
            .any(|call| call.contains("git@forge-alias.test")),
        "{:?}",
        world.ssh.calls()
    );
}

#[test]
fn a_port_that_does_not_answer_is_asked_over_https_once() {
    let world = World::new("access-closed-once");
    let url = format!("http://127.0.0.1:{}/team/plugins", closed_port());

    let error = world
        .application()
        .marketplace()
        .register(&url)
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("https (credentials): skipped"),
        "the credentialed HTTPS attempt would meet the same closed port: {error}"
    );
}

#[test]
fn a_refresh_nobody_answers_leaves_the_mirror_as_it_was() {
    let world = World::new("access-failed-refresh");
    world.publish("team/plugins");
    let server = GitHttpServer::start(&world.forge, None, Answer::Git);
    let url = server.url("team/plugins");
    world.application().marketplace().register(&url).unwrap();
    let mirror = world.mirror();
    let refs = isolated_git_in(&mirror, &["for-each-ref"]);

    server.answer_with(Answer::LoginPage);
    let refreshed = uze_core::acquisition::mirror::ensure(&url, &url, &mirror);

    assert!(refreshed.is_err(), "a login page is not a repository");
    assert_eq!(
        isolated_git_in(&mirror, &["for-each-ref"]),
        refs,
        "an empty answer pruned nothing"
    );
    assert_eq!(
        isolated_git_in(&mirror, &["remote", "get-url", "origin"]),
        url,
        "origin still names the transport the mirror is remembered by"
    );
}

#[test]
fn the_operators_proxy_carries_the_anonymous_attempt() {
    let mut world = World::new("access-proxy");
    world.publish("team/plugins");
    let server = GitHttpServer::start(&world.forge, None, Answer::Git);
    let proxy = RecordingProxy::start();
    world.environment.set("http_proxy", proxy.url());

    world
        .application()
        .marketplace()
        .register(&server.url("team/plugins"))
        .unwrap();

    assert!(
        proxy
            .carried()
            .iter()
            .any(|request| request.contains("/team/plugins/info/refs")),
        "{:?}",
        proxy.carried()
    );
}

#[test]
fn the_operators_aliases_and_filters_do_not_run() {
    let world = World::new("access-no-config");
    let marker = world.root.join("filter-ran");
    world.publish("team/plugins");
    let server = GitHttpServer::start(&world.forge, Some(("user", TOKEN)), Answer::Git);
    world.credential_for(&server.url(""));
    let credentials = fs::read_to_string(world.operator.join(".gitconfig")).unwrap();
    world.operator_config(&format!(
        "{credentials}[filter \"evil\"]\n\tsmudge = sh -c 'touch {}; cat'\n\
         [core]\n\tattributesFile = {}\n[alias]\n\tshow = !touch {}\n",
        marker.display(),
        world.operator.join("attributes").display(),
        marker.display(),
    ));
    fs::write(world.operator.join("attributes"), "* filter=evil\n").unwrap();

    world
        .application()
        .marketplace()
        .register(&server.url("team/plugins"))
        .unwrap();
    world
        .application()
        .marketplace()
        .install_plugin("flow@mkt", &uze_core::trust::AlwaysTrust)
        .unwrap();

    assert!(
        !marker.exists(),
        "the operator's configuration ran inside acquisition"
    );
}

#[test]
fn plain_http_off_this_machine_is_refused() {
    let world = World::new("access-plain-http");

    let error = world
        .application()
        .marketplace()
        .register("http://git.acme.io/team/plugins")
        .unwrap_err();

    assert!(error.to_string().contains("plain HTTP"), "{error}");
}

#[test]
fn a_short_locator_asks_one_host_and_suggests_the_others() {
    let world = World::new("access-short");
    let asked = GitHttpServer::start(&world.forge, None, Answer::Git);
    let other = GitHttpServer::start(&world.forge, None, Answer::Git);
    let application = world.application();
    let market = application.marketplace();
    market.define_host("here", &asked.url("")).unwrap();
    market.define_host("there", &other.url("")).unwrap();
    market.set_default_host("here").unwrap();

    let error = market.register("team/plugins").unwrap_err().to_string();

    assert!(other.requests().is_empty(), "another host was asked");
    assert!(!asked.requests().is_empty());
    assert!(error.contains("there:team/plugins"), "{error}");
}

#[test]
fn a_failure_under_an_organisation_alias_suggests_paths_that_resolve() {
    let world = World::new("access-org-hint");
    let server = GitHttpServer::start(&world.forge, None, Answer::Git);
    let application = world.application();
    let market = application.marketplace();
    market.define_host("org", &server.url("my-org")).unwrap();

    let error = market.register("org:plugins").unwrap_err().to_string();

    assert!(error.contains("github:my-org/plugins"), "{error}");
    assert!(
        !error.contains("org:my-org"),
        "the alias asked is never suggested: {error}"
    );
}

#[test]
fn an_alias_and_the_default_host_resolve_what_is_typed() {
    let world = World::new("access-alias");
    world.publish("team/plugins");
    let server = GitHttpServer::start(&world.forge, None, Answer::Git);
    let application = world.application();
    let market = application.marketplace();
    market.define_host("org", &server.url("team")).unwrap();

    let by_alias = market.register("org:plugins").unwrap();
    assert_eq!(by_alias.identity, server.url("team/plugins"));

    market.define_host("forge", &server.url("")).unwrap();
    market.set_default_host("forge").unwrap();
    let by_default = market.register("team/plugins").unwrap();
    assert_eq!(by_default.identity, server.url("team/plugins"));
    assert!(!by_default.added, "the same repository, already registered");
}

#[test]
fn an_older_spelling_meets_its_canonical_form_without_conflict_or_reclone() {
    let world = World::new("access-old-spelling");
    world.publish("team/plugins");
    let server = GitHttpServer::start(&world.forge, None, Answer::Git);
    let old = server.url("team/plugins.git");
    let application = world.application();
    application.marketplace().register(&old).unwrap();
    // What a registry and a mirror written before canonical spellings hold.
    let registry = world.home.marketplaces_path();
    let text = fs::read_to_string(&registry)
        .unwrap()
        .replace(&server.url("team/plugins"), &old);
    fs::write(&registry, text).unwrap();
    fs::remove_file(world.mirror().join("transport.json")).unwrap();
    isolated_git_in(&world.mirror(), &["remote", "set-url", "origin", &old]);
    fs::write(world.mirror().join("kept"), "").unwrap();

    application
        .marketplace()
        .install_plugin("flow@mkt", &uze_core::trust::AlwaysTrust)
        .unwrap();
    assert!(
        world.mirror().join("kept").exists(),
        "the mirror was re-cloned"
    );

    let again = world
        .application()
        .marketplace()
        .register(&server.url("team/plugins"))
        .unwrap();
    assert!(!again.added);
    assert!(
        fs::read_to_string(&registry)
            .unwrap()
            .contains(&format!("\"{}\"", server.url("team/plugins"))),
        "the entry takes the canonical spelling"
    );
}

#[test]
fn a_credential_in_the_url_is_refused_before_git_runs() {
    let world = World::new("access-inline-credential");

    let error = world
        .application()
        .marketplace()
        .register(&format!("https://user:{TOKEN}@github.com/hiukky/ai"))
        .unwrap_err();

    assert!(matches!(error, UzeError::CredentialBearingUrl), "{error}");
    assert!(!error.to_string().contains(TOKEN));
    world.assert_no_token_in_uze_home();
}
