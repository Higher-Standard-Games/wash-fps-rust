//! The lobby panel drawn: its layout from `LobbyPanel.css` and every page
//! (`LobbyPanel.ts`'s render functions). Split out of `lobby_panel.rs`; the
//! same module tree.

use super::*;

/// The CSS's sizes for one window size.
pub(super) struct Layout {
    pub(super) screen: Rect,
    pub(super) w: f32,
    pub(super) h: f32,
    pub(super) rail_w: f32,
    /// The rail's content: `22px` from its left, `skew + 18px` short of its right.
    pub(super) rail_left: f32,
    pub(super) rail_right: f32,
    /// `max-height: 820px` and `680px`.
    pub(super) short: bool,
    pub(super) shorter: bool,
    /// `max-width: 1500px`: the rail's hints give way.
    pub(super) narrow: bool,
    /// `max-width: 1100px`: two tiles a row, the chat under the mode.
    pub(super) narrower: bool,
}

impl Layout {
    pub(super) fn new(screen: Rect) -> Self {
        let (w, h) = (screen.width(), screen.height());
        let rail_w = TwinRail::width(w);
        Self {
            screen,
            w,
            h,
            rail_w,
            rail_left: screen.min.x + 22.0,
            rail_right: screen.min.x + rail_w - RAIL_SKEW - 18.0,
            short: w > 700.0 && h <= 820.0,
            shorter: w > 700.0 && h <= 680.0,
            narrow: w <= 1500.0,
            narrower: w <= 1100.0,
        }
    }
}

pub(super) fn body() -> FontFamily {
    FontFamily::Proportional
}

/// `text` with letter spacing in em.
pub(super) fn line(
    painter: &Painter,
    s: &str,
    size: f32,
    family: FontFamily,
    color: Color32,
    em: f32,
) -> Arc<egui::Galley> {
    text(painter, s, size, family, color, size * em)
}

/// Text wrapped at `width`.
pub(super) fn wrapped(painter: &Painter, s: &str, font: FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(s.to_owned(), font, color, width.max(1.0));
    job.wrap.break_anywhere = false;
    painter.layout_job(job)
}

/// The cut-cornered panel of the chat (`clip-path` off its top right).
pub(super) fn cut_panel(painter: &Painter, r: Rect, cut: f32, stops: &[(f32, Color32)], border: Option<Stroke>) {
    let shape = [
        r.left_top(),
        pos2(r.right() - cut, r.top()),
        pos2(r.right(), r.top() + cut),
        r.right_bottom(),
        r.left_bottom(),
    ];
    gradient_polygon(painter, &shape, stops);
    if let Some(stroke) = border {
        painter.add(Shape::closed_line(shape.to_vec(), stroke));
    }
}

impl LobbyPanel {
    /// Opens a page by name, as `settings` or `settings/video`: for
    /// screenshots and the in-match Settings key.
    pub fn open_page(&mut self, page: &str) {
        let mut parts = page.split('/');
        let (tab, sub, section) = (
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
            parts.next().unwrap_or_default(),
        );
        if tab == "welcome" {
            if sub == "restore" {
                self.welcome.open_restore(&account!(self));
            } else {
                self.welcome.open(&account!(self));
            }
            return;
        }
        if tab == "profile" {
            self.open_profile(ProfileScreen::from_id(sub));
            return;
        }
        if tab == "update" {
            self.shown_update = Some(crate::update_check::Update {
                version: "v9.9.9".into(),
                url: Some("https://washoi.net/".into()),
            });
            return;
        }
        if tab == "console" {
            self.console_toggle = true;
            return;
        }
        if tab == "credits" {
            self.credits = Some(CreditsDialog::default());
            return;
        }
        if tab == "guide" {
            // `guide/2`: the step, once the terms are agreed.
            self.guide.set_authenticated(true);
            self.guide.help(true);
            if let Some(d) = &mut self.guide.dialog {
                d.show_step(sub.parse::<usize>().unwrap_or(1).saturating_sub(1));
            }
            return;
        }
        // `campaign/<mission id>`: its comic card, as a mission start plays
        // it, for screenshots.
        if tab == "campaign" && sub == "complete" {
            self.campaign.preview_finale();
        }
        if tab == "campaign"
            && let Some(story) = campaign_story(sub)
        {
            let mut cutscene = CampaignCutscene::new(story.card, story.title);
            cutscene.set_status("Deploying…");
            self.cutscene = Some(cutscene);
        }
        self.tab = match tab {
            "leaderboard" => Tab::Leaderboard,
            "campaign" => Tab::Campaign,
            "demos" => Tab::Demos,
            "settings" => Tab::Settings,
            _ => Tab::Play,
        };
        if self.tab == Tab::Settings {
            let view = self.settings.get_or_insert_with(FpsSettingsView::default);
            if let Some(p) = SETTINGS_PAGES.iter().find(|p| p.title.eq_ignore_ascii_case(sub)) {
                view.show_page(p.id);
            }
            view.video.open_section(section);
        }
        if self.tab == Tab::Leaderboard {
            self.leaderboard.open_board(sub);
        }
        if self.tab == Tab::Demos {
            self.demos.open_board(sub, section == "reel");
        }
    }

    /// Widens the Profile out of the rail, at `screen`.
    pub fn open_profile(&mut self, screen: ProfileScreen) {
        self.profile.open(screen, &account!(self), &mut self.client);
    }

    /// `switchFpsAccount`: a new connection, logging in with the pasted key.
    /// The stored key stays until the lobby welcomes the new one.
    pub(super) fn start_switch(&mut self, pasted: &str) {
        let Some(key) = normalize_key(pasted) else {
            self.switch_answered(Err(login_failure_text("unknown_key")));
            return;
        };
        self.connection_lost(true);
        self.retry_at = None;
        self.full_notice = None;
        self.switch_key = Some(key);
        self.client = LobbyClient::connect(&self.url);
    }

    /// A switch's outcome, to whichever screen asked for it.
    pub(super) fn switch_answered(&mut self, answer: Result<String, String>) {
        if std::mem::take(&mut self.switch_from_welcome) {
            self.welcome.switch_answered(answer);
        } else {
            self.profile.switch_answered(answer, &account!(self));
        }
    }

    /// The settings screen, open on `page`.
    pub fn settings_view(&mut self) -> &mut FpsSettingsView {
        self.settings.get_or_insert_with(FpsSettingsView::default)
    }

    /// Escape: closes what the page has open. True when it did.
    pub fn back(&mut self) -> bool {
        if self.guide.back() {
            return true;
        }
        if self.credits.take().is_some() {
            return true;
        }
        if self.welcome.showing() {
            self.welcome.back(&account!(self));
            return true;
        }
        if self.profile.showing() {
            self.profile.back(&account!(self));
            return true;
        }
        match self.tab {
            Tab::Settings => self.settings.as_mut().is_some_and(FpsSettingsView::back),
            Tab::Demos => self.demos.back(),
            _ => false,
        }
    }

