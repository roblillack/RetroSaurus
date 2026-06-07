//! The top-level [`RetroSaurus`] widget.
//!
//! A single flat [`Shell`] holds the menu bar, the search toolbar, the
//! word-result list and the definition pane. saudade widgets are callback-free,
//! so the cross-pane wiring lives here: after each event the search text and
//! list selection are polled, the result list and definition document are
//! rebuilt from the [`Thesaurus`], and a small command queue (menu items,
//! keyboard accelerators) is drained. Clicking a cross-reference in the
//! definition pane drives back/forward navigation through the same path.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use saudade::{
    Dialog, Event, EventCtx, Key, List, ListItem, Menu, MenuBar, MenuItem, NamedKey, Painter,
    PopupRequest, Rect, Theme, Widget,
};

use crate::thesaurus::{Thesaurus, WordId};
use crate::widgets::{DefinitionView, SearchBar, Shared, Shell, build_document, layout};

/// Maximum number of prefix matches shown in the result list.
const MAX_RESULTS: usize = 500;

// Direct-child indices in the shell (set by `add` order).
const SEARCH_IDX: usize = 1;
const LIST_IDX: usize = 2;

/// Deferred actions menu items / accelerators request; drained after event
/// dispatch so they can mutate state the callbacks can't reach.
#[derive(Clone, Copy)]
enum AppCommand {
    Back,
    Forward,
    Random,
}

pub struct RetroSaurus {
    thesaurus: Rc<dyn Thesaurus>,
    bounds: Rect,
    root: Shell,

    search: Rc<RefCell<SearchBar>>,
    word_list: Rc<RefCell<List>>,
    definition: Rc<RefCell<DefinitionView>>,
    dialog: Rc<RefCell<Dialog>>,
    commands: Rc<RefCell<Vec<AppCommand>>>,

    // ---- sync + navigation state ----
    /// Word ids backing the current result-list rows.
    results: Vec<WordId>,
    last_query: String,
    /// The head word currently displayed in the definition pane.
    shown: Option<WordId>,
    back: Vec<WordId>,
    forward: Vec<WordId>,
    rng: u64,
}

impl RetroSaurus {
    pub fn new(thesaurus: Rc<dyn Thesaurus>) -> Self {
        let dialog = Rc::new(RefCell::new(Dialog::new()));
        let commands: Rc<RefCell<Vec<AppCommand>>> = Rc::new(RefCell::new(Vec::new()));

        let search = Rc::new(RefCell::new(SearchBar::new(Rect::new(0, 0, 0, 0))));
        let word_list = Rc::new(RefCell::new(List::new(Rect::new(0, 0, 0, 0))));
        let definition = Rc::new(RefCell::new(DefinitionView::new(Rect::new(0, 0, 0, 0))));

        // Add order is also the Tab focus order: search → list → definition.
        // The menu bar (index 0) isn't focusable; it works via accelerators.
        let root = Shell::new()
            .add(build_menu(&commands, &dialog), layout::menu)
            .add(Shared::new(search.clone()), layout::toolbar)
            .add(Shared::new(word_list.clone()), layout::list)
            .add(Shared::new(definition.clone()), layout::detail)
            .add_overlay(Shared::new(dialog.clone()));

        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
            | 1;

        Self {
            thesaurus,
            bounds: Rect::new(0, 0, 0, 0),
            root,
            search,
            word_list,
            definition,
            dialog,
            commands,
            results: Vec::new(),
            last_query: String::new(),
            shown: None,
            back: Vec::new(),
            forward: Vec::new(),
            rng: seed,
        }
    }

    /// The lemma currently displayed in the definition pane, if any. Exposed
    /// for tests and embedders that want to observe navigation.
    pub fn shown_lemma(&self) -> Option<String> {
        self.shown
            .and_then(|w| self.thesaurus.lemma(w).map(String::from))
    }

    /// The on-screen rect of the cross-reference link to `lemma` in the current
    /// definition layout, if present. Exposed for tests that click a link.
    #[doc(hidden)]
    pub fn link_rect(&self, lemma: &str) -> Option<Rect> {
        let word = self.thesaurus.lookup(lemma)?;
        self.definition.borrow().link_rect_for(word)
    }

    /// Open RetroSaurus showing `lemma` (if it exists). Used by the binary to
    /// land on a friendly first word instead of a blank pane.
    pub fn with_initial_word(self, lemma: &str) -> Self {
        let mut me = self;
        if let Some(word) = me.thesaurus.lookup(lemma) {
            me.navigate_to(word, false);
            me.shown = Some(word);
            me.show_word(Some(word));
        }
        me
    }

