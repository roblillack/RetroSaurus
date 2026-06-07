//! RetroSaurus — a Windows 3.1-flavored thesaurus and dictionary built on the
//! saudade toolkit, shipping the Open English WordNet for offline use.
//!
//! The crate is split so the UI is testable without the multi-megabyte word
//! index:
//!
//! * [`thesaurus`] — the [`Thesaurus`](thesaurus::Thesaurus) trait and its
//!   denormalized view types, the embedded-index reader, the on-disk format
//!   shared with `build.rs`, and a small in-memory fixture for snapshot tests;
//! * [`widgets`] — dictionary-specific widgets (the search bar, the result
//!   list, the scrollable clickable definition view) layered on saudade's
//!   generic widget set;
//! * [`ui`] — the top-level [`RetroSaurus`](ui::RetroSaurus) widget that wires
//!   the panes together and drives navigation.

pub mod thesaurus;
pub mod ui;
pub mod widgets;