    /// The rail and everything on it.
    pub(super) fn rail(&mut self, ui: &mut egui::Ui, l: &Layout, style: &TwinStyle) {
        let painter = ui.painter().clone();
        let (top, bottom) = (l.screen.min.y, l.screen.max.y);
        paint_rail(&painter, l.screen.min.x, top, bottom, l.rail_w, RAIL_SKEW, 1.0);
        let pad_top = if l.shorter { 14.0 } else { 22.0 };
        let logo_max = if l.shorter {
            Some(l.h * 0.11)
        } else if l.short {
            Some(l.h * 0.13)
        } else {
            None
        };
        let mut y = paint_brand(
            &painter,
            l.screen.min.x,
            l.rail_w,
            top + pad_top,
            logo_max,
            l.w,
            style,
            1.0,
        );

        // `lobby__rail-nav`: Quick play, then the pages.
        let key_h = if l.shorter {
            46.0
        } else if l.short {
            54.0
        } else {
            66.0
        };
        let label_size = clamp_css(15.0, l.w * 0.011, 21.0);
        let nav_w = l.rail_right - l.rail_left;
        // In a room, Play online reads Lobby and leads back to it, cyan with
        // a radar sweeping on it while the room waits.
        let in_room = self.room.is_some();
        let play_label = if in_room { "Lobby" } else { "Play online" };
        // Campaign follows Quick play, in the campaign's own purple.
        let keys: [(&str, Option<Tab>); 5] = [
            ("Quick play", None),
            ("Campaign", Some(Tab::Campaign)),
            (play_label, Some(Tab::Play)),
            ("Leaderboard", Some(Tab::Leaderboard)),
            ("Demos", Some(Tab::Demos)),
        ];
        let time = ui.input(|i| i.time);
        for (label, tab) in keys {
            let rect = Rect::from_min_size(pos2(l.rail_left, y), vec2(nav_w, key_h));
            let gold = KeyLook::gold().flat();
            let current = tab.is_some_and(|t| self.tab == t);
            let campaign = tab == Some(Tab::Campaign);
            let lobby = tab == Some(Tab::Play) && in_room && !current;
            let key = match tab {
                // Quick play: pink, so a new player finds the way into a match.
                None => {
                    let pink = KeyLook {
                        face: vec![
                            (0.0, hsl(322.0, 100.0, 77.0)),
                            (0.55, hsl(322.0, 95.0, 62.0)),
                            (1.0, hsl(318.0, 85.0, 50.0)),
                        ],
                        side: [hsl(312.0, 70.0, 33.0), hsl(306.0, 70.0, 21.0)],
                        shadow: hsla(306.0, 70.0, 14.0, 0.42),
                        ..gold.clone()
                    };
                    let lit = KeyLook {
                        face: vec![
                            (0.0, hsl(322.0, 100.0, 84.0)),
                            (0.55, hsl(322.0, 100.0, 70.0)),
                            (1.0, hsl(318.0, 90.0, 57.0)),
                        ],
                        ..pink.clone()
                    };
                    RaisedKey::new(label, rect, pink)
                        .hover(lit, -0.35)
                        .disabled(!self.quick_play_enabled())
                }
                // The campaign's dark plate, purple, cream italic type on a
                // pink shadow; open, the deep plate, gold on deep purple.
                Some(_) if campaign => {
                    let plate = KeyLook {
                        face: vec![
                            (0.0, hsl(278.0, 56.0, 30.0)),
                            (0.48, hsl(270.0, 54.0, 22.0)),
                            (1.0, hsl(262.0, 52.0, 16.0)),
                        ],
                        side: [hsl(266.0, 50.0, 12.0), hsl(260.0, 50.0, 6.0)],
                        text: cream(),
                        shadow: hsla(262.0, 60.0, 8.0, 0.45),
                        ..gold.clone()
                    };
                    let lit = KeyLook {
                        face: vec![
                            (0.0, hsl(280.0, 60.0, 40.0)),
                            (0.48, hsl(272.0, 56.0, 30.0)),
                            (1.0, hsl(264.0, 54.0, 22.0)),
                        ],
                        ..plate.clone()
                    };
                    let deep = KeyLook {
                        face: vec![
                            (0.0, hsl(284.0, 72.0, 30.0)),
                            (0.45, hsl(274.0, 74.0, 20.0)),
                            (1.0, hsl(264.0, 78.0, 12.0)),
                        ],
                        text: hsl(46.0, 100.0, 57.0),
                        ..plate.clone()
                    };
                    if current {
                        RaisedKey::new(label, rect, deep.clone()).hover(deep, 0.0).current(true)
                    } else {
                        RaisedKey::new(label, rect, plate).hover(lit, -0.35)
                    }
                }
                // Back to the room: cyan, one click from the seat it kept.
                Some(_) if lobby => {
                    let cyan = KeyLook {
                        face: vec![
                            (0.0, hsl(182.0, 100.0, 74.0)),
                            (0.55, hsl(182.0, 95.0, 55.0)),
                            (1.0, hsl(188.0, 90.0, 42.0)),
                        ],
                        side: [hsl(192.0, 85.0, 28.0), hsl(198.0, 80.0, 17.0)],
                        text: hsl(250.0, 45.0, 8.0),
                        ..gold.clone()
                    };
                    let lit = KeyLook {
                        face: vec![
                            (0.0, hsl(182.0, 100.0, 82.0)),
                            (0.55, hsl(182.0, 100.0, 64.0)),
                            (1.0, hsl(188.0, 90.0, 50.0)),
                        ],
                        ..cyan.clone()
                    };
                    RaisedKey::new(label, rect, cyan).hover(lit, -0.35)
                }
                // The rail keeps its gold under the pointer; only the lift says so.
                Some(_) => RaisedKey::new(label, rect, gold.clone())
                    .hover(gold, -0.35)
                    .current(current),
            };
            let face = key.show(ui);
            if campaign {
                if !current {
                    rail_scanlines(&painter, face.rect, 0.03);
                }
                campaign_key_label(&painter, &face, label, label_size, 28.0);
            } else {
                KeyText::new(label, "", display_family(), body())
                    .size(label_size, 13.0)
                    .pad(28.0, 24.0)
                    .paint(&painter, &face);
            }
            if lobby {
                key_radar(
                    &painter,
                    pos2(face.rect.right() - 24.0 - 17.0, face.rect.center().y),
                    time,
                );
            }
            if face.clicked {
                match tab {
                    None => self.start_quick_play(),
                    Some(tab) => self.tab = tab,
                }
            }
            y += key_h + 6.0 + 8.0;
        }

        // The foot: the player card over the utility keys.
        let util_h = 40.0;
        let util_top = bottom - 18.0 - 6.0 - 3.6 - util_h;
        let card_h = 62.0;
        let card_top = util_top - 18.0 - card_h;
        self.player_card(
            &painter,
            Rect::from_min_size(pos2(l.rail_left, card_top), vec2(nav_w, card_h)),
        );
        let gap = 10.0;
        let util_w = (nav_w - 2.0 * gap) / 3.0;
        for (i, (label, tab)) in [
            ("Profile", UtilityKey::Profile),
            ("Settings", UtilityKey::Settings),
            ("Console", UtilityKey::Console),
        ]
        .into_iter()
        .enumerate()
        {
            let x = l.rail_left + i as f32 * (util_w + gap);
            let rect = Rect::from_min_size(pos2(x, util_top), vec2(util_w, util_h));
            let current = match tab {
                UtilityKey::Profile => self.profile.showing(),
                UtilityKey::Settings => self.tab == Tab::Settings,
                UtilityKey::Console => self.console_open,
            };
            let face = RaisedKey::new(label, rect, KeyLook::gold().sm().flat())
                .current(current)
                .show(ui);
            let (size, em) = if l.narrow { (10.0, 0.06) } else { (11.0, 0.12) };
            KeyText::new(label, "", display_family(), body())
                .size(size, 13.0)
                .spacing(em)
                .pad(4.0, 4.0)
                .layout(KeyTextLayout::Centered)
                .paint(&painter, &face);
            if face.clicked {
                match tab {
                    UtilityKey::Profile => self.open_profile(ProfileScreen::Key),
                    UtilityKey::Settings => self.tab = Tab::Settings,
                    UtilityKey::Console => self.console_toggle = true,
                }
            }
        }

        // `lobby__rail-mid--rooms`: what is left between.
        let mid = Rect::from_min_max(pos2(l.screen.min.x, y), pos2(l.screen.min.x + l.rail_w, card_top - 4.0));
        // On Settings, the page keys take the middle; in a room, the lobby
        // list gives way to the room's chat.
        if self.tab == Tab::Settings {
            self.settings_nav(ui, l, mid);
        } else if self.room_chat.is_some() {
            self.room_chat_panel(ui, l, mid);
        } else if mid.height() > 40.0 {
            self.rooms(ui, l, mid);
        }
    }