    fn next_rand(&mut self) -> u64 {
        // SplitMix64-style step — plenty random for "pick a word".
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Replace the result list and select `select` (falling back to the first
    /// row when it isn't present).
    fn set_results(&mut self, results: Vec<WordId>, select: Option<WordId>) {
        self.results = results;
        let items: Vec<ListItem> = self
            .results
            .iter()
            .filter_map(|&w| self.thesaurus.lemma(w))
            .map(ListItem::new)
            .collect();
        let mut list = self.word_list.borrow_mut();
        list.set_items(items);
        let pos = select
            .and_then(|w| self.results.iter().position(|&r| r == w))
            .or(if self.results.is_empty() {
                None
            } else {
                Some(0)
            });
        list.set_selected(pos);
    }

    /// Re-filter the result list for `query`, keeping the shown word selected
    /// when it survives.
    fn rebuild_results(&mut self, query: &str) {
        let results = if query.is_empty() {
            Vec::new()
        } else {
            self.thesaurus.search_prefix(query, MAX_RESULTS)
        };
        let keep = self.shown;
        self.set_results(results, keep);
    }

    /// Show `word`: sync the search field and list to it, optionally recording
    /// the current word on the back stack. Does not itself build the document —
    /// `sync` does, off the resulting selection change.
    fn navigate_to(&mut self, word: WordId, push_history: bool) {
        if push_history {
            if let Some(current) = self.shown {
                self.back.push(current);
            }
            self.forward.clear();
        }
        let lemma = self.thesaurus.lemma(word).unwrap_or_default().to_string();
        self.search.borrow_mut().set_text(&lemma);
        self.last_query = lemma.clone();
        let mut results = self.thesaurus.search_prefix(&lemma, MAX_RESULTS);
        if !results.contains(&word) {
            results.insert(0, word);
        }
        self.set_results(results, Some(word));
    }

    fn go_back(&mut self) -> bool {
        let Some(prev) = self.back.pop() else {
            return false;
        };
        if let Some(current) = self.shown {
            self.forward.push(current);
        }
        self.navigate_to(prev, false);
        true
    }

    fn go_forward(&mut self) -> bool {
        let Some(next) = self.forward.pop() else {
            return false;
        };
        if let Some(current) = self.shown {
            self.back.push(current);
        }
        self.navigate_to(next, false);
        true
    }

    fn random_word(&mut self) -> bool {
        let count = self.thesaurus.word_count();
        if count == 0 {
            return false;
        }
        let idx = (self.next_rand() % count as u64) as usize;
        match self.thesaurus.word_at(idx) {
            Some(word) => {
                self.navigate_to(word, true);
                true
            }
            None => false,
        }
    }

    /// Build and install the definition document for `word` (or the placeholder).
    fn show_word(&mut self, word: Option<WordId>) {
        match word.and_then(|w| self.thesaurus.entry(w)) {
            Some(entry) => self
                .definition
                .borrow_mut()
                .set_document(build_document(&entry)),
            None => self.definition.borrow_mut().clear(),
        }
    }

    fn drain_commands(&mut self) -> bool {
        let pending: Vec<AppCommand> = self.commands.borrow_mut().drain(..).collect();
        let mut changed = false;
        for command in pending {
            changed |= match command {
                AppCommand::Back => self.go_back(),
                AppCommand::Forward => self.go_forward(),
                AppCommand::Random => self.random_word(),
            };
        }
        changed
    }

    /// Poll the panes after an event: query → result list, link click →
    /// navigation, selection → displayed definition.
    fn sync(&mut self) -> bool {
        let mut changed = false;

        // 1. Re-filter when the query changes (user typing).
        let query = self.search.borrow().text().trim().to_string();
        if query != self.last_query {
            self.last_query = query.clone();
            self.rebuild_results(&query);
            changed = true;
        }

        // 2. A clicked cross-reference navigates (and pushes history).
        let nav = self.definition.borrow_mut().take_navigation();
        if let Some(target) = nav {
            self.navigate_to(target, true);
            changed = true;
        }

        // 3. The selected row drives the definition pane.
        let sel = self.word_list.borrow().selected_index();
        let sel_word = sel.and_then(|i| self.results.get(i).copied());
        if sel_word != self.shown {
            self.shown = sel_word;
            self.show_word(sel_word);
            changed = true;
        }

        changed
    }

    /// Application accelerators, handled before the focused pane sees the event
    /// so they fire regardless of focus. Returns `true` when consumed.
    fn handle_shortcut(&mut self, event: &Event, ctx: &mut EventCtx) -> bool {
        if self.dialog.borrow().is_open() {
            return false;
        }
        let Event::KeyDown { key, modifiers } = event else {
            return false;
        };

        // Ctrl chords (no Alt / Logo).
        if modifiers.control
            && !modifiers.alt
            && !modifiers.logo
            && let Key::Char(c) = key
        {
            match c.to_ascii_lowercase() {
                'q' => {
                    ctx.close();
                    return true;
                }
                // Focus the search field and select its text — Ctrl+F (find)
                // and Ctrl+L (location bar, browser-style) both do this.
                'f' | 'l' => {
                    self.focus_search(ctx);
                    return true;
                }
                'r' => {
                    self.commands.borrow_mut().push(AppCommand::Random);
                    return true;
                }
                _ => {}
            }
        }

        // Left-Alt chords (AltGr composes, so it's excluded).
        if modifiers.mnemonic_alt() && !modifiers.control && !modifiers.logo {
            match key {
                Key::Named(NamedKey::Left) => {
                    self.commands.borrow_mut().push(AppCommand::Back);
                    return true;
                }
                Key::Named(NamedKey::Right) => {
                    self.commands.borrow_mut().push(AppCommand::Forward);
                    return true;
                }
                // Alt+W jumps to the word field and selects it.
                Key::Char(c) if c.eq_ignore_ascii_case(&'w') => {
                    self.focus_search(ctx);
                    return true;
                }
                _ => {}
            }
        }

        false
    }

    /// Move keyboard focus to the search field and select all of its text, so
    /// the next keystroke replaces the current query.
    fn focus_search(&mut self, ctx: &mut EventCtx) {
        self.root.focus_child(SEARCH_IDX);
        self.search.borrow_mut().select_all();
        ctx.request_paint();
    }
}

impl Widget for RetroSaurus {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn paint(&mut self, painter: &mut Painter, theme: &Theme) {
        self.root.paint(painter, theme);
    }

    fn paint_overlay(&mut self, painter: &mut Painter, theme: &Theme) {
        self.root.paint_overlay(painter, theme);
    }

    fn event(&mut self, event: &Event, ctx: &mut EventCtx) {
        if !self.handle_shortcut(event, ctx) {
            self.root.event(event, ctx);
        }
        let mut dirty = self.drain_commands();
        dirty |= self.sync();
        if dirty {
            ctx.request_paint();
        }
    }

    fn captures_pointer(&self) -> bool {
        self.root.captures_pointer()
    }

    fn focusable(&self) -> bool {
        self.root.focusable()
    }

    fn set_focused(&mut self, focused: bool) {
        self.root.set_focused(focused);
    }

    fn layout(&mut self, bounds: Rect) {
        self.bounds = bounds;
        self.root.layout(bounds);
    }

    fn focus_first(&mut self) -> bool {
        // Land on the search field so typing filters immediately.
        self.root.focus_child(SEARCH_IDX) || self.root.focus_child(LIST_IDX)
    }

    fn popup_request(&self) -> Option<PopupRequest> {
        self.root.popup_request()
    }

    fn wants_ticks(&self) -> bool {
        self.root.wants_ticks()
    }
}

/// Build the menu bar: File ▸ Exit, Go ▸ Back / Forward / Random, Help ▸ About.
fn build_menu(commands: &Rc<RefCell<Vec<AppCommand>>>, dialog: &Rc<RefCell<Dialog>>) -> MenuBar {
    MenuBar::new(Rect::new(0, 0, 0, 0))
        .add_menu(Menu::new(
            "&File",
            vec![MenuItem::action("E&xit", |cx| cx.close()).with_accel("Ctrl+Q")],
        ))
        .add_menu(Menu::new(
            "&Go",
            vec![
                cmd_item("&Back", commands, AppCommand::Back).with_accel("Alt+Left"),
                cmd_item("&Forward", commands, AppCommand::Forward).with_accel("Alt+Right"),
                MenuItem::separator(),
                cmd_item("&Random Word", commands, AppCommand::Random).with_accel("Ctrl+R"),
            ],
        ))
        .add_menu(Menu::new("&Help", vec![about_item(dialog)]))
}

/// A menu item that pushes `command` onto the deferred-command queue.
fn cmd_item(label: &str, commands: &Rc<RefCell<Vec<AppCommand>>>, command: AppCommand) -> MenuItem {
    let commands = commands.clone();
    MenuItem::action(label, move |cx| {
        commands.borrow_mut().push(command);
        cx.request_paint();
    })
}

/// The Help ▸ About item — carries the required Open English WordNet
/// attribution (CC BY 4.0).
fn about_item(dialog: &Rc<RefCell<Dialog>>) -> MenuItem {
    let dialog = dialog.clone();
    MenuItem::action("&About", move |cx| {
        dialog.borrow_mut().show_info(
            "About RetroSaurus",
            "RetroSaurus\n\n\
             A Windows 3.1-flavored thesaurus & dictionary\n\
             built on the Saudade toolkit.\n\n\
             Word data: Open English WordNet 2025\n\
             \u{00A9} the OEWN community \u{2014} CC BY 4.0\n\
             https://en-word.net",
        );
        cx.request_paint();
    })
}
