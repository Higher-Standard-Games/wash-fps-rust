//! wash-fps `client/credits/CreditsDialog.ts` and `styles/CreditsDialog.css`:
//! CREDITS.md from the game's repository, on the welcome guide's card. The
//! file is built in, so the dialog and GitHub show the same text.

use shagger::ui::egui_layer::egui;

use egui::{Rect, pos2};

use super::markdown_lite::{self, Block, Reading, render_markdown};
use crate::client::lobby_welcome_dialog::{
    card_rect, footer, header, ink_key, ink_key_width, paint_card, scroll_body, veil,
};

/// The credits, as the web build bundles them.
pub const CREDITS: &str = include_str!("../../../assets/docs/CREDITS.md");

/// What the dialog asks of the lobby.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreditsAnswer {
    Close,
    /// A link, for the system's browser.
    Open(String),
}

/// `CreditsDialog`.
#[derive(Debug)]
pub struct CreditsDialog {
    blocks: Vec<Block>,
    opened: bool,
}

impl Default for CreditsDialog {
    fn default() -> Self {
        Self::new(CREDITS)
    }
}

impl CreditsDialog {
    #[must_use]
    pub fn new(source: &str) -> Self {
        let mut blocks = render_markdown(source, 0);
        // The file's own "# Credits" title is the dialog's header here.
        if matches!(blocks.first(), Some(Block::Heading(1, _))) {
            blocks.remove(0);
        }
        Self { blocks, opened: true }
    }

    /// Draws the dialog over `screen`; Close, a click off the card, or a link.
    pub fn ui(&mut self, ui: &mut egui::Ui, screen: Rect) -> Option<CreditsAnswer> {
        let w = screen.width();
        // `width: min(860px, 100%)`, as tall as the window lets a long read be.
        let card = card_rect(screen, 860.0, f32::INFINITY);
        let mut answer = veil(ui, screen, "credits", card).then_some(CreditsAnswer::Close);
        let painter = ui.painter().clone();
        let inner = paint_card(&painter, card, w);
        let top = header(&painter, inner, w, "WASHOI / CREDITS", "Credits & copyrights", 18.0);
        let keys_top = footer(&painter, inner);
        let body = Rect::from_min_max(pos2(inner.left(), top), pos2(inner.right(), keys_top - 22.0 - 10.0));
        let reset = std::mem::take(&mut self.opened);
        let blocks = &self.blocks;
        let mut link = None;
        scroll_body(ui, body, "credits", reset, |ui, at, width| {
            // `padding: 6px 5px 10px`.
            let (clicked, bottom) =
                markdown_lite::paint(ui, blocks, at.x + 5.0, at.y + 6.0, width - 10.0, Reading::Credits, w);
            link = clicked;
            bottom + 10.0
        });
        if let Some(url) = link {
            answer = Some(CreditsAnswer::Open(url));
        }
        let kw = ink_key_width(&painter, "Close", 125.0);
        if ink_key(
            ui,
            "credits-close",
            inner.right() - kw - (6.0 * 0.75 + 2.0),
            keys_top,
            kw,
            "Close",
            false,
        ) {
            answer = Some(CreditsAnswer::Close);
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_s_title_gives_way_to_the_header() {
        let d = CreditsDialog::default();
        assert!(matches!(d.blocks.first(), Some(Block::Paragraph(_))));
        assert!(d.blocks.iter().any(|b| matches!(b, Block::Heading(2, _))));
    }
}
