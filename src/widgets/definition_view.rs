//! The scrollable, clickable definition pane — RetroSaurus's centerpiece.
//!
//! saudade has no rich-text widget and no bold/italic faces (text is drawn only
//! by size + color; see `painter.rs`), so this widget renders a small styled
//! "document" itself: a list of [`Line`]s of [`Span`]s, word-wrapped to the
//! pane width and scrolled by visual line — the scrolling chrome mirrors
//! `journey`'s `DiffView`. Spans tagged [`RunStyle::Link`] are cross-references:
//! a click hit-tests them and exposes the target head word via
//! [`DefinitionView::take_navigation`], which the app turns into navigation.
//!
//! Word-wrapping needs [`Painter::measure_text`], which is only available inside
//! `paint`, so the layout is (re)built there whenever the document or the pane
//! width changes; pointer hit-testing in `event` runs against the fragments laid
//! out by the previous paint — the same approach saudade's `TextInput` uses.

use saudade::{
    Color, Event, EventCtx, Key, MouseButton, NamedKey, Painter, Point, Rect, SCROLLBAR_THICKNESS,
    ScrollBar, Theme, Widget,
};

use crate::thesaurus::{Entry, Pos, WordId};

const PAD_X: i32 = 6;
const PAD_Y: i32 = 4;
const INDENT_SENSE: i32 = 16;
const INDENT_DETAIL: i32 = 30;

// Link colors — a brighter blue than the navy chrome, underlined so they read
// as clickable without a hand cursor (saudade exposes no cursor API).
const LINK: Color = Color::rgb(0x00, 0x00, 0xCC);
const LINK_HOVER: Color = Color::rgb(0x33, 0x66, 0xFF);

/// How a span is drawn. saudade can vary only size and color, so emphasis is
/// faked: a larger headword, colored part-of-speech / labels, a muted example.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunStyle {
    Headword,
    Pos,
    SenseNum,
    Definition,
    Example,
    Label,
    Link,
}

/// One run of text within a line. A `link` makes it a clickable cross-reference.
#[derive(Clone, Debug)]
pub struct Span {
    pub text: String,
    pub style: RunStyle,
    pub link: Option<WordId>,
}

impl Span {
    pub fn text(text: impl Into<String>, style: RunStyle) -> Span {
        Span {
            text: text.into(),
            style,
            link: None,
        }
    }

    pub fn link(text: impl Into<String>, word: WordId) -> Span {
        Span {
            text: text.into(),
            style: RunStyle::Link,
            link: Some(word),
        }
    }
}

/// A logical line (paragraph) of the document: an indent plus a sequence of
/// spans that flow and wrap together. An empty `spans` renders as a blank row
/// for vertical spacing.
#[derive(Clone, Debug, Default)]
pub struct Line {
    pub indent: i32,
    pub spans: Vec<Span>,
}

impl Line {
    fn blank() -> Line {
        Line::default()
    }
}

/// A span positioned on a concrete visual line by the wrap pass — what `paint`
/// draws and `event` hit-tests.
struct Frag {
    line: usize,
    x: i32,
    w: i32,
    text: String,
    style: RunStyle,
    link: Option<WordId>,
}

/// A read-only, scrollable, clickable definition pane.
pub struct DefinitionView {
    rect: Rect,
    doc: Vec<Line>,
    /// Wrapped fragments + total visual line count, rebuilt when the document or
    /// pane width changes.
    frags: Vec<Frag>,
    line_count: i32,
    laid_width: i32,
    dirty: bool,
    font_size: f32,
    v_scrollbar: ScrollBar,
    focused: bool,
    hovered: Option<usize>,
    pending_nav: Option<WordId>,
    /// Placeholder shown when there's no document (empty query / no selection).
    placeholder: String,
}

impl DefinitionView {
    pub fn new(rect: Rect) -> Self {
        let mut me = Self {
            rect,
            doc: Vec::new(),
            frags: Vec::new(),
            line_count: 0,
            laid_width: -1,
            dirty: true,
            font_size: 14.0,
            v_scrollbar: ScrollBar::vertical(Rect::new(0, 0, 0, 0)),
            focused: false,
            hovered: None,
            pending_nav: None,
            placeholder: "Type a word in the search box above.".to_string(),
        };
        me.relayout_scrollbar();
        me
    }

