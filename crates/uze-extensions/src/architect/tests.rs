use super::*;

const SPACE: Size = Size {
    width: 100,
    height: 30,
};

fn showing(name: &str) -> ArchitectView {
    let mut state = ArchitectView::opening();
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
    let mut state = ArchitectView::opening();
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
    let mut state = ArchitectView::opening();
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
    let state = ArchitectView::opening();
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
    let mut state = ArchitectView::opening();
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
    let mut state = ArchitectView::opening();
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
    let mut state = ArchitectView::opening();
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