    pub(super) fn player_card(&self, painter: &Painter, r: Rect) {
        let cut = r.width() * 0.03;
        painter.add(Shape::convex_polygon(
            vec![
                pos2(r.left() + cut, r.top()),
                r.right_top(),
                pos2(r.right() - cut, r.bottom()),
                r.left_bottom(),
            ],
            hsla(250.0, 45.0, 7.0, 0.92),
            Stroke::NONE,
        ));
        // `lobby__rail-avatar`: cyan to violet, a skewed square.
        let a = Rect::from_min_size(pos2(r.left() + 16.0, r.center().y - 19.0), vec2(38.0, 38.0));
        let corners = [
            pos2(a.left() + a.width() * 0.14, a.top()),
            pos2(a.right(), a.top() + a.height() * 0.08),
            pos2(a.left() + a.width() * 0.88, a.bottom()),
            pos2(a.left(), a.top() + a.height() * 0.9),
        ];
        gradient_polygon(painter, &corners, &[(0.0, hsl(182.0, 95.0, 55.0)), (1.0, violet())]);
        let text_left = a.right() + 12.0;
        let width = r.right() - 16.0 - text_left;
        let display = display_family();
        let name = if self.name.is_empty() { "Player" } else { &self.name };
        let name = elide(painter, name, 18.0, display.clone(), width);
        let name = line(painter, &name, 18.0, display, cream(), 0.05);
        painter.galley(pos2(text_left, r.center().y - name.size().y / 2.0), name, cream());
    }