    /// Replace the document and scroll back to the top.
    pub fn set_document(&mut self, doc: Vec<Line>) {
        self.doc = doc;
        self.dirty = true;
        self.hovered = None;
        self.v_scrollbar.set_value(0);
    }

    /// Clear the pane back to the placeholder.
    pub fn clear(&mut self) {
        self.set_document(Vec::new());
    }

    /// Consume and return a pending cross-reference navigation (a clicked link).
    pub fn take_navigation(&mut self) -> Option<WordId> {
        self.pending_nav.take()
    }

    fn line_height(&self) -> i32 {
        (self.font_size as i32 + 8).max(10)
    }

    fn text_area(&self) -> Rect {
        let sb_w = if self.v_scrollbar.rect().w > 0 {
            SCROLLBAR_THICKNESS
        } else {
            0
        };
        Rect::new(
            self.rect.x,
            self.rect.y,
            (self.rect.w - sb_w).max(0),
            self.rect.h,
        )
    }

    fn content_w(&self) -> i32 {
        (self.text_area().w - PAD_X * 2).max(0)
    }

    fn visible_rows(&self) -> i32 {
        ((self.text_area().h - PAD_Y * 2) / self.line_height()).max(1)
    }

    fn scroll_top(&self) -> usize {
        self.v_scrollbar.value().max(0) as usize
    }

    fn sync_scrollbar(&mut self) {
        let visible = self.visible_rows();
        let max_scroll = (self.line_count - visible).max(0);
        self.v_scrollbar.set_range(visible, max_scroll);
        self.v_scrollbar.set_line_step(1);
    }

    fn relayout_scrollbar(&mut self) {
        let sb_rect = Rect::new(
            self.rect.right() - SCROLLBAR_THICKNESS,
            self.rect.y,
            SCROLLBAR_THICKNESS,
            self.rect.h,
        );
        self.v_scrollbar.set_rect(sb_rect);
        self.sync_scrollbar();
    }

    fn scroll_by(&mut self, delta: i32) {
        let v = self.v_scrollbar.value();
        self.v_scrollbar.set_value(v + delta);
    }

    fn style_size(&self, style: RunStyle) -> f32 {
        match style {
            RunStyle::Headword => self.font_size + 4.0,
            _ => self.font_size,
        }
    }

    /// Word-wrap the document into `self.frags`. Requires a painter for text
    /// measurement, so it runs inside `paint`.
    fn relayout(&mut self, painter: &Painter) {
        self.frags.clear();
        let content_w = self.content_w();
        let space_w = painter.measure_text(" ", self.font_size).w.max(1);
        let mut vline: usize = 0;

        for line in &self.doc {
            let x_start = line.indent;
            let mut x = x_start;
            let mut first = true;
            for span in &line.spans {
                let size = self.style_size(span.style);
                let w = painter.measure_text(&span.text, size).w;
                let space = if first { 0 } else { space_w };
                // Wrap before this span if it would overflow — unless it's the
                // first span on the line (then it's placed and may clip).
                if !first && x + space + w > content_w {
                    vline += 1;
                    x = x_start;
                    first = true;
                }
                if !first {
                    x += space;
                }
                self.frags.push(Frag {
                    line: vline,
                    x,
                    w,
                    text: span.text.clone(),
                    style: span.style,
                    link: span.link,
                });
                x += w;
                first = false;
            }
            // Every logical line consumes at least one visual line.
            vline += 1;
        }

        self.line_count = vline as i32;
        self.laid_width = content_w;
        self.dirty = false;
        self.sync_scrollbar();
    }

    /// The absolute on-screen rect of the first link to `word` in the current
    /// (last-painted) layout. Exposed for tests that click a cross-reference
    /// without hard-coding pixel coordinates.
    #[doc(hidden)]
    pub fn link_rect_for(&self, word: WordId) -> Option<Rect> {
        let text = self.text_area();
        let line_h = self.line_height();
        let x0 = text.x + PAD_X;
        let y0 = text.y + PAD_Y;
        let scroll_top = self.scroll_top() as i32;
        self.frags.iter().find(|f| f.link == Some(word)).map(|f| {
            let row = f.line as i32 - scroll_top;
            Rect::new(x0 + f.x, y0 + row * line_h, f.w, line_h)
        })
    }

