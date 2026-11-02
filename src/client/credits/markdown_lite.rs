//! wash-fps `client/credits/MarkdownLite.ts`: the bit of Markdown our own
//! documents use (CREDITS.md, TERMS.md), as blocks egui draws.
//!
//! - `#`–`######` headings, paragraphs, `-`/`*`/`1.` lists, nested one level
//!   by indenting two or more spaces, and `---` rules.
//! - Inline `**bold**`, `*italic*` / `_italic_`, `` `code` ``, `[text](url)`.
//! - Links keep their address only for http(s) and mailto; anything else is
//!   plain text. HTML comments are skipped and other HTML shows as text.

use shagger::ui::egui_layer::egui;

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

/// The spans as one wrapped egui label.
fn spans_label(ui: &mut egui::Ui, spans: &[Span], size: f32, strong: egui::Color32) {
    let mut job = egui::text::LayoutJob::default();
    let base = ui.visuals().text_color();
    for span in spans {
        let mut format = egui::TextFormat {
            font_id: if span.code {
                egui::FontId::monospace(size)
            } else {
                egui::FontId::proportional(size)
            },
            color: if span.bold || span.link.is_some() { strong } else { base },
            italics: span.italic,
            ..egui::TextFormat::default()
        };
        if span.link.is_some() {
            format.underline = egui::Stroke::new(1.0, strong);
        }
        job.append(&span.text, 0.0, format);
    }
    job.wrap.max_width = ui.available_width();
    ui.label(job);
}

/// Draw the blocks; `strong` colours bold text and links.
pub fn show(ui: &mut egui::Ui, blocks: &[Block], strong: egui::Color32) {
    for block in blocks {
        match block {
            Block::Heading(level, spans) => {
                ui.add_space(6.0);
                let size = match level {
                    1 => 28.0,
                    2 => 24.0,
                    3 => 21.0,
                    _ => 19.0,
                };
                let spans: Vec<Span> = spans.iter().cloned().map(|s| Span { bold: true, ..s }).collect();
                spans_label(ui, &spans, size, strong);
            }
            Block::Paragraph(spans) => spans_label(ui, spans, 17.0, strong),
            Block::List { ordered, items } => {
                for (n, item) in items.iter().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(if *ordered {
                            format!("{}.", n + 1)
                        } else {
                            "•".to_owned()
                        });
                        ui.vertical(|ui| {
                            spans_label(ui, &item.text, 17.0, strong);
                            for (m, sub) in item.sub.iter().enumerate() {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(if item.ordered_sub {
                                        format!("{}.", m + 1)
                                    } else {
                                        "-".to_owned()
                                    });
                                    spans_label(ui, sub, 17.0, strong);
                                });
                            }
                        });
                    });
                }
            }
            Block::Rule => {
                ui.separator();
            }
        }
    }
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
        let blocks = render_markdown(include_str!("../../../assets/docs/TERMS.md"), 2);
        assert!(blocks.len() > 10);
    }
}