    /// `.lobby__rail .fps-set-pages`: the settings' page keys in the rail.
    pub(super) fn settings_nav(&mut self, ui: &mut egui::Ui, l: &Layout, mid: Rect) {
        let list = Rect::from_min_max(
            pos2(mid.left() + 22.0, mid.top() + 6.0),
            pos2(mid.left() + l.rail_w - RAIL_SKEW - 12.0 - 14.0, mid.bottom()),
        );
        let key_h = if l.h <= 900.0 { 44.0 } else { 58.0 };
        // A short screen has no room for a column of five under the logo:
        // two by two, so every page stays in sight (`max-height: 820px`).
        let short = l.h <= 820.0;
        let (columns, label_size) = if short {
            (2, clamp_css(10.0, l.w * 0.0095, 13.0))
        } else {
            (1, clamp_css(14.0, l.w * 0.01, 18.0))
        };
        let view = self.settings.get_or_insert_with(FpsSettingsView::default);
        ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("settings-pages")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let top = ui.cursor().min;
                    let r = Rect::from_min_size(top, vec2(list.width() - 4.0, 1.0));
                    let used = view.page_nav(ui, r, key_h, label_size, !l.narrow && !short, columns);
                    ui.allocate_rect(Rect::from_min_size(top, vec2(r.width(), used)), egui::Sense::hover());
                });
        });
    }

    /// The rooms list in the rail's middle.
    pub(super) fn rooms(&mut self, ui: &mut egui::Ui, l: &Layout, mid: Rect) {
        let painter = ui.painter().clone();
        let sub = hsla(250.0, 45.0, 8.0, 0.62);
        let matches = sort_rooms(&self.live_matches);
        let list = Rect::from_min_max(
            pos2(mid.left() + 22.0, mid.top() + 6.0),
            pos2(mid.left() + l.rail_w - RAIL_SKEW - 12.0, mid.bottom()),
        );
        if matches.is_empty() {
            // No rooms: the list stays empty, and says so only when the
            // lobby cannot be reached.
            if self.connected {
                return;
            }
            let empty = if self.lobby_full {
                "Servers are full."
            } else {
                "Lobby server offline."
            };
            let g = wrapped(
                &painter,
                empty,
                FontId::proportional(15.0),
                sub,
                list.width().min(220.0),
            );
            painter.galley(pos2(list.left(), list.top() + 4.0), g, sub);
            return;
        }
        let mut open = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("lobby-rooms")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for m in &matches {
                        let (slot, _) =
                            ui.allocate_exact_size(vec2(list.width() - 14.0, 46.0 + 9.6), egui::Sense::hover());
                        let rect = Rect::from_min_size(slot.min, vec2(slot.width() - 4.0, 46.0));
                        if room_row(ui, rect, m).clicked {
                            open = Some(m.clone());
                        }
                    }
                });
        });
        if let Some(m) = open {
            self.open_room(&m);
        }
    }

    /// The stage: the Play page, a room, or a page not built yet.
    pub(super) fn stage(&mut self, ui: &mut egui::Ui, l: &Layout, style: &TwinStyle, action: &mut Option<PanelAction>) {
        let painter = ui.painter().clone();
        let mode = *self.mode();
        let accent = self.page_accent();
        let page_left = l.screen.min.x + l.rail_w - RAIL_SKEW;
        let page_w = l.screen.max.x - page_left;
        // `lobby__ghost`: the mode's name, huge and outlined, off the bottom
        // left. It shows behind a room too, in the room's colour.
        let ghost = match self.tab {
            Tab::Play => Some(mode.title.to_uppercase()),
            Tab::Settings => Some(self.settings_view().ghost().to_owned()),
            Tab::Leaderboard => Some("TOP".to_owned()),
            Tab::Demos => Some("REPLAY".to_owned()),
            Tab::Campaign => None,
        };
        if let Some(word) = ghost {
            let size = clamp_css(200.0, l.w * 0.25, 440.0);
            let at = pos2(page_left + RAIL_SKEW - page_w * 0.01, l.screen.max.y + l.h * 0.04);
            let clip = painter.with_clip_rect(Rect::from_min_max(pos2(page_left, l.screen.min.y), l.screen.max));
            paint_outline_text(
                &clip,
                DISPLAY_FONT,
                &word,
                size,
                -0.04 * size,
                at,
                Stroke::new(2.0, accent.face.gamma_multiply(0.26)),
            );
        }
        // `lobby__links` along the foot, and the page above it.
        let links_h = 44.0 + 5.0 + 12.0;
        let content = Rect::from_min_max(
            pos2(l.screen.min.x + l.rail_w + 28.0, l.screen.min.y + 30.0),
            pos2(l.screen.max.x - 52.0, l.screen.max.y - links_h - 20.0),
        );
        // The Campaign page is its own night, edge to edge under the links.
        if self.tab == Tab::Campaign {
            let page = Rect::from_min_max(pos2(page_left, l.screen.min.y), l.screen.max);
            let content = Rect::from_min_max(
                content.min,
                pos2(
                    content.max.x.min(content.min.x + 1720.0),
                    l.screen.max.y - links_h - 8.0,
                ),
            );
            match self.campaign.ui(ui, page, content) {
                Some(CampaignAction::Start(mission)) => self.start_campaign_mission(mission, false),
                // `readCard`: the card panel by panel, starting nothing.
                Some(CampaignAction::Read(card, title)) if self.cutscene.is_none() => {
                    self.cutscene = Some(CampaignCutscene::new(Some(card), title));
                }
                // `openBoard`: read fresh from the lobby, or say why it cannot be.
                Some(CampaignAction::OpenBoard) => {
                    if !self.connected {
                        self.campaign
                            .board_failed("The lobby server is offline. The campaign boards come back with it.");
                    } else if !self.campaign_supported {
                        self.campaign.board_failed("This lobby server runs no campaign.");
                    } else {
                        let request = self.client.request_campaign_board();
                        self.campaign.board_loading(request);
                    }
                }
                _ => {}
            }
        }
        if let Some(a) = self.links(ui, l, style) {
            *action = Some(a);
        }
        match self.tab {
            Tab::Campaign => {}
            Tab::Play => self.play_page(ui, l, content, &mode),
            Tab::Settings => {
                // `« Back to welcome`, above the page, while the guide waits.
                let mut content = content;
                if self.guide.suspended {
                    if back_key(ui, content.min) {
                        let can_open = self.guide_can_open();
                        self.guide.resume(can_open);
                    }
                    content.min.y += back_key_size(&ui.painter().clone()).y + 5.0 + 8.0;
                }
                self.settings_view().ui(ui, content);
            }
            Tab::Leaderboard => self.leaderboard.ui(ui, content, &mut self.client),
            Tab::Demos => {
                if let Some(demo) = self.demos.ui(ui, content, &mut self.client, &mut self.levelshots) {
                    self.choice = Some(LobbyChoice {
                        server_url: String::new(),
                        map: demo.path.display().to_string(),
                        admission_ticket: None,
                        name: String::new(),
                        play: false,
                        demo: Some(demo.path),
                        reel: demo.reel,
                        team: None,
                    });
                }
            }
        }
    }

    /// `lobby__links`: the version and the devlog on the Play page, and Quit
    /// (the shell's), right to left along the foot.
    pub(super) fn links(&mut self, ui: &mut egui::Ui, l: &Layout, style: &TwinStyle) -> Option<PanelAction> {
        let painter = ui.painter().clone();
        let bottom = l.screen.max.y - 12.0 - 5.0;
        let mut right = l.screen.max.x - 52.0;
        let mut action = None;
        let key = |ui: &mut egui::Ui, right: &mut f32, label: &str| {
            let g = line(&painter, label, 13.0, display_family(), hsl(43.0, 100.0, 94.0), 0.12);
            let r = Rect::from_min_max(pos2(*right - g.size().x - 32.0, bottom - 44.0), pos2(*right, bottom));
            *right = r.left() - 12.0;
            link_key(ui, &painter, r, g, label)
        };
        if key(ui, &mut right, "QUIT") {
            action = Some(PanelAction::Quit);
        }
        // On the Play page only: a room keeps the foot clear.
        if self.tab == Tab::Play && self.room.is_none() {
            // `.lobby-guide-help`: the welcome guide again.
            let can_open = self.guide_can_open();
            if self.guide.help_shown(can_open) {
                let r = Rect::from_min_max(pos2(right - 44.0, bottom - 44.0), pos2(right, bottom));
                right = r.left() - 12.0;
                if dark_key(ui, "help", r, "?", 17.0) {
                    self.guide.help(can_open);
                }
            }
            // `.lobby-music-key`: the menus' music off and on.
            let r = Rect::from_min_max(pos2(right - 44.0, bottom - 44.0), pos2(right, bottom));
            right = r.left() - 12.0;
            if lobby_music_key(ui, r, self.music_muted) {
                self.music_muted = !self.music_muted;
                set_lobby_music_muted(self.music_muted);
            }
            if key(ui, &mut right, "CREDITS") && self.credits.is_none() {
                self.credits = Some(CreditsDialog::default());
            }
            if key(ui, &mut right, "DEVLOG") {
                action = Some(PanelAction::OpenUrl(DEVLOG_URL.to_owned()));
            }
            // The version, so a screenshot for a bug report carries it.
            let version = line(
                &painter,
                &format!("client {}", style.version.trim_start_matches('v')),
                15.0,
                display_family(),
                hsla(45.0, 100.0, 92.0, 0.7),
                0.16,
            );
            let at = pos2(right - version.size().x, bottom - 22.0 - version.size().y / 2.0);
            painter.galley_with_override_text_color(at + vec2(0.0, 2.0), version.clone(), hsla(0.0, 0.0, 0.0, 0.5));
            painter.galley(at, version, cream());
        }
        action
    }

    /// The title (`lobby__heading`) at the top of its header; returns its
    /// bottom.
    pub(super) fn heading(painter: &Painter, l: &Layout, at: Pos2, title: &str) -> f32 {
        let size = clamp_css(40.0, l.w * 0.044, 74.0);
        let title = line(painter, title, size, display_family(), cream(), 0.0);
        let ty = at.y;
        painter.galley_with_override_text_color(pos2(at.x + 5.0, ty + 6.0), title.clone(), hsl(250.0, 50.0, 3.0));
        let th = title.size().y;
        painter.galley(pos2(at.x, ty), title, cream());
        ty + th
    }

    pub(super) fn play_page(&mut self, ui: &mut egui::Ui, l: &Layout, content: Rect, mode: &GameMode) {
        let painter = ui.painter().clone();
        let accent = mode.accent();
        if self.room.is_some() {
            self.room_page(ui, l, content, &accent);
            return;
        }
        let bottom = Self::heading(&painter, l, content.min, "Pick your fight");

        // `lobby__play-tiles`: a column per mode, two a row when narrow.
        let tile_h = clamp_css(130.0, l.h * 0.18, 176.0);
        let per_row = if l.narrower { 2 } else { 4 };
        let gap = 24.0;
        let tile_w = (content.width() - gap * (per_row as f32 - 1.0)) / per_row as f32;
        let mut y = bottom + 22.0 + 10.0;
        for (i, m) in GAME_MODES.iter().enumerate() {
            let (row, col) = (i / per_row, i % per_row);
            let rect = Rect::from_min_size(
                pos2(
                    content.left() + col as f32 * (tile_w + gap),
                    y + row as f32 * (tile_h + 9.0 + 8.0 + gap * 0.5),
                ),
                vec2(tile_w, tile_h),
            );
            if self.mode_tile(ui, rect, i, m) {
                self.selected_mode = i;
            }
        }
        let rows = GAME_MODES.len().div_ceil(per_row) as f32;
        y += rows * (tile_h + 9.0 + 8.0) + (rows - 1.0) * gap * 0.5 + 22.0;

        // `lobby__play-lower`: the mode on the left, the chat on the right.
        let lower = Rect::from_min_max(pos2(content.left(), y), content.max);
        let (detail, chat) = if l.narrower {
            let split = lower.top() + 200.0_f32.min(lower.height() * 0.5);
            (
                Rect::from_min_max(lower.min, pos2(lower.right(), split)),
                Rect::from_min_max(pos2(lower.left(), split + 16.0), lower.max),
            )
        } else {
            let left_w = (lower.width() - 42.0) / 2.12;
            (
                Rect::from_min_max(lower.min, pos2(lower.left() + left_w, lower.bottom())),
                Rect::from_min_max(pos2(lower.left() + left_w + 42.0, lower.top()), lower.max),
            )
        };
        self.detail(ui, detail, mode);
        if chat.height() > 120.0 {
            self.chat_panel(ui, chat);
        }
    }

    /// A room: the whole page is `MatchLobbyView`'s.
    pub(super) fn room_page(&mut self, ui: &mut egui::Ui, l: &Layout, content: Rect, accent: &Accent) {
        let Some(room) = &mut self.room else { return };
        let shots = &self.levelshots;
        let map_shot = |map: &str| shots.texture(map);
        let style = RoomStyle {
            accent: *accent,
            display: display_family(),
            body: body(),
            window_width: l.w,
            short: l.short,
            narrow: l.narrow,
            narrower: l.narrower,
            site: &self.site,
            online: self.connected,
            map_shot: &map_shot,
        };
        match room.ui(ui, content, &self.client, &style) {
            Some(ViewAction::Leave) => {
                self.show_play_stage();
                self.room_to_rejoin = None;
            }
            Some(ViewAction::Rejoin(match_id)) => {
                let request = self.client.request_seat_reissue(&match_id);
                self.seat_request = Some((request, match_id));
            }
            None => {}
        }
    }

    /// A mode's tile (`raised-key--mode`); true when picked.
    pub(super) fn mode_tile(&self, ui: &mut egui::Ui, rect: Rect, index: usize, m: &GameMode) -> bool {
        let accent = m.accent();
        let on = index == self.selected_mode;
        let rest = KeyLook {
            face: vec![
                (0.0, hsl(45.0, 100.0, 97.0)),
                (0.62, hsl(42.0, 58.0, 89.0)),
                (1.0, hsl(40.0, 40.0, 80.0)),
            ],
            side: [hsl(36.0, 24.0, 56.0), hsl(30.0, 20.0, 38.0)],
            shadow: hsla(250.0, 70.0, 2.0, 0.6),
            depth: 9.0,
            cut: 0.022,
            ..KeyLook::gold()
        };
        let picked = KeyLook {
            face: vec![(0.0, accent.hi), (0.58, accent.face), (1.0, accent.lo)],
            side: [accent.side_hi, accent.side_lo],
            sub: hsla(250.0, 45.0, 8.0, 0.66),
            ..rest.clone()
        };
        let key = if on {
            RaisedKey::new(("mode", m.id), rect, picked.clone())
                .rest_press(-0.5)
                .hover(picked, -0.75)
                .active_face(vec![(0.0, accent.face), (1.0, accent.lo)])
        } else {
            RaisedKey::new(("mode", m.id), rect, rest)
        };
        let face = key.show(ui);
        let painter = ui.painter();
        // `lobby__mode-band`: the mode's colour down the left edge.
        let r = face.rect;
        let cut = rect.width() * 0.022;
        let band = if on { accent.side_hi } else { accent.face };
        painter.add(Shape::convex_polygon(
            vec![
                pos2(r.left() + cut, r.top()),
                pos2(r.left() + cut + 16.0, r.top()),
                pos2(r.left() + 16.0, r.bottom()),
                r.left_bottom(),
            ],
            band,
            Stroke::NONE,
        ));
        let inner = Rect::from_min_max(r.min + vec2(40.0, 18.0), r.max - vec2(26.0, 22.0));
        let number = line(
            painter,
            &format!("{:02}", index + 1),
            13.0,
            display_family(),
            face.sub,
            0.14,
        );
        painter.galley(inner.left_top(), number, face.sub);
        let available = self.mode_ready(m.id);
        if self.connected && !available {
            let soon = line(painter, "SOON", 13.0, display_family(), face.sub, 0.14);
            painter.galley(pos2(inner.right() - soon.size().x, inner.top()), soon, face.sub);
        }
        let name_size = clamp_css(28.0, ui.ctx().content_rect().width() * 0.026, 46.0);
        let name = line(
            painter,
            &m.title.to_uppercase(),
            name_size,
            display_family(),
            face.text,
            0.02,
        );
        let subtitle = line(painter, &m.subtitle.to_uppercase(), 14.0, body(), face.sub, 0.18);
        let sub_y = inner.bottom() - subtitle.size().y;
        let name_y = sub_y - 8.0 - name.size().y;
        painter.galley(pos2(inner.left(), name_y), name, face.text);
        painter.galley(pos2(inner.left(), sub_y), subtitle, face.sub);
        face.clicked
    }

    /// `lobby__play-detail`: the hook, Find and Private lobby, the notice.
    pub(super) fn detail(&mut self, ui: &mut egui::Ui, r: Rect, mode: &GameMode) {
        let painter = ui.painter().clone();
        let tagline = wrapped(
            &painter,
            mode.hook,
            FontId::proportional(24.0),
            hsla(45.0, 100.0, 95.0, 0.9),
            r.width().min(520.0),
        );
        let mut y = r.top();
        let tag_h = tagline.size().y;
        painter.galley(pos2(r.left(), y), tagline, cream());
        y += tag_h + 16.0 + 8.0;
        let keys = play_keys(PlayKeysInput {
            mode_title: mode.title,
            available: self.mode_ready(mode.id),
            connected: self.connected,
            queue_pending: self.queue_pending,
            private_lobby_supported: self.private_lobby_supported,
            lobby_full: self.lobby_full,
        });
        // The arcade key: its label's width, padded.
        let label = line(
            &painter,
            &keys.find_label.to_uppercase(),
            30.0,
            display_family(),
            cream(),
            0.1,
        );
        let find_w = (label.size().x + 126.0).min(r.width());
        let find = Rect::from_min_size(pos2(r.left(), y), vec2(find_w, 90.0));
        let face = RaisedKey::new("find", find, KeyLook::cta(&mode.accent()))
            .disabled(keys.find_disabled)
            .show(ui);
        KeyText::new(&keys.find_label, "", display_family(), body())
            .size(30.0, 13.0)
            .spacing(0.1)
            .pad(60.0, 66.0)
            .layout(KeyTextLayout::Centered)
            .paint(&painter, &face);
        if face.clicked {
            self.show_match_lobby(mode.id);
        }
        // Private lobby, beside it when it fits, else under it.
        let private_label = "Private lobby";
        let hint = "Invite only or bot matches";
        let lw = line(
            &painter,
            &private_label.to_uppercase(),
            16.0,
            display_family(),
            cream(),
            0.1,
        )
        .size()
        .x;
        let hw = line(&painter, &hint.to_uppercase(), 13.0, body(), cream(), 0.12)
            .size()
            .x;
        let private_w = lw.max(hw) + 62.0;
        let beside = find.right() + 9.6 * 0.75 + 30.0;
        let at = if beside + private_w <= r.right() {
            pos2(beside, find.center().y - 32.0)
        } else {
            pos2(r.left(), find.bottom() + 9.6 + 12.0)
        };
        let private = Rect::from_min_size(at, vec2(private_w, 64.0));
        let face = RaisedKey::new("private", private, KeyLook::cream().depth(6.6))
            .disabled(keys.private_disabled)
            .show(ui);
        KeyText::new(private_label, hint, display_family(), body())
            .size(16.0, 13.0)
            .spacing(0.1)
            .pad(32.0, 30.0)
            .layout(KeyTextLayout::Stacked)
            .paint(&painter, &face);
        if face.clicked {
            self.show_private_lobby(mode.id);
        }
        let notice_y = private.bottom().max(find.bottom()) + 9.6 + 16.0;
        if !self.notice.is_empty() {
            let n = wrapped(
                &painter,
                &self.notice,
                FontId::proportional(16.0),
                hsl(46.0, 100.0, 80.0),
                r.width(),
            );
            painter.galley(pos2(r.left(), notice_y), n, cream());
        }
    }

    /// `lobby-chat`: the lobby's chat beside the mode.
    pub(super) fn chat_panel(&mut self, ui: &mut egui::Ui, r: Rect) {
        let send = chat_box(
            ui,
            r,
            ChatBox {
                id: "lobby-chat",
                title: "LOBBY",
                count: "",
                pips: Some(online_pips(
                    if self.connected { self.online.len() } else { 0 },
                    self.lobby_capacity,
                )),
                lines: &self.chat,
                draft: &mut self.chat_draft,
                placeholder: if self.connected {
                    "Say something…"
                } else {
                    "Lobby offline"
                },
                enabled: self.connected,
                empty: if self.connected {
                    "Nobody has said anything yet. Say hello."
                } else {
                    "Chat opens when the lobby server is back."
                },
                mine: &self.name,
                rail: false,
            },
        );
        if send {
            self.send_chat();
        }
    }

    /// The room's chat in the rail's middle (`lobby__rail--room .lobby-chat`).
    pub(super) fn room_chat_panel(&mut self, ui: &mut egui::Ui, l: &Layout, mid: Rect) {
        let Some(chat) = &mut self.room_chat else { return };
        let r = Rect::from_min_max(
            pos2(mid.left() + 22.0, mid.top() + 4.0),
            pos2(mid.left() + l.rail_w - RAIL_SKEW - 18.0, mid.bottom() - 16.0),
        );
        if r.height() < 90.0 {
            return;
        }
        let enabled = self.connected && chat.supported;
        let count = chat.here().to_uppercase();
        let placeholder = chat.placeholder(self.connected);
        let empty = chat.empty_text();
        let send = chat_box(
            ui,
            r,
            ChatBox {
                id: "room-chat",
                title: "ROOM CHAT",
                count: &count,
                pips: None,
                lines: &chat.lines,
                draft: &mut chat.draft,
                placeholder,
                enabled,
                empty,
                mine: &self.name,
                rail: true,
            },
        );
        if send {
            chat.send(&self.client, self.connected);
        }
    }
}

