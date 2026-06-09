//! End-to-end navigation: typing filters the list and shows the top match,
//! clicking a cross-reference jumps to that word, and Back returns — all driven
//! against the in-memory [`Fixture`].

mod common;

use std::rc::Rc;

use retrosaurus::thesaurus::{Fixture, Thesaurus};
use retrosaurus::ui::RetroSaurus;
use saudade::{Event, Key, Modifiers, MouseButton, NamedKey, Point, Widget};

const W: i32 = 660;
const H: i32 = 470;

fn type_str(text: &str) -> Vec<Event> {
    text.chars()
        .map(|ch| Event::Char {
            ch,
            modifiers: Modifiers::default(),
        })
        .collect()
}

fn alt_left() -> Event {
    Event::KeyDown {
        key: Key::Named(NamedKey::Left),
        modifiers: Modifiers {
            alt: true,
            ..Modifiers::default()
        },
    }
}

fn ctrl(c: char) -> Event {
    Event::KeyDown {
        key: Key::Char(c),
        modifiers: Modifiers {
            control: true,
            ..Modifiers::default()
        },
    }
}

fn alt(c: char) -> Event {
    Event::KeyDown {
        key: Key::Char(c),
        modifiers: Modifiers {
            alt: true,
            ..Modifiers::default()
        },
    }
}

#[test]
fn typing_shows_top_match_then_link_navigates_and_back_returns() {
    let backend = common::backend(W, H);
    let thesaurus: Rc<dyn Thesaurus> = Rc::new(Fixture::sample());
    let mut app = RetroSaurus::new(thesaurus);

    // Warm-up render, focus the search field, type "abandon".
    let _ = backend.render(&mut app);
    app.focus_first();
    for event in type_str("abandon") {
        backend.dispatch(&mut app, &event);
    }
    // Render so the definition pane lays out and links become hit-testable.
    let _ = backend.render(&mut app);
    assert_eq!(app.shown_lemma().as_deref(), Some("abandon"));

    // Click the "forsake" cross-reference (located via the widget, not a magic
    // pixel) and confirm we navigate to it.
    let rect = app
        .link_rect("forsake")
        .expect("a 'forsake' link should be present in abandon's definition");
    let center = Point::new(rect.x + rect.w / 2, rect.y + rect.h / 2);
    backend.dispatch(
        &mut app,
        &Event::PointerDown {
            pos: center,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    );
    let _ = backend.render(&mut app);
    assert_eq!(app.shown_lemma().as_deref(), Some("forsake"));

    // Back returns to "abandon".
    backend.dispatch(&mut app, &alt_left());
    let _ = backend.render(&mut app);
    assert_eq!(app.shown_lemma().as_deref(), Some("abandon"));
}

/// Ctrl+L and Alt+W focus the word field and select its whole contents, so the
/// next keystroke replaces the query rather than appending to it.
#[test]
fn focus_word_field_accelerators_select_all() {
    for accelerator in [ctrl('l'), alt('w')] {
        let backend = common::backend(W, H);
        let thesaurus: Rc<dyn Thesaurus> = Rc::new(Fixture::sample());
        let mut app = RetroSaurus::new(thesaurus);

        let _ = backend.render(&mut app);
        app.focus_first();
        for event in type_str("abandon") {
            backend.dispatch(&mut app, &event);
        }
        let _ = backend.render(&mut app);
        assert_eq!(app.shown_lemma().as_deref(), Some("abandon"));

        // The accelerator selects the whole field; typing replaces it.
        backend.dispatch(&mut app, &accelerator);
        for event in type_str("happy") {
            backend.dispatch(&mut app, &event);
        }
        let _ = backend.render(&mut app);
        assert_eq!(
            app.shown_lemma().as_deref(),
            Some("happy"),
            "typing after the focus accelerator should replace, not append"
        );
    }
}
