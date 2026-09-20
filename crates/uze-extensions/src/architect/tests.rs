use super::*;

const SPACE: Size = Size {
    width: 100,
    height: 30,
};

/// uze's own architecture, as the repository keeps it. Living fixtures:
/// what is checked here is also that the project's real artifacts draw.
fn opened() -> ArchitectView {
    filled(ArchitectView::opening("~/project".to_owned()))
}

/// The same artifacts, read into a surface that may already have been
/// told where the viewer left off.
fn filled(mut state: ArchitectView) -> ArchitectView {
    let artifacts = vec![
        Artifact::read(
            "containers.mmd",
            include_str!("../../../../docs/architecture/containers.mmd"),
        ),
        Artifact::read(
            "system-context.mmd",
            include_str!("../../../../docs/architecture/system-context.mmd"),
        ),
        Artifact::read(
            "install-sequence.mmd",
            include_str!("../../../../docs/architecture/install-sequence.mmd"),
        ),
        Artifact::read(
            "crate-layering.mmd",
            include_str!("../../../../docs/architecture/crate-layering.mmd"),
        ),
        Artifact::read(
            "install-pipeline.mmd",
            include_str!("../../../../docs/architecture/install-pipeline.mmd"),
        ),
        Artifact::read(
            "core-components.mmd",
            include_str!("../../../../docs/architecture/core-components.mmd"),
        ),
        Artifact::read(
            "agent-lifecycle.mmd",
            include_str!("../../../../docs/architecture/agent-lifecycle.mmd"),
        ),
        Artifact::read(
            "attachment-lifecycle.mmd",
            include_str!("../../../../docs/architecture/attachment-lifecycle.mmd"),
        ),
    ];
    state.absorb(ArtifactsAnswer {
        branch: "main".to_owned(),
        artifacts: Artifacts::Found {
            artifacts,
            project: PathBuf::from("/project"),
        },
    });
    state
}

/// The box `id` names on the diagram on show.
fn node_named_in(state: &ArchitectView, id: &str) -> Option<usize> {
    let Drawing::Graph(scene) = &state.drawing else {
        return None;
    };
    scene.graph.nodes.iter().position(|node| node.id == id)
}

fn showing(name: &str) -> ArchitectView {
    let mut state = opened();
    let index = state
        .catalog
        .artifacts()
        .iter()
        .position(|artifact| artifact.name == name)
        .expect("the artifact exists");
    state.open(index);
    state
}

fn drawn(state: &ArchitectView) -> String {
    state.canvas.as_ref().expect("it draws").to_text()
}

/// Run with `--nocapture` to read the diagrams: this is the quickest way
/// to look at what the layout produced without opening the TUI.
#[test]
fn every_artifact_is_drawn_with_every_edge_routed() {
    let mut state = opened();
    for index in 0..state.catalog.artifacts().len() {
        state.open(index);
        let name = state.catalog.get(index).unwrap().name.clone();
        if let Drawing::Graph(scene) = &state.drawing {
            assert_eq!(scene.routes.unrouted, 0, "{name} left an edge out");
        }
        let text = drawn(&state);
        println!("\n=== {name} ===\n{text}");
        assert!(text.lines().count() > 5);
    }
}

#[test]
fn ascii_draws_the_same_diagram_in_seven_bit_characters() {
    let mut state = opened();
    state.show(Showing::Ascii);
    for index in 0..state.catalog.artifacts().len() {
        state.open(index);
        let foreign: Vec<char> = drawn(&state)
            .chars()
            .filter(|c| !c.is_ascii() && !c.is_alphanumeric() && *c != '·' && *c != '~')
            .collect();
        assert!(foreign.is_empty(), "{foreign:?}");
    }
}

#[test]
fn the_screen_is_exactly_the_space_it_was_given() {
    let state = opened();
    let Content::Lines { lines, scroll, .. } = view(&state, SPACE).content else {
        panic!("a diagram is lines");
    };
    assert_eq!((lines.len(), scroll), (30, 0));
    for line in &lines {
        let width: i32 = line
            .spans
            .iter()
            .map(|s| crate::shared::canvas::text_width(&s.text))
            .sum();
        assert!(width <= 100, "{width} columns would be cut");
    }
}

