use super::*;

const SPACE: Size = Size {
    width: 100,
    height: 30,
};

/// uze's own architecture, as the repository keeps it. Living fixtures:
/// what is checked here is also that the project's real artifacts draw.
fn opened() -> ArchitectView {
    let mut state = ArchitectView::opening();
    let artifacts = vec![
        Artifact::read(
            "containers.mmd",
            include_str!("../../../../docs/architecture/diagrams/containers.mmd"),
        ),
        Artifact::read(
            "system-context.mmd",
            include_str!("../../../../docs/architecture/diagrams/system-context.mmd"),
        ),
        Artifact::read(
            "install-sequence.mmd",
            include_str!("../../../../docs/architecture/diagrams/install-sequence.mmd"),
        ),
        Artifact::read(
            "crate-layering.mmd",
            include_str!("../../../../docs/architecture/diagrams/crate-layering.mmd"),
        ),
        Artifact::read(
            "install-pipeline.mmd",
            include_str!("../../../../docs/architecture/diagrams/install-pipeline.mmd"),
        ),
        Artifact::read(
            "core-components.mmd",
            include_str!("../../../../docs/architecture/diagrams/core-components.mmd"),
        ),
    ];
    state.absorb(ArtifactsAnswer::Found {
        artifacts,
        project: PathBuf::from("/project"),
    });
    state
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
        let width: i32 = line.spans.iter().map(|s| canvas::text_width(&s.text)).sum();
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
    let mut state = ArchitectView::opening();
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
    state.absorb(read_artifacts(&Bare, ArtifactSource::Undeclared));
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
    state.absorb(read_artifacts(&Bare, empty));
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
    assert_eq!(names, ["Crate layering", "Install pipeline"]);
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

fn measured() -> CodeMeasure {
    let file = |path: &str, lines: u32, commits: u32| codemap::FileMeasure {
        path: path.to_owned(),
        lines,
        commits,
        changed: false,
    };
    CodeMeasure {
        root: PathBuf::from("/checkout"),
        files: vec![
            file("src/main.rs", 900, 12),
            file("src/ui/render.rs", 700, 30),
            file("src/ui/input.rs", 400, 3),
            file("docs/guide.md", 300, 1),
        ],
    }
}

/// Selects the tile of `path` the way a viewer does — a click on it —
/// rather than by walking the arrows to wherever the layout put it.
fn click_tile(state: &mut ArchitectView, path: &str, space: Size) {
    let cells = (i32::from(space.width), i32::from(space.height));
    let map = state.code.as_mut().expect("measured");
    let frame = map
        .tiles(cells)
        .into_iter()
        .find(|tile| tile.path == path)
        .unwrap_or_else(|| panic!("no tile for {path}"))
        .frame;
    let (x, y) = frame.center();
    map.click(x, y, cells);
}

fn areas_listed(state: &ArchitectView) -> Vec<String> {
    view(state, SPACE)
        .navigator
        .expect("a menu")
        .rows
        .into_iter()
        .filter_map(|row| match row {
            NavigatorRow::Group { name, .. } => Some(name),
            _ => None,
        })
        .collect()
}

#[test]
fn the_code_map_is_listed_last_and_a_late_measurement_moves_nothing() {
    let mut state = showing("Containers");
    let before = state.selected;
    state.absorb_code(measured());
    assert_eq!(state.selected, before, "what was on show stays on show");
    assert_eq!(
        areas_listed(&state),
        ["C4", "Sequence", "Flowchart", "Code"]
    );
    assert_eq!(
        state.catalog.artifacts().last().map(|a| a.name.as_str()),
        Some("Code map")
    );
    assert_eq!(state.insides.len(), state.catalog.artifacts().len());
}

#[test]
fn a_project_that_declares_nothing_still_has_its_code_map() {
    let mut state = ArchitectView::opening();
    state.absorb_code(measured());
    assert!(
        matches!(state.drawing, Drawing::Code),
        "shown as soon as it is measured"
    );
    assert!(
        !state.caption(SPACE).contains("declares"),
        "nothing is the matter yet"
    );
    state.absorb(ArtifactsAnswer::Nothing {
        text: "This project declares no artifacts yet".to_owned(),
        hint: "Add `artifacts:` to agents.yaml".to_owned(),
    });
    assert!(matches!(state.drawing, Drawing::Code));
    let Content::Lines { heading, lines, .. } = view(&state, SPACE).content else {
        panic!("the map is drawn");
    };
    assert!(heading.contains("declares no artifacts"), "{heading}");
    assert!(heading.starts_with("4 files · 2,300 lines"), "{heading}");
    assert_eq!(lines.len(), usize::from(SPACE.height), "it fills the space");

    // The other way round: the declaration answered first.
    let mut state = ArchitectView::opening();
    state.absorb(ArtifactsAnswer::Nothing {
        text: "This project declares no artifacts yet".to_owned(),
        hint: "Add `artifacts:` to agents.yaml".to_owned(),
    });
    state.absorb_code(measured());
    assert!(matches!(state.drawing, Drawing::Code));
    assert!(state.nothing.is_none());
}

#[test]
fn the_map_is_entered_and_left_and_a_file_on_it_opens_the_code() {
    let mut state = ArchitectView::opening();
    state.absorb_code(measured());
    state.absorb(ArtifactsAnswer::Nothing {
        text: "declares nothing".to_owned(),
        hint: String::new(),
    });
    click_tile(&mut state, "src", SPACE);
    assert!(state.caption(SPACE).starts_with("src/ "));
    assert_eq!(
        handle_command(&mut state, Command::Activate, SPACE),
        ArchitectOutcome::Stay
    );
    assert_eq!(trail(&state), ["Code map", "src"]);
    assert_eq!(
        state.corner(SPACE),
        (0, 0),
        "a map that fits is never moved"
    );
    assert!(state.minimap(SPACE).is_none());

    click_tile(&mut state, "src/main.rs", SPACE);
    assert!(state.caption(SPACE).starts_with("src/main.rs "));
    assert_eq!(
        handle_command(&mut state, Command::Activate, SPACE),
        ArchitectOutcome::OpenPath {
            project: PathBuf::from("/checkout"),
            target: PathBuf::from("/checkout/src/main.rs"),
        }
    );

    handle_command(&mut state, Command::Back, SPACE);
    assert_eq!(trail(&state), ["Code map"]);
    assert!(
        state.caption(SPACE).starts_with("src/ "),
        "back with the directory that was entered selected: {}",
        state.caption(SPACE)
    );
}

#[test]
fn the_map_is_laid_out_again_for_a_narrower_space() {
    let mut state = ArchitectView::opening();
    state.absorb_code(measured());
    state.absorb(ArtifactsAnswer::Nothing {
        text: String::new(),
        hint: String::new(),
    });
    let narrow = Size {
        width: 60,
        height: 18,
    };
    for space in [SPACE, narrow] {
        let Content::Lines { lines, .. } = view(&state, space).content else {
            panic!("the map is drawn");
        };
        assert_eq!(lines.len(), usize::from(space.height));
        let widest = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.text.chars().count())
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(0);
        assert_eq!(
            widest,
            usize::from(space.width),
            "filled edge to edge at {space:?}"
        );
    }
}

