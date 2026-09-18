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

#[test]
fn a_line_never_outgrows_the_space_it_was_given() {
    let state = ArchitectView::opening();
    let space = Size {
        width: 60,
        height: 20,
    };
    let Content::Lines { lines, total, .. } = view(&state, space).content else {
        panic!("a diagram is lines");
    };
    assert_eq!(lines.len(), total);
    for line in &lines {
        let width: i32 = line.spans.iter().map(|s| canvas::text_width(&s.text)).sum();
        assert!(width <= 56, "{width} columns would wrap");
    }
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
    handle_mouse(&mut state, Some(hit));
    assert_eq!(state.picked, Some(0));
    handle_mouse(&mut state, Some(hit));
    assert_eq!(state.picked, None);
}
