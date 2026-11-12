//! wash-fps `client/credits/MarkdownLite.ts`: the bit of Markdown our own
//! documents use (CREDITS.md, TERMS.md), as blocks egui draws.
//!
//! - `#`–`######` headings, paragraphs, `-`/`*`/`1.` lists, nested one level
//!   by indenting two or more spaces, and `---` rules.
//! - Inline `**bold**`, `*italic*` / `_italic_`, `` `code` ``, `[text](url)`.
//! - Links keep their address only for http(s) and mailto; anything else is
//!   plain text. HTML comments are skipped and other HTML shows as text.

use shagger::ui::components::lobby_twin::{clamp_css, hsl, hsla};
use shagger::ui::egui_layer::egui;

use egui::text::LayoutJob;
use egui::{Color32, FontFamily, FontId, Painter, Pos2, Rect, Sense, Stroke, TextFormat, pos2, vec2};

use crate::client::washoi_style::display_family;

/// One run of inline text and its marks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: Option<String>,
}

/// A list item: its text, and the items nested under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub text: Vec<Span>,
    pub ordered_sub: bool,
    pub sub: Vec<Vec<Span>>,
}

/// One block of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// Level 1 to 6, after the offset.
    Heading(u8, Vec<Span>),
    Paragraph(Vec<Span>),
    List {
        ordered: bool,
        items: Vec<Item>,
    },
    Rule,
}

/// `HEADING`: `#{1,6}`, space, text, trailing `#`s dropped.
fn heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &line[hashes..];
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim_end();
    Some((hashes, text))
}

/// `RULE`.
fn is_rule(line: &str) -> bool {
    line.len() >= 3
        && ["-", "*", "_"]
            .iter()
            .any(|c| line.chars().all(|x| x.to_string() == *c))
}

/// `BULLET` and `NUMBERED`: the item's text, and whether it is numbered.
fn list_item(line: &str) -> Option<(&str, bool)> {
    let mut chars = line.chars();
    if let Some(first) = chars.next()
        && matches!(first, '-' | '*' | '+')
    {
        let rest = &line[1..];
        if rest.starts_with(char::is_whitespace) {
            return Some((rest.trim_start(), false));
        }
        return None;
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let rest = &line[digits..];
    let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))?;
    rest.starts_with(char::is_whitespace).then(|| (rest.trim_start(), true))
}

