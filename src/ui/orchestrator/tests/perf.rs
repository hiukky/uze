//! How the code surface behaves against a real, large checkout — the
//! latencies an operator waits through and the CPU it burns while open.
//!
//! Ignored: it needs a repository worth measuring, named by `BENCH_REPO`,
//! and `BENCH_FILE`, a file in it to open (a directory holding siblings,
//! so moving to the next one is a real second open). Run it with
//! `cargo test --release -p uze --lib code_surface_under_load -- --ignored --nocapture`.
//!
//! Drives the same `Attach` the workspace client runs — its threads, its
//! channels, its frame — so a number here is what the loop would have
//! waited, not what one function took in isolation.

use std::path::{Path, PathBuf};

use super::workspace_tests::{Driven, driven, model_of, session};
use crate::ui::orchestrator::{POLL, WorkspaceModel, open_code_at};
use std::time::{Duration, Instant};
use uze_core::UzeHome;
use uze_extensions::code::{CodeView, ContentMode};
use uze_extensions::view::Content;

const TIMEOUT: Duration = Duration::from_secs(60);

/// What the surface has on screen, if it is content rather than a
/// message: its heading.
fn shown(view: &CodeView) -> Option<String> {
    let drawn = uze_extensions::code::view(
        view,
        uze_extensions::view::Size {
            width: 140,
            height: 40,
        },
    );
    match drawn.content {
        Content::Lines { heading, lines, .. } if !lines.is_empty() => Some(heading),
        _ => None,
    }
}

/// Whether what is on screen is the selected file, arrived.
fn selection_shown(view: &CodeView) -> bool {
    let Some(selected) = view.selected() else {
        return false;
    };
    let name = selected
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    !view.diff_pending() && shown(view).is_some_and(|heading| heading.contains(&name))
}

fn cpu() -> (Duration, Duration) {
    let of = |who| {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        unsafe { libc::getrusage(who, &mut usage) };
        let time = |t: libc::timeval| {
            Duration::from_secs(t.tv_sec as u64) + Duration::from_micros(t.tv_usec as u64)
        };
        time(usage.ru_utime) + time(usage.ru_stime)
    };
    (of(libc::RUSAGE_SELF), of(libc::RUSAGE_CHILDREN))
}

struct Loop<'a> {
    driven: Driven<'a>,
    frames: Vec<Duration>,
}

impl Loop<'_> {
    /// One turn of the client's loop: answers in, questions out, a frame.
    fn turn(&mut self) {
        self.driven.pump();
        let started = Instant::now();
        self.driven.frame();
        self.frames.push(started.elapsed());
        std::thread::sleep(POLL);
    }

    fn until(&mut self, what: &str, done: impl Fn(&WorkspaceModel) -> bool) -> Duration {
        let started = Instant::now();
        while !done(&self.driven.attach.model) {
            if started.elapsed() >= TIMEOUT {
                let view = self.driven.attach.model.code.as_ref();
                panic!(
                    "{what} never happened: selected {:?}, shown {:?}, pending {:?}, content {:?}",
                    view.and_then(CodeView::selected),
                    view.and_then(shown),
                    view.map(CodeView::diff_pending),
                    view.map(|view| uze_extensions::code::view(
                        view,
                        uze_extensions::view::Size {
                            width: 140,
                            height: 40
                        }
                    )
                    .content),
                );
            }
            self.turn();
        }
        started.elapsed()
    }

    fn idle(&mut self, span: Duration) -> (f64, f64) {
        let (own, children) = cpu();
        let started = Instant::now();
        while started.elapsed() < span {
            self.turn();
        }
        let (own_after, children_after) = cpu();
        let wall = started.elapsed().as_secs_f64();
        (
            (own_after - own).as_secs_f64() / wall,
            (children_after - children).as_secs_f64() / wall,
        )
    }

    fn key(&mut self, key: crossterm::event::KeyEvent) {
        self.driven.press_key(key);
    }
}

fn alt(character: char) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char(character),
        crossterm::event::KeyModifiers::ALT,
    )
}

fn plain(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}

fn summary(label: &str, samples: &mut [Duration]) {
    samples.sort();
    let at = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    println!(
        "{label:<28} n={:<3} p50={:>8.1?} p95={:>8.1?} max={:>8.1?}",
        samples.len(),
        at(0.5),
        at(0.95),
        samples[samples.len() - 1]
    );
}

fn code_view(model: &WorkspaceModel) -> &CodeView {
    model.code.as_ref().expect("the surface is open")
}

#[test]
#[ignore = "needs BENCH_REPO and BENCH_FILE"]
fn code_surface_under_load() {
    let repo = PathBuf::from(std::env::var("BENCH_REPO").expect("BENCH_REPO"));
    let file = repo.join(std::env::var("BENCH_FILE").expect("BENCH_FILE"));
    let home = UzeHome::at(uze_testkit::temp::scratch("code-surface-under-load"));
    let driven = driven(model_of(session(&repo)), &home).on_a_roomy_terminal();
    let mut run = Loop {
        driven,
        frames: Vec::new(),
    };

    // The badge's own cadence, with nothing open: the floor every other
    // number sits on.
    let (own, children) = run.idle(Duration::from_secs(8));
    println!("idle, nothing open          uze={own:.2} cpu  git={children:.2} cpu");

    run.key(alt('g'));
    let open = run.until("the first diff", |model| {
        model.code.as_ref().is_some_and(selection_shown)
    });
    println!("open changes → first diff   {open:>8.1?}");

    let mut clicks = Vec::new();
    for _ in 0..15 {
        let before = code_view(&run.driven.attach.model)
            .selected()
            .map(Path::to_path_buf);
        run.key(plain(crossterm::event::KeyCode::Down));
        let moved = code_view(&run.driven.attach.model)
            .selected()
            .map(Path::to_path_buf);
        if moved == before {
            break;
        }
        clicks.push(run.until("the next diff", |model| {
            model.code.as_ref().is_some_and(selection_shown)
        }));
    }
    summary("changes: next file → diff", &mut clicks);

    let (own, children) = run.idle(Duration::from_secs(8));
    println!("idle, changes open          uze={own:.2} cpu  git={children:.2} cpu");

    open_code_at(&mut run.driven.attach.model, &repo, &file);
    let first = run.until("the file", |model| {
        model.code.as_ref().is_some_and(selection_shown)
    });
    println!("open file                   {first:>8.1?}");
    let mut steps = Vec::new();
    for _ in 0..10 {
        run.key(plain(crossterm::event::KeyCode::Down));
        run.key(plain(crossterm::event::KeyCode::Enter));
        steps.push(run.until("the next file", |model| {
            model.code.as_ref().is_some_and(selection_shown)
        }));
    }
    summary("files: next file → shown", &mut steps);

    run.key(plain(crossterm::event::KeyCode::Char('m')));
    let map = run.until("the map", |model| {
        model
            .code
            .as_ref()
            .is_some_and(|view| view.showing() == ContentMode::Map && view.has_map())
    });
    println!("map → drawn                 {map:>8.1?}");

    summary("frame", &mut run.frames);
}