    /// The frag index of a clickable link under `pos`, using the last paint's
    /// layout.
    fn link_at(&self, pos: Point) -> Option<usize> {
        let text = self.text_area();
        if !text.inset(1).contains(pos) {
            return None;
        }
        let line_h = self.line_height();
        let rel_y = pos.y - (text.y + PAD_Y);
        if rel_y < 0 {
            return None;
        }
        let line = self.scroll_top() + (rel_y / line_h) as usize;
        let x0 = text.x + PAD_X;
        self.frags.iter().position(|f| {
            f.link.is_some() && f.line == line && pos.x >= x0 + f.x && pos.x < x0 + f.x + f.w
        })
    }
}

impl Widget for DefinitionView {
    fn bounds(&self) -> Rect {
        self.rect
    }

    fn paint(&mut self, painter: &mut Painter, theme: &Theme) {
        if self.dirty || self.laid_width != self.content_w() {
            self.relayout(painter);
        }
        self.sync_scrollbar();

        let text = self.text_area();
        painter.fill_rect(text, Color::WHITE);
        painter.sunken_bevel(text, theme.highlight, theme.shadow);
        painter.stroke_rect(text, theme.border);

        let line_h = self.line_height();
        let x0 = text.x + PAD_X;
        let y0 = text.y + PAD_Y;
        let visible = self.visible_rows() as usize;
        let scroll_top = self.scroll_top();

        let saved = painter.push_clip(text.inset(1));

        if self.doc.is_empty() {
            painter.text(x0, y0, &self.placeholder, self.font_size, Color::MID_GRAY);
        } else {
            for (idx, frag) in self.frags.iter().enumerate() {
                if frag.line < scroll_top || frag.line >= scroll_top + visible {
                    continue;
                }
                let row = (frag.line - scroll_top) as i32;
                let size = self.style_size(frag.style);
                let y = y0 + row * line_h;
                let hovered = self.hovered == Some(idx);
                let color = color_for(frag.style, hovered);
                let baseline = y + (line_h - size as i32) / 2;
                painter.text(x0 + frag.x, baseline, &frag.text, size, color);
                if frag.style == RunStyle::Link {
                    painter.h_line(x0 + frag.x, baseline + size as i32, frag.w, color);
                }
            }
        }

        painter.restore_clip(saved);
        self.v_scrollbar.paint(painter, theme);
    }

    fn event(&mut self, event: &Event, ctx: &mut EventCtx) {
        // Route to the scrollbar while it's dragging or being clicked.
        if self.v_scrollbar.captures_pointer() {
            self.v_scrollbar.event(event, ctx);
            return;
        }
        if let Some(pos) = event.position()
            && self.v_scrollbar.rect().contains(pos)
        {
            self.v_scrollbar.event(event, ctx);
            return;
        }
        // Wheel anywhere over the pane scrolls it.
        if let Event::Scroll { .. } = event {
            self.v_scrollbar.event(event, ctx);
            return;
        }

        match event {
            Event::PointerDown {
                pos,
                button: MouseButton::Left,
            } => {
                ctx.request_focus();
                if let Some(idx) = self.link_at(*pos) {
                    self.pending_nav = self.frags[idx].link;
                }
                ctx.request_paint();
            }
            Event::PointerMove { pos } => {
                let hovered = self.link_at(*pos);
                if hovered != self.hovered {
                    self.hovered = hovered;
                    ctx.request_paint();
                }
            }
            Event::PointerLeave => {
                if self.hovered.is_some() {
                    self.hovered = None;
                    ctx.request_paint();
                }
            }
            Event::KeyDown { key, modifiers } if self.focused && !modifiers.has_command() => {
                let page = (self.visible_rows() - 1).max(1);
                let consumed = match key {
                    Key::Named(NamedKey::Up) => {
                        self.scroll_by(-1);
                        true
                    }
                    Key::Named(NamedKey::Down) => {
                        self.scroll_by(1);
                        true
                    }
                    Key::Named(NamedKey::PageUp) => {
                        self.scroll_by(-page);
                        true
                    }
                    Key::Named(NamedKey::PageDown) => {
                        self.scroll_by(page);
                        true
                    }
                    Key::Named(NamedKey::Home) => {
                        self.v_scrollbar.set_value(0);
                        true
                    }
                    Key::Named(NamedKey::End) => {
                        self.v_scrollbar.set_value(self.line_count);
                        true
                    }
                    _ => false,
                };
                if consumed {
                    ctx.request_paint();
                }
            }
            _ => {}
        }
    }

