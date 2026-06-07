//! Renders the README screenshot straight from the real [`RetroSaurus`] widget.
//!
//! It drives the embedded Open English WordNet index through saudade's offscreen
//! [`MockBackend`] (with the bundled DejaVu fonts, so glyph rasterization is
//! identical regardless of the host's installed fonts), then wraps the window in
//! Canoe-style chrome (a title bar, frame and drop shadow) with
//! [`MockBackend::render_framed`] so the image looks the way a user sees the app
//! on the desktop. No windowing system is needed and the output is byte-stable
//! across machines. Run it from the crate root to refresh `docs/screenshot.png`:
//!
//! ```sh
//! cargo run --example screenshots
//! ```

use std::path::Path;
use std::rc::Rc;

use retrosaurus::thesaurus::{EmbeddedThesaurus, Thesaurus};
use retrosaurus::ui::RetroSaurus;
use saudade::mock::MockBackend;
use saudade::{Event, Font, Modifiers, Widget, WindowChrome};

const WINDOW_W: i32 = 660;
const WINDOW_H: i32 = 470;
// Capture at 2× so the README image stays crisp on hi-DPI displays.
const SCALE: f32 = 2.0;

fn main() {
    let thesaurus: Rc<dyn Thesaurus> =
        Rc::new(EmbeddedThesaurus::load().expect("embedded WordNet index"));
    let mut app = RetroSaurus::new(thesaurus);

    let backend = MockBackend::new(WINDOW_W, WINDOW_H)
        .with_scale(SCALE)
        .with_font(sans_font())
        .with_mono_font(mono_font());

    // Warm-up render, focus the search field, then type an on-theme word with a
    // rich entry (definitions, examples, synonyms, antonyms, related words).
    let _ = backend.render(&mut app);
    app.focus_first();
    for event in type_text("extinct") {
        backend.dispatch(&mut app, &event);
    }

    let chrome = WindowChrome::resizable("RetroSaurus");
    let png = backend.render_framed(&mut app, &chrome).to_png();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs")
        .join("screenshot.png");
    std::fs::create_dir_all(path.parent().unwrap()).expect("create docs/");
    std::fs::write(&path, png).expect("write screenshot");
    println!("wrote {}", path.display());
}

fn sans_font() -> Font {
    Font::from_bytes(include_bytes!("../tests/fonts/DejaVuSans.ttf").to_vec())
        .expect("bundled DejaVuSans.ttf failed to load")
}

fn mono_font() -> Font {
    Font::from_bytes(include_bytes!("../tests/fonts/DejaVuSansMono.ttf").to_vec())
        .expect("bundled DejaVuSansMono.ttf failed to load")
}

fn type_text(s: &str) -> Vec<Event> {
    s.chars()
        .map(|ch| Event::Char {
            ch,
            modifiers: Modifiers::default(),
        })
        .collect()
}