fn strip_comments(source: &str) -> String {
    let mut out = String::new();
    let mut rest = source;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => {
                rest = &rest[start..];
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A list item being read: its lines, whether its sub list is numbered,
/// and the sub items' lines.
type OpenItem = (Vec<String>, bool, Vec<Vec<String>>);
/// A list being read: numbered, and its items.
type OpenList = (bool, Vec<OpenItem>);

/// `renderMarkdown`: the document's blocks; `heading_offset` 1 makes `#` a
/// level-2 heading.
#[must_use]
pub fn render_markdown(source: &str, heading_offset: u8) -> Vec<Block> {
    let source = strip_comments(&source.replace("\r\n", "\n").replace('\r', "\n"));
    let mut out = Vec::new();
    let mut paragraph: Vec<String> = Vec::new();
    // The open list: ordered, its items (text lines, sub lists).
    let mut list: Option<OpenList> = None;
    let finish = |list: OpenList| Block::List {
        ordered: list.0,
        items: list
            .1
            .into_iter()
            .map(|(text, ordered_sub, sub)| Item {
                text: inline(&text.join(" ")),
                ordered_sub,
                sub: sub.into_iter().map(|s| inline(&s.join(" "))).collect(),
            })
            .collect(),
    };
    let flush = |out: &mut Vec<Block>, paragraph: &mut Vec<String>, list: &mut Option<_>| {
        if !paragraph.is_empty() {
            out.push(Block::Paragraph(inline(&paragraph.join(" "))));
            paragraph.clear();
        }
        if let Some(l) = list.take() {
            out.push(finish(l));
        }
    };
    for raw in source.split('\n') {
        let line = raw.trim();
        if line.is_empty() {
            flush(&mut out, &mut paragraph, &mut list);
            continue;
        }
        if let Some((level, text)) = heading(line) {
            flush(&mut out, &mut paragraph, &mut list);
            out.push(Block::Heading((level as u8 + heading_offset).min(6), inline(text)));
            continue;
        }
        if is_rule(line) {
            flush(&mut out, &mut paragraph, &mut list);
            out.push(Block::Rule);
            continue;
        }
        if let Some((text, ordered)) = list_item(line) {
            let indented = raw.len() - raw.trim_start().len() >= 2;
            if indented
                && let Some((_, items)) = &mut list
                && let Some(parent) = items.last_mut()
            {
                // An indented item nests under the item above it.
                if !parent.2.is_empty() && parent.1 != ordered {
                    parent.2.clear();
                }
                parent.1 = ordered;
                parent.2.push(vec![text.to_owned()]);
                continue;
            }
            if !paragraph.is_empty() || list.as_ref().is_some_and(|(o, _)| *o != ordered) {
                flush(&mut out, &mut paragraph, &mut list);
            }
            list.get_or_insert_with(|| (ordered, Vec::new()))
                .1
                .push((vec![text.to_owned()], false, Vec::new()));
            continue;
        }
        // A wrapped line continues the list item or paragraph it follows.
        match list.as_mut().and_then(|(_, items)| items.last_mut()) {
            Some((text, _, sub)) => match sub.last_mut() {
                Some(s) => s.push(line.to_owned()),
                None => text.push(line.to_owned()),
            },
            None => paragraph.push(line.to_owned()),
        }
    }
    flush(&mut out, &mut paragraph, &mut list);
    out
}

fn is_word(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// One alternative of `inline`'s pattern tried at the start of `s`, with
/// the char before it: the whole match's length and the span(s) it makes.
fn match_at(s: &str, before: Option<char>) -> Option<(usize, Vec<Span>)> {
    let mark = |inner: &str, bold: bool| -> Vec<Span> {
        inline(inner)
            .into_iter()
            .map(|mut sp| {
                if bold {
                    sp.bold = true;
                } else {
                    sp.italic = true;
                }
                sp
            })
            .collect()
    };
    // `code`
    if let Some(rest) = s.strip_prefix('`')
        && let Some(end) = rest.find('`')
        && end > 0
    {
        let span = Span {
            text: rest[..end].to_owned(),
            code: true,
            ..Span::default()
        };
        return Some((end + 2, vec![span]));
    }
    // [label](href)
    if let Some(rest) = s.strip_prefix('[')
        && let Some(close) = rest.find(']')
        && close > 0
        && rest[close + 1..].starts_with('(')
    {
        let after = &rest[close + 2..];
        if let Some(end) = after.find(')') {
            let href = &after[..end];
            if !href.is_empty() && !href.contains(char::is_whitespace) {
                let label = &rest[..close];
                let safe = ["http:", "https:", "mailto:"]
                    .iter()
                    .any(|p| href.len() >= p.len() && href[..p.len()].eq_ignore_ascii_case(p));
                let spans = inline(label)
                    .into_iter()
                    .map(|mut sp| {
                        if safe {
                            sp.link = Some(href.to_owned());
                        }
                        sp
                    })
                    .collect();
                return Some((1 + close + 2 + end + 1, spans));
            }
        }
    }
    // **strong** and __strong__: the shortest inner text that closes.
    for delim in ["**", "__"] {
        if let Some(rest) = s.strip_prefix(delim)
            && let Some(end) = rest.get(1..).and_then(|r| r.find(delim)).map(|e| e + 1)
        {
            return Some((end + 4, mark(&rest[..end], true)));
        }
    }
    // *em*
    if let Some(rest) = s.strip_prefix('*')
        && let Some(end) = rest.get(1..).and_then(|r| r.find('*')).map(|e| e + 1)
    {
        return Some((end + 2, mark(&rest[..end], false)));
    }
    // \b_em_\b
    if let Some(rest) = s.strip_prefix('_')
        && !is_word(before)
    {
        let mut from = 1;
        while let Some(i) = rest.get(from..).and_then(|r| r.find('_')).map(|e| e + from) {
            if !is_word(rest[i + 1..].chars().next()) {
                return Some((i + 2, mark(&rest[..i], false)));
            }
            from = i + 1;
        }
    }
    None
}

/// `inline`: marks, links and code in one line of text.
#[must_use]
pub fn inline(text: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    let mut before = None;
    while i < text.len() {
        if let Some((len, spans)) = match_at(&text[i..], before) {
            if !plain.is_empty() {
                out.push(Span {
                    text: std::mem::take(&mut plain),
                    ..Span::default()
                });
            }
            out.extend(spans);
            before = text[..i + len].chars().next_back();
            i += len;
            continue;
        }
        let c = text[i..].chars().next().expect("in bounds");
        plain.push(c);
        before = Some(c);
        i += c.len_utf8();
    }
    if !plain.is_empty() {
        out.push(Span {
            text: plain,
            ..Span::default()
        });
    }
    out
}

/// How a document reads on the gold card: `.lobby-credits__body` or
/// `.lobby-guide__terms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    Credits,
    Terms,
}

/// `--ink` at `a`.
fn ink(a: f32) -> Color32 {
    hsla(250.0, 45.0, 7.0, a)
}

/// `--violet-ink`.
fn violet_ink() -> Color32 {
    hsl(276.0, 70.0, 40.0)
}

/// One heading's look: size, face, uppercase and spacing, margins, and
/// whether a rule runs under it.
struct HeadingLook {
    size: f32,
    display: bool,
    upper: bool,
    em: f32,
    above: f32,
    below: f32,
    ruled: bool,
}

impl Reading {
    /// Body text: size and line height.
    fn text(self) -> (f32, f32) {
        match self {
            Self::Credits => (17.0, 17.0 * 1.6),
            Self::Terms => (18.0, 18.0 * 1.55),
        }
    }

    fn heading(self, level: u8, screen_w: f32) -> HeadingLook {
        match (self, level) {
            // `h2`: a small ruled caption.
            (Self::Credits, ..=2) => HeadingLook {
                size: 13.0,
                display: true,
                upper: true,
                em: 0.14,
                above: 26.0,
                below: 8.0,
                ruled: true,
            },
            (Self::Credits, _) => HeadingLook {
                size: 16.0,
                display: true,
                upper: false,
                em: 0.0,
                above: 18.0,
                below: 6.0,
                ruled: false,
            },
            // `h3`, the terms' own title.
            (Self::Terms, ..=3) => HeadingLook {
                size: clamp_css(21.0, screen_w * 0.019, 26.0),
                display: true,
                upper: false,
                em: 0.0,
                above: 16.0,
                below: 4.0,
                ruled: false,
            },
            // `h4`: each section, ruled.
            (Self::Terms, _) => HeadingLook {
                size: 15.0,
                display: true,
                upper: true,
                em: 0.12,
                above: 22.0,
                below: 6.0,
                ruled: true,
            },
        }
    }

    /// `p` margins (`li` takes the list's).
    fn paragraph_margin(self) -> f32 {
        match self {
            Self::Credits => 8.0,
            Self::Terms => 6.0,
        }
    }
}

/// The spans as one job: ink text, bold full ink, `em` faded, links inked
/// and underlined in violet, code in the mono face on a faint band.
fn spans_job(spans: &[Span], size: f32, line_h: f32, width: f32) -> (LayoutJob, Vec<Option<String>>) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width.max(1.0);
    let mut links = Vec::new();
    for span in spans {
        let color = if span.italic {
            ink(0.72)
        } else if span.bold || span.link.is_some() {
            ink(1.0)
        } else {
            ink(0.9)
        };
        let mut format = TextFormat {
            font_id: if span.code {
                FontId::monospace(size * 0.9)
            } else {
                FontId::proportional(size)
            },
            color,
            line_height: Some(line_h),
            italics: span.italic,
            ..TextFormat::default()
        };
        if span.code {
            format.background = ink(0.12);
        }
        if span.link.is_some() {
            format.underline = Stroke::new(2.0, violet_ink());
        }
        job.append(&span.text, 0.0, format);
        links.push(span.link.clone());
    }
    (job, links)
}

/// The link under `pos` in a galley laid out from `links`' spans.
fn link_at(galley: &egui::Galley, origin: Pos2, pos: Pos2, links: &[Option<String>]) -> Option<String> {
    let local = pos - origin;
    if !galley.rect.expand(2.0).contains(local.to_pos2()) {
        return None;
    }
    // The character under the pointer, and the span whose bytes hold it.
    let index = galley.cursor_from_pos(local).index;
    let text = &galley.job.text;
    let byte = text.char_indices().nth(index.0).map_or(text.len(), |(b, _)| b);
    let section = galley
        .job
        .sections
        .iter()
        .position(|s| (s.byte_range.start.0..s.byte_range.end.0).contains(&byte))?;
    links.get(section).cloned().flatten()
}

/// A wrapped run of spans at `left`, `top`, `width` wide; the link clicked,
/// if any, and the bottom.
fn paint_spans(
    ui: &mut egui::Ui,
    spans: &[Span],
    left: f32,
    top: f32,
    width: f32,
    size: f32,
    line_h: f32,
) -> (Option<String>, f32) {
    let (job, links) = spans_job(spans, size, line_h, width);
    let painter = ui.painter().clone();
    let galley = painter.layout_job(job);
    let origin = pos2(left, top);
    let rect = Rect::from_min_size(origin, galley.size());
    let mut clicked = None;
    if links.iter().any(Option::is_some) {
        let response = ui.interact(
            rect,
            ui.scope_id().with(("md", top.to_bits(), left.to_bits())),
            Sense::click(),
        );
        if let Some(pos) = response.hover_pos()
            && let Some(url) = link_at(&galley, origin, pos, &links)
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if response.clicked() {
                clicked = Some(url);
            }
        }
    }
    painter.galley(origin, galley, ink(1.0));
    (clicked, rect.bottom())
}