/// What a chat box shows: the lobby's on the Play page, a room's in the rail.
pub(super) struct ChatBox<'a> {
    pub(super) id: &'static str,
    pub(super) title: &'a str,
    pub(super) count: &'a str,
    /// How full the lobby is: this many of [`ONLINE_PIPS`] lit, in place of a count.
    pub(super) pips: Option<usize>,
    pub(super) lines: &'a VecDeque<ChatLine>,
    pub(super) draft: &'a mut String,
    pub(super) placeholder: &'a str,
    pub(super) enabled: bool,
    pub(super) empty: &'a str,
    /// This player's name, whose lines are lit.
    pub(super) mine: &'a str,
    /// In the rail: a solid slab, tighter, its cut corner smaller.
    pub(super) rail: bool,
}

/// `lobby-chat`: head, log and form. True when the player sends a line.
pub(super) fn chat_box(ui: &mut egui::Ui, r: Rect, c: ChatBox<'_>) -> bool {
    let painter = ui.painter().clone();
    let rule = Stroke::new(1.0, hsla(276.0, 90.0, 64.0, 0.2));
    if c.rail {
        cut_panel(
            &painter,
            r,
            18.0,
            &[(0.0, hsla(250.0, 45.0, 7.0, 0.94)), (1.0, hsla(250.0, 45.0, 7.0, 0.94))],
            None,
        );
    } else {
        cut_panel(
            &painter,
            r,
            22.0,
            &[
                (0.0, hsla(250.0, 40.0, 11.0, 0.72)),
                (1.0, hsla(250.0, 44.0, 7.0, 0.82)),
            ],
            Some(Stroke::new(1.5, hsla(276.0, 90.0, 64.0, 0.32))),
        );
    }
    let inner = if c.rail {
        Rect::from_min_max(r.min + vec2(16.0, 14.0), r.max - vec2(16.0, 14.0))
    } else {
        Rect::from_min_max(r.min + vec2(22.0, 16.0), r.max - vec2(20.0, 16.0))
    };
    // `lobby-chat__head`.
    let title = line(&painter, c.title, 14.0, display_family(), cream(), 0.16);
    let head_h = title.size().y;
    painter.galley(inner.left_top(), title, cream());
    let count = line(&painter, c.count, 12.0, body(), hsla(45.0, 100.0, 95.0, 0.55), 0.14);
    let cx = inner.right() - count.size().x;
    let cy = inner.top() + head_h / 2.0;
    painter.galley(pos2(cx, cy - count.size().y / 2.0), count, cream());
    if let Some(lit) = c.pips {
        // Four slanted pips, as the video settings show cost.
        let (w, h, gap) = (18.0, 9.0, 3.0);
        let total = ONLINE_PIPS as f32 * w + (ONLINE_PIPS as f32 - 1.0) * gap;
        let pink = hsl(322.0, 100.0, 58.0);
        for i in 0..ONLINE_PIPS {
            let x = inner.right() - total + i as f32 * (w + gap);
            let r = Rect::from_min_size(pos2(x, cy - h / 2.0), vec2(w, h));
            let pts = vec![
                pos2(r.left() + w * 0.25, r.top()),
                r.right_top(),
                pos2(r.right() - w * 0.25, r.bottom()),
                r.left_bottom(),
            ];
            if i < lit {
                painter.add(Shape::convex_polygon(
                    pts.iter().map(|p| *p + (*p - r.center()) * 0.4).collect(),
                    pink.gamma_multiply(0.2),
                    Stroke::NONE,
                ));
                painter.add(Shape::convex_polygon(pts, pink, Stroke::NONE));
            } else {
                painter.add(Shape::convex_polygon(pts, hsla(45.0, 100.0, 95.0, 0.14), Stroke::NONE));
            }
        }
    }
    let head_bottom = inner.top() + head_h + 12.0;
    painter.hline(inner.x_range(), head_bottom, rule);

    // `lobby-chat__form`, at the foot.
    let form_top = inner.bottom() - 44.0 - 3.6;
    painter.hline(inner.x_range(), form_top - 12.0, rule);
    let send_label = line(&painter, "SEND", 13.0, display_family(), cream(), 0.12);
    let pad = if c.rail { (16.0, 14.0) } else { (24.0, 22.0) };
    let send_w = send_label.size().x + pad.0 + pad.1;
    let send = Rect::from_min_size(pos2(inner.right() - send_w - 4.0, form_top), vec2(send_w, 44.0));
    let gap = if c.rail { 10.0 } else { 14.0 };
    let field = Rect::from_min_max(pos2(inner.left(), form_top), pos2(send.left() - gap, form_top + 44.0));
    let typed = TextField::new(c.id, field, c.draft, FontId::proportional(18.0))
        .placeholder(c.placeholder)
        .enabled(c.enabled)
        .show(ui);
    let face = RaisedKey::new((c.id, "send"), send, KeyLook::gold().sm())
        .disabled(!c.enabled)
        .show(ui);
    KeyText::new("Send", "", display_family(), body())
        .size(13.0, 13.0)
        .spacing(0.12)
        .pad(pad.0, pad.1)
        .layout(KeyTextLayout::Centered)
        .paint(&painter, &face);
    let sent = c.enabled && (typed.submitted || face.clicked);

    // `lobby-chat__log`: follows new lines while at the bottom.
    let log = Rect::from_min_max(
        pos2(inner.left(), head_bottom + 6.0),
        pos2(inner.right(), form_top - 18.0),
    );
    if log.height() < 20.0 {
        return sent;
    }
    if c.lines.is_empty() {
        let size = if c.rail { 16.0 } else { 17.0 };
        let alpha = if c.rail { 0.58 } else { 0.5 };
        let g = wrapped(
            &painter,
            c.empty,
            FontId::proportional(size),
            hsla(45.0, 100.0, 95.0, alpha),
            log.width(),
        );
        painter.galley(pos2(log.left(), log.top() + 14.0), g, cream());
        return sent;
    }
    let time_w = if c.rail { 42.0 } else { 50.0 };
    let size = if c.rail { 16.0 } else { 18.0 };
    ui.scope_builder(egui::UiBuilder::new().max_rect(log), |ui| {
        egui::ScrollArea::vertical()
            .id_salt((c.id, "log"))
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for chat in c.lines {
                    chat_row(ui, chat, c.mine, log.width() - 12.0, time_w, size);
                }
            });
    });
    sent
}

