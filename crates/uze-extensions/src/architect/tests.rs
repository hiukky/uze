use super::*;

fn drawn(sample: usize, glyphs: Glyphs) -> String {
    match mermaid::parse(SAMPLES[sample].source).expect("the sample parses") {
        Diagram::Graph(graph) => paint::paint(&Scene::of(graph), glyphs, None).to_text(),
        Diagram::Sequence(sequence) => sequence::paint(&sequence, glyphs).to_text(),
    }
}

/// Run with `--nocapture` to read the diagrams: this is the quickest way
/// to look at what the layout produced without opening the TUI.
#[test]
fn every_sample_is_drawn_with_every_edge_routed() {
    for (index, sample) in SAMPLES.iter().enumerate() {
        if let Diagram::Graph(graph) = mermaid::parse(sample.source).unwrap() {
            let scene = Scene::of(graph);
            assert_eq!(scene.routes.unrouted, 0, "{} left an edge out", sample.name);
        }
        let text = drawn(index, Glyphs::Unicode);
        println!("\n=== {} / {} ===\n{text}", sample.group, sample.name);
        assert!(text.lines().count() > 5);
    }
}

#[test]
fn ascii_draws_the_same_diagram_in_seven_bit_characters() {
    for index in 0..SAMPLES.len() {
        let text = drawn(index, Glyphs::Ascii);
        let foreign: Vec<char> = text
            .chars()
            .filter(|c| !c.is_ascii() && !c.is_alphanumeric() && *c != '·' && *c != '~')
            .collect();
        assert!(foreign.is_empty(), "{foreign:?} in {}", SAMPLES[index].name);
    }
}

const SPACE: Size = Size {
    width: 60,
    height: 20,
};

#[test]
fn the_screen_is_exactly_the_space_it_was_given() {
    let state = ArchitectView::opening();
    let Content::Lines { lines, scroll, .. } = view(&state, SPACE).content else {
        panic!("a diagram is lines");
    };
    assert_eq!(lines.len(), usize::from(scroll) + 20);
    for line in &lines {
        let width: i32 = line.spans.iter().map(|s| canvas::text_width(&s.text)).sum();
        assert!(width <= 60, "{width} columns would be cut");
    }
}

#[test]
fn a_dragged_board_follows_the_pointer_and_stops_at_its_edges() {
    let mut state = ArchitectView::opening();
    drag_by(&mut state, -30, -10, SPACE);
    assert_eq!(state.corner, (30, 10));
    drag_by(&mut state, 12, 4, SPACE);
    assert_eq!(state.corner, (18, 6));
    drag_by(&mut state, 500, 500, SPACE);
    assert_eq!(state.corner, (0, 0));
    drag_by(&mut state, -5000, -5000, SPACE);
    let board = state.board_size();
    assert_eq!(state.corner, (board.0 - 60, board.1 - 20));
}

#[test]
fn a_board_narrower_than_the_screen_sits_in_the_middle_of_it() {
    let mut state = ArchitectView::opening();
    handle_mouse(&mut state, Some(ViewHit::SelectItem(2)), SPACE);
    let wide = Size {
        width: 200,
        height: 40,
    };
    let inset = state.inset(wide);
    assert_eq!(inset.0, (200 - state.board_size().0) / 2);
    let Drawing::Graph(scene) = &state.drawing else {
        panic!("the context view is a graph");
    };
    let frame = scene.placement.nodes[0];
    let hit = ViewHit::PlaceCaret {
        line: (frame.y + 1 + inset.1) as usize,
        cell: (frame.x + 1 + inset.0) as usize,
    };
    handle_mouse(&mut state, Some(hit), wide);
    assert_eq!(state.picked, Some(0));
}

#[test]
fn a_click_on_the_minimap_brings_that_part_of_the_board_to_the_screen() {
    let mut state = ArchitectView::opening();
    let screen = Size {
        width: 100,
        height: 30,
    };
    let map = state
        .minimap(screen)
        .expect("the layering board outgrows the screen");
    let hit = ViewHit::PlaceCaret {
        line: (map.frame.y + map.frame.h - 2) as usize,
        cell: (map.frame.x + map.frame.w - 2) as usize,
    };
    handle_mouse(&mut state, Some(hit), screen);
    assert!(
        state.corner.0 > 40 && state.corner.1 > 20,
        "{:?}",
        state.corner
    );
    assert_eq!(state.picked, None);
}

#[test]
fn the_diagrams_go_round() {
    let mut state = ArchitectView::opening();
    handle_command(&mut state, Command::PreviousView, SPACE);
    assert_eq!(state.sample, SAMPLES.len() - 1);
    handle_command(&mut state, Command::NextView, SPACE);
    assert_eq!(state.sample, 0);
}

#[test]
fn clicking_a_box_selects_it_and_clicking_it_again_lets_go() {
    let mut state = ArchitectView::opening();
    let Drawing::Graph(scene) = &state.drawing else {
        panic!("the first sample is a graph");
    };
    let frame = scene.placement.nodes[0];
    let hit = ViewHit::PlaceCaret {
        line: (frame.y + 1) as usize,
        cell: (frame.x + 1) as usize,
    };
    let wide = Size {
        width: 400,
        height: 20,
    };
    let inset = state.inset(wide);
    let hit = match hit {
        ViewHit::PlaceCaret { line, cell } => ViewHit::PlaceCaret {
            line,
            cell: cell + inset.0 as usize,
        },
        other => other,
    };
    handle_mouse(&mut state, Some(hit), wide);
    assert_eq!(state.picked, Some(0));
    handle_mouse(&mut state, Some(hit), wide);
    assert_eq!(state.picked, None);
}