/// Draws the blocks from `top` down, `width` wide from `left`, the way
/// `reading` styles them. Returns the link clicked and the bottom.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    ui: &mut egui::Ui,
    blocks: &[Block],
    left: f32,
    top: f32,
    width: f32,
    reading: Reading,
    screen_w: f32,
) -> (Option<String>, f32) {
    let painter = ui.painter().clone();
    let (size, line_h) = reading.text();
    let mut y = top;
    // Margins collapse: the larger of the last block's foot and the next one's head.
    let mut margin = 0.0_f32;
    let mut first = true;
    let mut clicked = None;
    let mut take = |c: Option<String>| {
        if c.is_some() {
            clicked = c;
        }
    };
    for block in blocks {
        match block {
            Block::Heading(level, spans) => {
                let look = reading.heading(*level, screen_w);
                y += if first { 0.0 } else { margin.max(look.above) };
                let text: String = spans.iter().map(|s| s.text.as_str()).collect();
                let text = if look.upper { text.to_uppercase() } else { text };
                let family = if look.display {
                    display_family()
                } else {
                    FontFamily::Proportional
                };
                let mut job = LayoutJob::default();
                job.wrap.max_width = width;
                job.append(
                    &text,
                    0.0,
                    TextFormat {
                        font_id: FontId::new(look.size, family),
                        color: ink(1.0),
                        extra_letter_spacing: look.size * look.em,
                        line_height: Some(look.size * 1.3),
                        ..TextFormat::default()
                    },
                );
                let g = painter.layout_job(job);
                let h = g.size().y;
                painter.galley(pos2(left, y), g, ink(1.0));
                y += h;
                if look.ruled {
                    y += 6.0;
                    painter.rect_filled(Rect::from_min_size(pos2(left, y), vec2(width, 2.0)), 0.0, ink(0.2));
                    y += 2.0;
                }
                margin = look.below;
            }
            Block::Paragraph(spans) => {
                let m = reading.paragraph_margin();
                y += if first { 0.0 } else { margin.max(m) };
                let w = if reading == Reading::Credits {
                    width.min(760.0)
                } else {
                    width
                };
                let (c, bottom) = paint_spans(ui, spans, left, y, w, size, line_h);
                take(c);
                y = bottom;
                margin = m;
            }
            Block::List { ordered, items } => {
                let (above, below, gap) = match reading {
                    Reading::Credits => (6.0, 10.0, 4.0),
                    Reading::Terms => (6.0, 6.0, 6.0),
                };
                y += if first { 0.0 } else { margin.max(above) };
                let indent = 22.0;
                for (n, item) in items.iter().enumerate() {
                    if n > 0 {
                        y += gap;
                    }
                    marker(&painter, *ordered, n, left + indent, y, size, line_h);
                    let (c, bottom) = paint_spans(ui, &item.text, left + indent, y, width - indent, size, line_h);
                    take(c);
                    y = bottom;
                    for (m, sub) in item.sub.iter().enumerate() {
                        y += gap;
                        let x = left + indent * 2.0;
                        marker(&painter, item.ordered_sub, m, x, y, size, line_h);
                        let (c, bottom) = paint_spans(ui, sub, x, y, width - indent * 2.0, size, line_h);
                        take(c);
                        y = bottom;
                    }
                }
                margin = below;
            }
            Block::Rule => {
                y += if first { 0.0 } else { margin.max(22.0) };
                painter.rect_filled(Rect::from_min_size(pos2(left, y), vec2(width, 2.0)), 0.0, ink(0.2));
                y += 2.0;
                margin = 22.0;
            }
        }
        first = false;
    }
    (clicked, y)
}