#[test]
fn a_drawing_smaller_than_the_screen_opens_in_the_middle_of_it() {
    let state = showing("System context");
    let wide = Size {
        width: 200,
        height: 30,
    };
    assert_eq!(state.corner(wide).0, (state.board_size().0 - 200) / 2);
}

#[test]
fn every_edge_of_the_board_can_be_brought_to_the_middle_of_the_screen() {
    let mut state = opened();
    let board = state.board_size();
    drag_by(&mut state, 5000, 5000, SPACE);
    assert_eq!(state.corner(SPACE), (-50, -15));
    drag_by(&mut state, -50_000, -50_000, SPACE);
    assert_eq!(state.corner(SPACE), (board.0 - 50, board.1 - 15));
}

#[test]
fn a_board_that_fits_the_screen_still_moves() {
    let mut state = showing("System context");
    let wide = Size {
        width: 200,
        height: 30,
    };
    let home = state.corner(wide);
    drag_by(&mut state, 20, 0, wide);
    assert_eq!(state.corner(wide), (home.0 - 20, home.1));
}

#[test]
fn a_click_on_the_minimap_brings_that_part_of_the_board_to_the_screen() {
    let mut state = opened();
    let home = state.corner(SPACE);
    let map = state.minimap(SPACE).expect("the screen has room for a map");
    let hit = ViewHit::PlaceCaret {
        line: (map.frame.y + map.frame.h - 2) as usize,
        cell: (map.frame.x + map.frame.w / 2) as usize,
    };
    handle_mouse(&mut state, Some(hit), SPACE);
    assert!(
        state.corner(SPACE).1 > home.1 + 10,
        "{:?}",
        state.corner(SPACE)
    );
    assert_eq!(state.picked, None);
}

#[test]
fn the_artifacts_go_round_and_an_area_opens_on_its_first() {
    let mut state = opened();
    let last = state.catalog.artifacts().len() - 1;
    handle_command(&mut state, Command::PreviousView, SPACE);
    assert_eq!(state.selected, last);
    handle_command(&mut state, Command::NextView, SPACE);
    assert_eq!(state.selected, 0);
    let View { navigator, .. } = view(&state, SPACE);
    let areas: Vec<usize> = navigator
        .unwrap()
        .rows
        .iter()
        .filter_map(|row| match row {
            NavigatorRow::Group { id, .. } => Some(*id),
            NavigatorRow::Item { .. } => None,
        })
        .collect();
    assert_eq!(areas.len(), 3);
    handle_mouse(&mut state, Some(ViewHit::ToggleGroup(areas[2])), SPACE);
    assert_eq!(state.selected, areas[2]);
}

#[test]
fn clicking_a_box_selects_it_and_clicking_it_again_lets_go() {
    let mut state = showing("Crate layering");
    let Drawing::Graph(scene) = &state.drawing else {
        panic!("the layering is a graph");
    };
    let frame = scene.placement.nodes[0];
    let corner = state.corner(SPACE);
    let hit = ViewHit::PlaceCaret {
        line: (frame.y + 1 - corner.1) as usize,
        cell: (frame.x + 1 - corner.0) as usize,
    };
    handle_mouse(&mut state, Some(hit), SPACE);
    assert_eq!(state.picked, Some(0));
    handle_mouse(&mut state, Some(hit), SPACE);
    assert_eq!(state.picked, None);
}