    fn captures_pointer(&self) -> bool {
        self.v_scrollbar.captures_pointer()
    }

    fn focusable(&self) -> bool {
        true
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn layout(&mut self, bounds: Rect) {
        self.rect = bounds;
        self.relayout_scrollbar();
        // Width may have changed; force a re-wrap on the next paint.
        self.dirty = true;
    }
}

/// The draw color for a span style, brightened while a link is hovered.
fn color_for(style: RunStyle, hovered: bool) -> Color {
    match style {
        RunStyle::Headword => Color::BLACK,
        RunStyle::Pos => Color::NAVY,
        RunStyle::SenseNum => Color::DARK_GRAY,
        RunStyle::Definition => Color::BLACK,
        RunStyle::Example => Color::DARK_GRAY,
        RunStyle::Label => Color::MID_GRAY,
        RunStyle::Link => {
            if hovered {
                LINK_HOVER
            } else {
                LINK
            }
        }
    }
}

/// Build the styled document for a head word: headword, then each sense grouped
/// by part of speech — definition, examples, synonyms, antonyms, related words.
pub fn build_document(entry: &Entry) -> Vec<Line> {
    let mut lines = Vec::new();
    lines.push(Line {
        indent: 0,
        spans: vec![Span::text(&entry.lemma, RunStyle::Headword)],
    });

    let mut last_pos: Option<Pos> = None;
    let mut sense_num = 0;
    for sense in &entry.senses {
        if Some(sense.pos) != last_pos {
            lines.push(Line::blank());
            lines.push(Line {
                indent: 0,
                spans: vec![Span::text(
                    format!("\u{25B8} {}", sense.pos.label()),
                    RunStyle::Pos,
                )],
            });
            last_pos = Some(sense.pos);
            sense_num = 0;
        }
        sense_num += 1;

        // Definition: "N. <words…>".
        let mut spans = vec![Span::text(format!("{sense_num}."), RunStyle::SenseNum)];
        for word in sense.definition.split_whitespace() {
            spans.push(Span::text(word, RunStyle::Definition));
        }
        lines.push(Line {
            indent: INDENT_SENSE,
            spans,
        });

        // Examples, quoted and muted.
        for example in &sense.examples {
            let quoted = format!("\u{201C}{example}\u{201D}");
            let spans = quoted
                .split_whitespace()
                .map(|w| Span::text(w, RunStyle::Example))
                .collect();
            lines.push(Line {
                indent: INDENT_DETAIL,
                spans,
            });
        }

        push_links(&mut lines, "synonyms:", &sense.synonyms);
        push_links(&mut lines, "antonyms:", &sense.antonyms);
        for group in &sense.related {
            push_links(&mut lines, &format!("{}:", group.label), &group.links);
        }
    }

    lines
}

/// Append a labeled line of clickable links (skipped when empty). Commas are
/// attached to each link so the run stays a single clickable unit.
fn push_links(lines: &mut Vec<Line>, label: &str, links: &[crate::thesaurus::Link]) {
    if links.is_empty() {
        return;
    }
    let mut spans = vec![Span::text(label, RunStyle::Label)];
    let last = links.len() - 1;
    for (i, link) in links.iter().enumerate() {
        let text = if i < last {
            format!("{},", link.lemma)
        } else {
            link.lemma.clone()
        };
        spans.push(Span::link(text, link.word));
    }
    lines.push(Line {
        indent: INDENT_DETAIL,
        spans,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thesaurus::{Fixture, Thesaurus};

    #[test]
    fn document_has_headword_pos_and_links() {
        let fx = Fixture::sample();
        let id = fx.lookup("abandon").unwrap();
        let entry = fx.entry(id).unwrap();
        let doc = build_document(&entry);

        // First line is the headword.
        assert_eq!(doc[0].spans[0].style, RunStyle::Headword);
        assert_eq!(doc[0].spans[0].text, "abandon");

        // A part-of-speech header appears.
        assert!(doc.iter().any(|l| {
            l.spans
                .iter()
                .any(|s| s.style == RunStyle::Pos && s.text.contains("verb"))
        }));

        // At least one clickable synonym link with a real target.
        let link = doc
            .iter()
            .flat_map(|l| &l.spans)
            .find(|s| s.style == RunStyle::Link)
            .expect("a link span");
        assert!(link.link.is_some());
    }
}