/// The key that closes peels one level at a time. Anything else makes a
/// surface that can be entered but not looked around in: one press and
/// the viewer is back where they started with three levels of work gone.
#[test]
fn the_key_that_closes_goes_up_a_level_until_there_is_none_left() {
    let mut state = ArchitectView::opening();
    state.absorb_code(measured());
    state.absorb(ArtifactsAnswer::Nothing {
        text: "declares nothing".to_owned(),
        hint: String::new(),
    });

    click_tile(&mut state, "src", SPACE);
    assert_eq!(
        handle_command(&mut state, Command::Close, SPACE),
        ArchitectOutcome::Stay,
        "the selection is let go of first"
    );
    assert!(
        state.caption(SPACE).starts_with("4 files"),
        "nothing picked"
    );

    click_tile(&mut state, "src", SPACE);
    handle_command(&mut state, Command::Activate, SPACE);
    click_tile(&mut state, "src/ui", SPACE);
    handle_command(&mut state, Command::Activate, SPACE);
    assert_eq!(trail(&state), ["Code map", "src", "ui"]);

    for left in [["Code map", "src"].as_slice(), ["Code map"].as_slice()] {
        assert_eq!(
            handle_command(&mut state, Command::Close, SPACE),
            ArchitectOutcome::Stay
        );
        // Coming back selects what was entered, which is a level of its
        // own — the next press lets go of it, the one after goes up.
        assert_eq!(
            handle_command(&mut state, Command::Close, SPACE),
            ArchitectOutcome::Stay
        );
        assert_eq!(trail(&state), left);
    }
    assert_eq!(trail(&state), ["Code map"], "back at the top");
    assert_eq!(
        handle_command(&mut state, Command::Close, SPACE),
        ArchitectOutcome::Close,
        "and only there does it close"
    );
}

/// The way up is named in the footer only where there is one.
#[test]
fn the_footer_offers_the_way_up_only_once_something_has_been_entered() {
    let mut state = ArchitectView::opening();
    state.absorb_code(measured());
    state.absorb(ArtifactsAnswer::Nothing {
        text: String::new(),
        hint: String::new(),
    });
    assert!(!view(&state, SPACE).footer.contains(&Command::Back));

    click_tile(&mut state, "src", SPACE);
    handle_command(&mut state, Command::Activate, SPACE);
    assert!(view(&state, SPACE).footer.contains(&Command::Back));
}

/// A selector that offers only what is on show is not opened at all, by
/// the pointer or by the key — the host draws it without the mark that
/// says it opens, and there is nothing behind it to draw.
#[test]
fn a_selector_with_nothing_to_choose_stays_shut() {
    let mut state = ArchitectView::opening();
    state.absorb_code(measured());
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