/// `lobby-credits-key`: a dark key on a 5px base, sinking onto it when held.
pub(super) fn link_key(ui: &mut egui::Ui, painter: &Painter, r: Rect, label: Arc<egui::Galley>, id: &str) -> bool {
    let response = ui.interact(
        r.expand2(vec2(0.0, 5.0)),
        ui.scope_id().with(("link", id)),
        egui::Sense::click(),
    );
    let held = response.is_pointer_button_down_on();
    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let sink = if held { 4.0 } else { 0.0 };
    let base = r.translate(vec2(0.0, 5.0));
    painter.rect_filled(
        base.translate(vec2(0.0, 4.0 - sink * 0.8)).expand(1.0),
        4.0,
        hsla(0.0, 0.0, 0.0, 0.25),
    );
    painter.rect_filled(base, 4.0, Color32::from_rgb(0x0d, 0x0b, 0x16));
    let face = r.translate(vec2(0.0, sink));
    let (top, foot) = if hovered {
        (Color32::from_rgb(0x57, 0x4b, 0x6d), Color32::from_rgb(0x28, 0x21, 0x37))
    } else {
        (Color32::from_rgb(0x39, 0x35, 0x4b), Color32::from_rgb(0x19, 0x16, 0x26))
    };
    painter.add(egui::epaint::RectShape::filled(face, 4.0, foot));
    gradient_polygon(
        painter,
        &[
            face.left_top() + vec2(2.0, 0.0),
            face.right_top() - vec2(2.0, 0.0),
            face.right_bottom() - vec2(2.0, 0.0),
            face.left_bottom() + vec2(2.0, 0.0),
        ],
        &[(0.0, top), (1.0, foot)],
    );
    painter.hline(
        (face.left() + 3.0)..=(face.right() - 3.0),
        face.top() + 0.5,
        Stroke::new(1.0, hsla(0.0, 0.0, 100.0, 0.2)),
    );
    let size = label.size();
    painter.galley(face.center() - size / 2.0, label, cream());
    response.clicked()
}