#[test]
fn a_surface_with_nothing_to_draw_says_why_and_what_to_do() {
    let mut state = ArchitectView::opening("~/project".to_owned());
    let Content::Message { hint, .. } = view(&state, SPACE).content else {
        panic!("a read in flight is a message");
    };
    assert_eq!(hint, None);

    struct Bare;
    impl Host for Bare {
        fn git(&self, _: &std::path::Path, _: &[&str], _: &[i32]) -> Result<String, String> {
            Err("no git here".to_owned())
        }
        fn repository_root(&self, _: &std::path::Path) -> Result<PathBuf, String> {
            Err("no git here".to_owned())
        }
        fn read_file(&self, _: &std::path::Path) -> Result<String, String> {
            Err("no such file".to_owned())
        }
        fn list_dir(&self, _: &std::path::Path) -> Result<Vec<crate::DirEntry>, String> {
            Ok(Vec::new())
        }
        fn write_file(&self, _: &std::path::Path, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn delete_file(&self, _: &std::path::Path) -> Result<(), String> {
            Ok(())
        }
        fn syntax_theme(&self) -> String {
            String::new()
        }
    }
    state.absorb(read_artifacts(
        &Bare,
        Path::new("/project"),
        ArtifactSource::Undeclared,
    ));
    let Content::Message { text, hint, .. } = view(&state, SPACE).content else {
        panic!("an undeclared project is a message");
    };
    assert!(text.contains("declares no artifacts"));
    assert!(hint.unwrap().contains("agents.yaml"));

    let empty = ArtifactSource::Directory {
        path: PathBuf::from("/project/docs/diagrams"),
        declared: "docs/diagrams".to_owned(),
        project: PathBuf::from("/project"),
    };
    state.absorb(read_artifacts(&Bare, Path::new("/project"), empty));
    let Content::Message { text, .. } = view(&state, SPACE).content else {
        panic!("an empty directory is a message");
    };
    assert!(text.contains("docs/diagrams"), "{text}");
}

#[test]
fn the_list_of_artifacts_opens_on_the_one_on_show_and_steps_over_to_the_areas() {
    let mut state = showing("Containers");
    let areas = state.areas();
    handle_command(&mut state, Command::ChooseItem, SPACE);
    assert_eq!(state.choosing, Some(Choosing::Item(state.selected)));
    handle_command(&mut state, Command::Pan(PanDirection::Down), SPACE);
    handle_command(&mut state, Command::Pan(PanDirection::Down), SPACE);
    assert_eq!(
        state.choosing,
        Some(Choosing::Item(areas[0])),
        "three C4 views, so two steps down from the second is the first again"
    );

    handle_command(&mut state, Command::Pan(PanDirection::Left), SPACE);
    assert_eq!(
        state.choosing,
        Some(Choosing::Group(areas[0])),
        "left is the areas"
    );
    handle_command(&mut state, Command::Pan(PanDirection::Down), SPACE);
    handle_command(&mut state, Command::Activate, SPACE);
    assert_eq!((state.selected, state.choosing), (areas[1], None));
}

#[test]
fn a_list_offers_only_the_area_on_show() {
    let state = showing("Crate layering");
    let names: Vec<&str> = state
        .siblings()
        .into_iter()
        .map(|artifact| state.catalog.get(artifact).unwrap().name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Agent lifecycle",
            "Attachment lifecycle",
            "Crate layering",
            "Install pipeline"
        ]
    );
}

#[test]
fn leaving_a_list_leaves_the_surface_open() {
    let mut state = opened();
    handle_command(&mut state, Command::ChooseItem, SPACE);
    let outcome = handle_command(&mut state, Command::Close, SPACE);
    assert_eq!((outcome, state.choosing), (ArchitectOutcome::Stay, None));

    handle_mouse(&mut state, Some(ViewHit::ChooseGroup), SPACE);
    assert!(matches!(state.choosing, Some(Choosing::Group(_))));
    handle_mouse(&mut state, Some(ViewHit::ChooseItem), SPACE);
    assert!(
        matches!(state.choosing, Some(Choosing::Item(_))),
        "the other selector takes over rather than only shutting this one"
    );

    let before = state.selected;
    let board = ViewHit::PlaceCaret { line: 1, cell: 1 };
    handle_mouse(&mut state, Some(board), SPACE);
    assert_eq!(
        (state.selected, state.choosing, state.picked),
        (before, None, None),
        "a click off the list shuts it and does nothing else"
    );

    handle_mouse(&mut state, Some(ViewHit::ChooseItem), SPACE);
    handle_mouse(&mut state, Some(ViewHit::SelectItem(before + 1)), SPACE);
    assert_eq!((state.selected, state.choosing), (before + 1, None));
}

fn pick(state: &mut ArchitectView, alias: &str) {
    let Drawing::Graph(scene) = &state.drawing else {
        panic!("a graph is on show");
    };
    state.picked = scene.graph.nodes.iter().position(|node| node.id == alias);
    assert!(state.picked.is_some(), "`{alias}` is drawn here");
}

/// The descent the menu offers, as names.
fn trail(state: &ArchitectView) -> Vec<String> {
    view(state, SPACE)
        .trail
        .into_iter()
        .map(|step| step.name)
        .collect()
}

