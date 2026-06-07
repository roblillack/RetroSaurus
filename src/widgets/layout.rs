//! Pure geometry for the RetroSaurus window.
//!
//! The [`Shell`](crate::widgets::Shell) places each pane by calling one of
//! these with the window bounds: a menu bar and a search toolbar pinned to the
//! top, then a fixed-width result list on the left and the definition pane
//! filling the rest.

use saudade::Rect;

/// Height of the menu bar.
pub const MENU_H: i32 = 20;
/// Height of the search toolbar below the menu.
pub const TOOLBAR_H: i32 = 28;
/// Width of the word-result list on the left.
pub const LIST_W: i32 = 196;
/// Breathing room around and between the two panes.
pub const PAD: i32 = 6;

pub fn menu(b: Rect) -> Rect {
    Rect::new(b.x, b.y, b.w, MENU_H)
}

pub fn toolbar(b: Rect) -> Rect {
    Rect::new(b.x, b.y + MENU_H, b.w, TOOLBAR_H)
}

/// Top of the padded content area, below the menu and toolbar.
fn content_y(b: Rect) -> i32 {
    b.y + MENU_H + TOOLBAR_H + PAD
}

/// Height available to the two panes, after top and bottom padding.
fn content_h(b: Rect) -> i32 {
    (b.h - MENU_H - TOOLBAR_H - 2 * PAD).max(0)
}

/// Width of the list, clamped so the definition pane keeps a usable minimum.
fn list_w(b: Rect) -> i32 {
    LIST_W.min((b.w - 3 * PAD - 120).max(0))
}

pub fn list(b: Rect) -> Rect {
    Rect::new(b.x + PAD, content_y(b), list_w(b), content_h(b))
}

pub fn detail(b: Rect) -> Rect {
    let x = b.x + 2 * PAD + list_w(b);
    let w = (b.right() - x - PAD).max(0);
    Rect::new(x, content_y(b), w, content_h(b))
}