/// `li::marker`, in violet ink, ending just left of the item's text.
fn marker(painter: &Painter, ordered: bool, n: usize, text_left: f32, top: f32, size: f32, line_h: f32) {
    let mid = top + line_h / 2.0;
    if ordered {
        let g = painter.layout_no_wrap(format!("{}.", n + 1), FontId::proportional(size), violet_ink());
        painter.galley(
            pos2(text_left - 6.0 - g.size().x, mid - g.size().y / 2.0),
            g,
            violet_ink(),
        );
    } else {
        painter.circle_filled(pos2(text_left - 12.0, mid), size * 0.16, violet_ink());
    }
}

/// The document's "Last Updated" date (`TERMS_VERSION`).
#[must_use]
pub fn last_updated(source: &str) -> String {
    source
        .lines()
        .find_map(|l| {
            l.split_once("**Last Updated:**")
                .map(|(_, rest)| rest.trim().to_owned())
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Span {
        Span {
            text: text.to_owned(),
            ..Span::default()
        }
    }

    #[test]
    fn inline_marks_follow_the_pattern() {
        let spans = inline("a **b** *c* `d` [e](https://x.y) [f](javascript:z) snake_case _g_");
        let texts: Vec<&str> = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            texts,
            ["a ", "b", " ", "c", " ", "d", " ", "e", " ", "f", " snake_case ", "g"]
        );
        assert!(spans[1].bold && spans[3].italic && spans[5].code);
        assert_eq!(spans[7].link.as_deref(), Some("https://x.y"));
        assert_eq!(spans[9].link, None);
        assert!(spans[11].italic);
        assert_eq!(inline("x"), [plain("x")]);
    }

    #[test]
    fn blocks_cover_headings_lists_and_rules() {
        let doc = "# Title\n\nOne\nline.\n\n- a\n  wrapped\n  - nested\n- b\n\n1. first\n\n---\n<!-- gone -->Text";
        let blocks = render_markdown(doc, 1);
        assert_eq!(blocks[0], Block::Heading(2, vec![plain("Title")]));
        assert_eq!(blocks[1], Block::Paragraph(vec![plain("One line.")]));
        let Block::List { ordered: false, items } = &blocks[2] else {
            panic!("{:?}", blocks[2]);
        };
        assert_eq!(items[0].text, [plain("a wrapped")]);
        assert_eq!(items[0].sub, [vec![plain("nested")]]);
        assert_eq!(items[1].text, [plain("b")]);
        assert!(matches!(&blocks[3], Block::List { ordered: true, .. }));
        assert_eq!(blocks[4], Block::Rule);
        assert_eq!(blocks[5], Block::Paragraph(vec![plain("Text")]));
    }

    #[test]
    fn the_terms_parse() {
        let terms = include_str!("../../../assets/docs/TERMS.md");
        let blocks = render_markdown(terms, 2);
        assert!(blocks.len() > 10);
        assert_eq!(last_updated(terms), "October 22, 2026");
    }
}