/// The step of it the viewer is standing on.
fn here(state: &ArchitectView) -> String {
    view(state, SPACE)
        .trail
        .into_iter()
        .find(|step| step.current)
        .map(|step| step.name)
        .unwrap_or_default()
}

const LEVELS: [&str; 3] = ["System context", "Containers", "Core components"];

#[test]
fn the_levels_are_joined_by_alias_from_the_context_down_to_the_code() {
    let mut state = showing("System context");
    assert_eq!(trail(&state), LEVELS, "a model's levels are all on show");
    assert_eq!(here(&state), "System context", "at the outermost of them");

    pick(&mut state, "uze");
    assert_eq!(
        handle_command(&mut state, Command::Activate, SPACE),
        ArchitectOutcome::Stay
    );
    assert_eq!(trail(&state), LEVELS, "the levels do not change");
    assert_eq!(here(&state), "Containers", "only where the viewer stands");

    pick(&mut state, "core");
    handle_command(&mut state, Command::Activate, SPACE);
    assert_eq!(here(&state), "Core components");

    pick(&mut state, "package");
    assert_eq!(
        handle_command(&mut state, Command::Activate, SPACE),
        ArchitectOutcome::OpenPath {
            project: PathBuf::from("/project"),
            target: PathBuf::from("/project/crates/uze-core/src/package"),
        },
        "the last level down is the code itself"
    );
}

#[test]
fn coming_back_puts_the_viewer_where_they_were_standing() {
    let mut state = showing("System context");
    pick(&mut state, "uze");
    handle_command(&mut state, Command::Activate, SPACE);
    pick(&mut state, "core");
    handle_command(&mut state, Command::Activate, SPACE);

    handle_command(&mut state, Command::Back, SPACE);
    assert_eq!(here(&state), "Containers");
    let Drawing::Graph(scene) = &state.drawing else {
        panic!("containers is a graph");
    };
    let core = scene.graph.nodes.iter().position(|n| n.id == "core");
    assert_eq!(
        state.picked, core,
        "the box that was entered is picked again"
    );

    handle_mouse(&mut state, Some(ViewHit::SelectTrail(0)), SPACE);
    assert!(
        state.trail.is_empty(),
        "back at the top, nothing is entered"
    );
    assert_eq!(here(&state), "System context");

    // And the other way: a level this descent reaches that nobody
    // entered is a step forward, not a step back.
    handle_mouse(&mut state, Some(ViewHit::SelectTrail(2)), SPACE);
    assert_eq!(here(&state), "Core components");
    assert!(state.trail.is_empty(), "jumped to, not descended into");
}

#[test]
fn choosing_from_the_menu_forgets_the_way_in() {
    let mut state = showing("System context");
    pick(&mut state, "uze");
    handle_command(&mut state, Command::Activate, SPACE);
    handle_command(&mut state, Command::NextView, SPACE);
    assert!(state.trail.is_empty());
}

#[test]
fn a_second_click_on_a_box_that_leads_somewhere_follows_it() {
    let mut state = showing("System context");
    let Drawing::Graph(scene) = &state.drawing else {
        panic!("the context is a graph");
    };
    let uze = scene
        .graph
        .nodes
        .iter()
        .position(|n| n.id == "uze")
        .unwrap();
    let frame = scene.placement.nodes[uze];
    let corner = state.corner(SPACE);
    let hit = ViewHit::PlaceCaret {
        line: (frame.y + 1 - corner.1) as usize,
        cell: (frame.x + 1 - corner.0) as usize,
    };
    handle_mouse(&mut state, Some(hit), SPACE);
    assert_eq!(state.picked, Some(uze));
    assert_eq!(here(&state), "System context", "one click only picks");
    handle_mouse(&mut state, Some(hit), SPACE);
    assert_eq!(here(&state), "Containers", "the second goes inside");
}

#[test]
fn the_keys_walk_from_box_to_box() {
    let mut state = showing("System context");
    handle_command(&mut state, Command::SelectToward(PanDirection::Down), SPACE);
    let first = state
        .picked
        .expect("with nothing picked, the nearest box is");
    handle_command(&mut state, Command::SelectToward(PanDirection::Down), SPACE);
    let second = state.picked.unwrap();
    assert_ne!(first, second);
    let Drawing::Graph(scene) = &state.drawing else {
        panic!("the context is a graph");
    };
    let frames = &scene.placement.nodes;
    assert!(
        frames[second].center().1 > frames[first].center().1,
        "down is down"
    );
}