/// One room in the rail (`raised-key--sm raised-key--cream raised-key--room`).
pub(super) fn room_row(ui: &mut egui::Ui, rect: Rect, m: &LiveMatchInfo) -> KeyFace {
    let row = describe_room(m);
    let mode = GAME_MODES.iter().find(|g| g.id == m.gametype);
    let face = RaisedKey::new(("room", m.id.as_str()), rect, KeyLook::cream().sm())
        // A full room is still worth reading, so it stays lit.
        .disabled(row.disabled)
        .show(ui);
    let painter = ui.painter();
    let r = face.rect;
    let (text_c, sub) = if row.disabled {
        (face.text.gamma_multiply(1.4), face.sub.gamma_multiply(1.4))
    } else {
        (face.text, face.sub)
    };
    let mut x = r.left() + 20.0;
    let cy = r.center().y;
    // `lobby__room-mark`, pulsing while the match runs.
    let (h, s, l) = mode.map_or((250.0, 20.0, 40.0), |g| g.mark);
    let pulse = if m.state == "playing" {
        let t = ui.ctx().input(|i| i.time) as f32;
        0.65 + 0.35 * (t * std::f32::consts::TAU / 1.6).cos()
    } else {
        1.0
    };
    painter.add(Shape::convex_polygon(
        vec![
            pos2(x + 2.2, cy - 5.0),
            pos2(x + 10.0, cy - 5.0),
            pos2(x + 7.8, cy + 5.0),
            pos2(x, cy + 5.0),
        ],
        hsl(h, s, l).gamma_multiply(pulse),
        Stroke::NONE,
    ));
    x += 10.0 + 10.0;
    let title = mode.map_or_else(
        || {
            if m.gametype_name.is_empty() {
                m.gametype.clone()
            } else {
                m.gametype_name.clone()
            }
        },
        |g| g.title.to_owned(),
    );
    let name = line(painter, &title.to_uppercase(), 14.0, display_family(), text_c, 0.07);
    painter.galley(pos2(x, cy - name.size().y / 2.0), name, text_c);
    x += 66.0 + 10.0;
    // From the right: the badge, then the seats.
    let badge_text = line(
        painter,
        &row.action.label().to_uppercase(),
        10.0,
        display_family(),
        Color32::WHITE,
        0.1,
    );
    let badge_w = (badge_text.size().x + 20.0).max(54.0);
    let badge = Rect::from_min_size(pos2(r.right() - 16.0 - badge_w, cy - 9.0), vec2(badge_w, 18.0));
    let (badge_fg, badge_bg) = match row.action {
        RoomAction::Join => (hsl(46.0, 100.0, 60.0), hsl(250.0, 45.0, 7.0)),
        RoomAction::Watch if face.hovered => (hsla(45.0, 100.0, 96.0, 0.85), Color32::from_white_alpha(41)),
        RoomAction::Watch => (hsl(322.0, 85.0, 38.0), hsla(322.0, 90.0, 52.0, 0.16)),
        RoomAction::Full => (hsla(250.0, 45.0, 8.0, 0.45), hsla(250.0, 45.0, 8.0, 0.1)),
    };
    let bc = badge.width() * 0.08;
    painter.add(Shape::convex_polygon(
        vec![
            pos2(badge.left() + bc, badge.top()),
            badge.right_top(),
            pos2(badge.right() - bc, badge.bottom()),
            badge.left_bottom(),
        ],
        badge_bg,
        Stroke::NONE,
    ));
    let badge_label = line(
        painter,
        &row.action.label().to_uppercase(),
        10.0,
        display_family(),
        badge_fg,
        0.1,
    );
    painter.galley(badge.center() - badge_label.size() / 2.0, badge_label, badge_fg);
    let seats = line(
        painter,
        &format!("{} / {}", m.player_count, m.max_players),
        13.0,
        body(),
        sub,
        0.12,
    );
    let seats_x = badge.left() - 10.0 - seats.size().x.max(42.0);
    painter.galley(pos2(seats_x, cy - seats.size().y / 2.0), seats, sub);
    // The map, while it is being played, in what is left.
    if m.state == "playing" {
        let room = seats_x - 10.0 - x;
        if room > 20.0 {
            let shown = elide(painter, &row.where_, 13.0, body(), room);
            let map = line(painter, &shown, 13.0, body(), sub, 0.06);
            painter.galley(pos2(x, cy - map.size().y / 2.0), map, sub);
        }
    }
    face
}

