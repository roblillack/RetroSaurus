//! Dictionary-specific widgets layered on saudade's generic set.

pub mod definition_view;
pub mod layout;
pub mod search_bar;
pub mod shared;
pub mod shell;

pub use definition_view::{DefinitionView, Line, RunStyle, Span, build_document};
pub use search_bar::SearchBar;
pub use shared::Shared;
pub use shell::Shell;