/// The key that closes peels one level at a time. Anything else makes a
/// surface that can be entered but not looked around in: one press and
/// the viewer is back where they started with three levels of work gone.
#[test]
fn the_key_that_closes_goes_up_a_level_until_there_is_none_left() {
    let mut state = showing("System context");
    pick(&mut state, "uze");
    handle_command(&mut state, Command::Activate, SPACE);
    pick(&mut state, "core");
    handle_command(&mut state, Command::Activate, SPACE);
    assert_eq!(here(&state), "Core components");

    for left in ["Containers", "System context"] {
        assert_eq!(
            handle_command(&mut state, Command::Close, SPACE),
            ArchitectOutcome::Stay
        );
        // Coming back selects the box that was entered, which is a level
        // of its own — the next press lets go of it, the one after goes up.
        assert_eq!(
            handle_command(&mut state, Command::Close, SPACE),
            ArchitectOutcome::Stay
        );
        assert_eq!(here(&state), left);
    }
    assert!(state.trail.is_empty(), "back at the top");
    assert_eq!(
        handle_command(&mut state, Command::Close, SPACE),
        ArchitectOutcome::Close,
        "and only there does it close"
    );
}

/// The way up is named in the footer only where there is one.
#[test]
fn the_footer_offers_the_way_up_only_once_something_has_been_entered() {
    let mut state = showing("System context");
    assert!(!view(&state, SPACE).footer.contains(&Command::Back));

    pick(&mut state, "uze");
    handle_command(&mut state, Command::Activate, SPACE);
    assert!(view(&state, SPACE).footer.contains(&Command::Back));
}

#[test]
fn a_selector_with_nothing_to_choose_stays_shut() {
    let lone = Artifact::read(
        "install-sequence.mmd",
        include_str!("../../../../docs/architecture/install-sequence.mmd"),
    );
    let mut state = ArchitectView::opening("~/project".to_owned());
    state.absorb(ArtifactsAnswer {
        branch: "main".to_owned(),
        artifacts: Artifacts::Found {
            artifacts: vec![lone],
            project: PathBuf::from("/project"),
        },
    });
    handle_command(&mut state, Command::ChooseGroup, SPACE);
    assert_eq!(state.choosing, None, "one area");
    handle_command(&mut state, Command::ChooseItem, SPACE);
    assert_eq!(state.choosing, None, "one artifact in it");

    let mut state = opened();
    handle_command(&mut state, Command::ChooseGroup, SPACE);
    assert!(state.choosing.is_some(), "four areas is a choice");
}

/// While a list is open the highlight follows the pointer: hovering a
/// row is highlighting it, which is what makes it read as a menu.
#[test]
fn hovering_a_row_of_an_open_list_highlights_it() {
    let mut state = showing("Crate layering");
    handle_command(&mut state, Command::ChooseItem, SPACE);
    let Some(Choosing::Item(first)) = state.choosing else {
        panic!("the artifacts of the area are offered");
    };
    let other = *state
        .siblings()
        .iter()
        .find(|&&artifact| artifact != first)
        .expect("the area holds two");

    assert!(handle_hover(&mut state, Some(ViewHit::SelectItem(other))));
    assert_eq!(state.choosing, Some(Choosing::Item(other)));
    assert!(
        !handle_hover(&mut state, Some(ViewHit::SelectItem(other))),
        "standing still costs no frame"
    );
    assert_eq!(state.selected, first, "hovering chooses nothing");

    state.choosing = None;
    assert!(
        !handle_hover(&mut state, Some(ViewHit::SelectItem(other))),
        "and with no list open it is the board's pointer, not a menu's"
    );
}