/// One chat line: time, who, what (`lobby-chat__msg`).
pub(super) fn chat_row(ui: &mut egui::Ui, chat: &ChatLine, mine: &str, width: f32, time_w: f32, size: f32) {
    let painter = ui.painter().clone();
    let time = line(
        &painter,
        &clock_time(chat.ts),
        12.0,
        body(),
        hsla(45.0, 100.0, 95.0, 0.3),
        0.08,
    );
    let text_x = time_w + 12.0;
    let sys = size - 2.0;
    let (who, body_text, color) = if chat.system || chat.from.is_empty() {
        let mut job = egui::text::LayoutJob::default();
        // The diamond is drawn: the body face has no `◆`.
        job.append(
            &chat.text,
            sys + 6.0,
            egui::TextFormat {
                font_id: FontId::proportional(sys),
                color: hsla(45.0, 100.0, 95.0, 0.45),
                italics: true,
                ..Default::default()
            },
        );
        job.wrap.max_width = (width - text_x).max(10.0);
        (None, painter.layout_job(job), cream())
    } else {
        let me = chat.from == mine;
        let who_color = if me {
            hsl(46.0, 100.0, 60.0)
        } else {
            name_color(&chat.from)
        };
        let who = line(&painter, &chat.from, 14.0, display_family(), who_color, 0.04);
        let left = text_x + who.size().x + 12.0;
        let said = wrapped(
            &painter,
            &chat.text,
            FontId::proportional(size),
            hsla(45.0, 100.0, 95.0, 0.9),
            width - left,
        );
        (Some((who, me)), said, cream())
    };
    let height = body_text.size().y.max(18.0) + 12.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
    let y = rect.top() + 6.0;
    let baseline = |g: &egui::Galley| y + body_text.rows.first().map_or(0.0, |r| r.max_y()) - g.size().y;
    if who.as_ref().is_some_and(|(_, me)| *me) {
        horizontal_gradient(
            &painter,
            rect,
            &[(0.0, hsla(46.0, 100.0, 60.0, 0.1)), (0.7, Color32::TRANSPARENT)],
        );
    }
    let tx = rect.left() + 4.0;
    let time_y = baseline(&time);
    painter.galley(pos2(tx, time_y), time, cream());
    let mut x = rect.left() + 4.0 + text_x;
    if let Some((who, _)) = who {
        let who_y = baseline(&who);
        let w = who.size().x;
        painter.galley(pos2(x, who_y), who, cream());
        x += w + 12.0;
    }
    if chat.system || chat.from.is_empty() {
        let d = sys * 0.3;
        let c = pos2(
            x + d + 1.0,
            y + body_text.rows.first().map_or(sys, |r| r.max_y()) * 0.55,
        );
        painter.add(Shape::convex_polygon(
            vec![c - vec2(0.0, d), c + vec2(d, 0.0), c + vec2(0.0, d), c - vec2(d, 0.0)],
            hsl(276.0, 90.0, 72.0),
            Stroke::NONE,
        ));
    }
    painter.galley(pos2(x, y), body_text, color);
}

/// Pips in the chat's online meter (`ONLINE_PIPS`).
pub(super) const ONLINE_PIPS: usize = 4;

/// `refreshPlayerList`: how many of the pips are lit for `online` players
/// against the lobby's cap; one as soon as anyone is.
pub(super) fn online_pips(online: usize, capacity: usize) -> usize {
    if online == 0 {
        return 0;
    }
    let share = (online as f64 / capacity.max(1) as f64 * ONLINE_PIPS as f64).ceil() as usize;
    share.clamp(1, ONLINE_PIPS)
}

/// The scanlines of the campaign's purple plates, across a key's face.
pub(super) fn rail_scanlines(painter: &Painter, face: Rect, cut: f32) {
    let inset = face.width() * cut;
    let clip = painter.with_clip_rect(face.shrink2(vec2(inset, 0.0)).intersect(painter.clip_rect()));
    let mut y = face.top();
    while y < face.bottom() {
        clip.rect_filled(
            Rect::from_min_max(pos2(face.left(), y), pos2(face.right(), y + 2.0)),
            0.0,
            hsla(276.0, 70.0, 70.0, 0.08),
        );
        y += 5.0;
    }
}

/// The Campaign key's label: italic, on a pink shadow.
pub(super) fn campaign_key_label(painter: &Painter, face: &KeyFace, label: &str, size: f32, pad: f32) {
    let galley = |color: Color32| {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            &label.to_uppercase(),
            0.0,
            egui::TextFormat {
                font_id: FontId::new(size, display_family()),
                color,
                italics: true,
                extra_letter_spacing: size * 0.07,
                ..Default::default()
            },
        );
        painter.layout_job(job)
    };
    let shadow_color = hsla(322.0, 95.0, 40.0, 0.7);
    let g = galley(face.text);
    let at = pos2(face.rect.left() + pad, face.rect.center().y - g.size().y / 2.0);
    painter.galley(at + vec2(0.06 * size, 0.07 * size), galley(shadow_color), shadow_color);
    painter.galley(at, g, face.text);
}

/// `lobby__key-radar`: the room's search radar, small, in the Lobby key's
/// cyan: a sweep, two rings, a blip and a core. It always sweeps.
pub(super) fn key_radar(painter: &Painter, centre: Pos2, time: f64) {
    let r = 17.0;
    let cyan = hsl(182.0, 95.0, 55.0);
    painter.circle_filled(centre, r, hsl(250.0, 44.0, 7.0));
    // The sweep: a quarter-and-a-bit wedge fading in behind its edge.
    let turn = ((time / 2.2).fract() as f32) * std::f32::consts::TAU;
    let steps = 24;
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let t1 = (i + 1) as f32 / steps as f32;
        // `conic-gradient(transparent 0 70%, cyan 100%)`, measured back from the edge.
        let a0 = turn - std::f32::consts::TAU * 0.3 * (1.0 - t0);
        let a1 = turn - std::f32::consts::TAU * 0.3 * (1.0 - t1);
        // CSS turns start at the top, clockwise.
        let at = |a: f32| centre + vec2(a.sin(), -a.cos()) * r;
        painter.add(Shape::convex_polygon(
            vec![centre, at(a0), at(a1)],
            cyan.gamma_multiply(0.9 * t1),
            Stroke::NONE,
        ));
    }
    painter.circle_stroke(centre, r - 6.0, Stroke::new(1.0, cyan.gamma_multiply(0.4)));
    painter.circle_stroke(centre, r - 12.0, Stroke::new(1.0, cyan.gamma_multiply(0.4)));
    // The blip, lit as the sweep passes it (34% to 80% of a turn).
    let phase = (time / 2.2).fract();
    let blip = if (0.3..0.34).contains(&phase) {
        ((phase - 0.3) / 0.04) as f32
    } else if (0.34..0.8).contains(&phase) {
        1.0 - ((phase - 0.34) / 0.46) as f32
    } else {
        0.0
    };
    if blip > 0.0 {
        let at = centre + vec2(6.0, -8.0);
        painter.circle_filled(at, 4.0, cyan.gamma_multiply(0.3 * blip));
        painter.circle_filled(at, 2.0, hsl(182.0, 100.0, 80.0).gamma_multiply(blip));
    }
    painter.circle_filled(centre, 3.0, cyan);
    painter.circle_stroke(centre, r, Stroke::new(1.5, cyan.gamma_multiply(0.6)));
}
