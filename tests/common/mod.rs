//! Helpers shared by RetroSaurus's integration tests.
//!
//! Mirrors saudade's harness: the widget tree is rendered against the bundled
//! DejaVu fonts (so glyph rasterization is bit-identical regardless of the
//! host's installed fonts) and compared to a checked-in PNG baseline via
//! `insta::assert_binary_snapshot!`. Review diffs with `cargo insta review`.

#![allow(dead_code)]

use saudade::mock::MockBackend;
use saudade::{Event, Font, Widget};

pub fn sans_font() -> Font {
    Font::from_bytes(include_bytes!("../fonts/DejaVuSans.ttf").to_vec())
        .expect("bundled DejaVuSans.ttf failed to load")
}

pub fn mono_font() -> Font {
    Font::from_bytes(include_bytes!("../fonts/DejaVuSansMono.ttf").to_vec())
        .expect("bundled DejaVuSansMono.ttf failed to load")
}

pub const SCALE: f32 = 1.0;

/// A MockBackend wired with the deterministic bundled fonts.
pub fn backend(width: i32, height: i32) -> MockBackend {
    MockBackend::new(width, height)
        .with_scale(SCALE)
        .with_font(sans_font())
        .with_mono_font(mono_font())
}

/// Render `build()` and emit one binary insta snapshot named `<name>.png`.
pub fn snapshot<F>(name: &str, width: i32, height: i32, mut build: F)
where
    F: FnMut() -> Box<dyn Widget>,
{
    snapshot_one(name, width, height, build(), &[]);
}

/// Like [`snapshot`] but feeds a sequence of synthetic events into the freshly
/// built widget (after a layout at the target size) before rendering — to
/// capture interaction states (a typed query, a selected row) deterministically.
pub fn snapshot_with_events<F, E>(name: &str, width: i32, height: i32, mut build: F, events: E)
where
    F: FnMut() -> Box<dyn Widget>,
    E: Fn() -> Vec<Event>,
{
    snapshot_one(name, width, height, build(), &events());
}

fn snapshot_one(
    name: &str,
    width: i32,
    height: i32,
    mut widget: Box<dyn Widget>,
    events: &[Event],
) {
    let backend = MockBackend::new(width, height)
        .with_scale(SCALE)
        .with_font(sans_font())
        .with_mono_font(mono_font());

    // Warm-up render so paint-time geometry (the definition pane's wrapped
    // layout, menu label rects) is ready, then focus and dispatch.
    let _ = backend.render(widget.as_mut());
    widget.focus_first();
    for event in events {
        backend.dispatch(widget.as_mut(), event);
    }

    let snap = backend.render(widget.as_mut());
    let snap_name = format!("{name}.png");
    let mut settings = insta::Settings::clone_current();
    settings.set_prepend_module_to_snapshot(false);
    settings.set_snapshot_path("../snapshots");
    settings.bind(|| {
        insta::assert_binary_snapshot!(snap_name.as_str(), snap.to_png());
    });
}