/// A boundary can only be read as one if a line through it means
/// something. Crossing it is what an edge with an end inside does; an
/// edge with neither end inside goes around.
#[test]
fn an_edge_with_no_business_in_a_region_stays_out_of_it() {
    let Diagram::Graph(graph) = mermaid::parse(
        "flowchart TD\n a --> m --> z\n a --> z\n subgraph region [Region]\n m\n end",
    )
    .expect("it parses") else {
        panic!("a flowchart is a graph");
    };
    let scene = Scene::of(graph);
    let region = scene.placement.clusters[0];
    let past = scene
        .routes
        .routes
        .iter()
        .find(|route| {
            let edge = &scene.graph.edges[route.edge];
            scene.graph.nodes[edge.from].cluster.is_none()
                && scene.graph.nodes[edge.to].cluster.is_none()
        })
        .expect("the edge that skips the region is routed");
    for &(x, y, _) in &past.cells {
        assert!(
            !region.contains(x, y),
            "({x},{y}) is inside a region the edge has no end in"
        );
    }
}

/// The board's grid is the texture of having nothing on it, so a region
/// stands on a ground of its own — and a region inside a region takes
/// its parent's ground back, which is what tells two nested walls apart.
#[test]
fn a_region_stands_on_its_own_ground() {
    let outer = Frame {
        x: 0,
        y: 0,
        w: 20,
        h: 20,
    };
    let inner = Frame {
        x: 5,
        y: 5,
        w: 5,
        h: 5,
    };
    assert!(grounded(&[outer, inner], (30, 30)));
    assert!(!grounded(&[outer, inner], (2, 2)));
    assert!(grounded(&[outer, inner], (6, 6)));
}

/// Leaving the surface and coming back is coming back: the diagram, the
/// levels entered to reach it, the box selected and where the board was
/// moved to are all where they were left.
#[test]
fn coming_back_stands_where_the_viewer_stood() {
    let mut left = showing("Containers");
    left.picked = node_named_in(&left, "core");
    let outcome = left.enter();
    assert_eq!(outcome, ArchitectOutcome::Stay, "core goes inside");
    left.picked = node_named_in(&left, "delivery");
    left.corner = Some((12, 34));

    let back = filled(ArchitectView::opening("~/project".to_owned()).resuming(left.place()));
    assert_eq!(back.selected, left.selected);
    assert_eq!(back.trail, left.trail);
    assert_eq!(back.picked, left.picked);
    assert_eq!(back.corner, Some((12, 34)));
}

/// A diagram that was deleted while the surface was shut is not an error
/// and not an empty board: the place is simply not restored.
#[test]
fn a_place_naming_a_diagram_that_is_gone_opens_at_the_top() {
    let place = ArchitectPlace {
        artifact: "vanished.mmd".to_owned(),
        ..ArchitectPlace::default()
    };
    let back = filled(ArchitectView::opening("~/project".to_owned()).resuming(place));
    assert_eq!(back.selected, 0);
    assert!(back.trail.is_empty());
}

/// The title names the checkout and the branch, the way the code
/// surface's does — the same sentence in the same place, because a
/// reader switching between the two is asking one question.
#[test]
fn the_title_says_the_checkout_the_way_the_code_surface_says_it() {
    let mut state = opened();
    state.display_root = "~/uze/.worktrees/joipv0".to_owned();
    state.branch = "feat/thing".to_owned();

    let title = view(&state, SPACE).title;
    let said: String = title.iter().map(|span| span.text.as_str()).collect();
    assert_eq!(said, "architect · ~/uze/.worktrees/joipv0 · feat/thing");

    let weight = |text: &str| {
        title
            .iter()
            .find(|span| span.text == text)
            .map(|span| (span.role, span.bold))
    };
    assert_eq!(weight("architect"), Some((Role::Muted, false)));
    assert_eq!(weight("~/uze/.worktrees/"), Some((Role::Dim, false)));
    assert_eq!(weight("joipv0"), Some((Role::Bright, true)));
    assert_eq!(weight("feat/thing"), Some((Role::Accent, true)));
}

/// The two surfaces' titles are one sentence with one word changed. Held
/// here because "they look the same today" is not the same claim as
/// "they are built the same way", and only the second one survives an
/// edit to either surface.
#[test]
fn both_surfaces_say_a_checkout_in_the_same_words() {
    let architect = crate::shared::checkout::title("architect", "~/uze/repo", "main");
    let code = crate::shared::checkout::title("code", "~/uze/repo", "main");
    assert_eq!(architect.len(), code.len());
    for (architect, code) in architect.iter().zip(&code).skip(1) {
        assert_eq!(architect, code);
    }
}
