//! retrosaurus — a Windows 3.1-styled thesaurus and dictionary.
//!
//! The binary is deliberately thin: it decodes the embedded WordNet index,
//! builds the root [`RetroSaurus`](retrosaurus::ui::RetroSaurus) widget, and
//! hands it to saudade's runtime. Everything testable lives in the library.

use std::process::ExitCode;
use std::rc::Rc;

use retrosaurus::thesaurus::{EmbeddedThesaurus, Thesaurus};
use retrosaurus::ui::RetroSaurus;
use saudade::{App, Theme, WindowConfig};

/// Initial window size, in logical pixels. Saudade's logical pixel targets
/// 72 dpi, so this opens at the same physical size 660×470 did at 96 dpi.
const WINDOW_W: i32 = 495;
const WINDOW_H: i32 = 352;

fn main() -> ExitCode {
    let thesaurus: Rc<dyn Thesaurus> = match EmbeddedThesaurus::load() {
        Ok(thesaurus) => Rc::new(thesaurus),
        Err(err) => {
            eprintln!("retrosaurus: failed to load the embedded word index: {err}");
            return ExitCode::FAILURE;
        }
    };

    let root = RetroSaurus::new(thesaurus).with_initial_word("dinosaur");

    App::new(
        WindowConfig::new("RetroSaurus", WINDOW_W, WINDOW_H).resizable(true),
        root,
    )
    .with_theme(Theme::windows_31())
    .run();

    ExitCode::SUCCESS
}
