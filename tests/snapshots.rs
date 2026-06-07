//! Pixel snapshot tests for the RetroSaurus UI, driven against the in-memory
//! [`Fixture`] so they never touch the embedded WordNet index.

mod common;

use std::rc::Rc;

use retrosaurus::thesaurus::{Fixture, Thesaurus};
use retrosaurus::ui::RetroSaurus;
use saudade::{Event, Modifiers, Widget};

const W: i32 = 660;
const H: i32 = 470;

fn app() -> Box<dyn Widget> {
    let thesaurus: Rc<dyn Thesaurus> = Rc::new(Fixture::sample());
    Box::new(RetroSaurus::new(thesaurus))
}

/// Synthesize the `Char` events for typing `text` into the focused search field.
fn type_str(text: &str) -> Vec<Event> {
    text.chars()
        .map(|ch| Event::Char {
            ch,
            modifiers: Modifiers::default(),
        })
        .collect()
}

#[test]
fn start_screen() {
    common::snapshot("start_screen", W, H, app);
}

#[test]
fn query_happy() {
    common::snapshot_with_events("query_happy", W, H, app, || type_str("happy"));
}

#[test]
fn query_abandon() {
    common::snapshot_with_events("query_abandon", W, H, app, || type_str("abandon"));
}

#[test]
fn query_dog() {
    common::snapshot_with_events("query_dog", W, H, app, || type_str("dog"));
}
