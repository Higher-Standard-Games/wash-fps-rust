//! The window's input: the console, the pause menu, pads, chat, the demo
//! and reel keys, and the commands a frame sends. Split out of `window.rs`; the same module tree.

use super::*;

impl App {
    /// The backquote opens and closes the console (`DebugConsole`'s
    /// keyboard handler); while it is open, the keys and the mouse are its.
    pub(super) fn console_event(&mut self, event: &WindowEvent) -> bool {
        let chat_typing = self
            .surface
            .as_ref()
            .and_then(|s| s.hud.as_ref())
            .is_some_and(|h| h.chat.is_input_active());
        // Escape closes the open console (`DebugConsole`'s key handler).
        if let WindowEvent::KeyboardInput { event: key, .. } = event
            && self.console.console.open
            && key.physical_key == PhysicalKey::Code(KeyCode::Escape)
        {
            if key.state == ElementState::Pressed && !key.repeat {
                self.console.console.toggle();
            }
            return true;
        }
        // A backquote typed into the chat is the chat's.
        if let WindowEvent::KeyboardInput { event: key, .. } = event
            && key.physical_key == PhysicalKey::Code(KeyCode::Backquote)
            && !chat_typing
        {
            if key.state == ElementState::Pressed && !key.repeat {
                self.console.console.toggle();
                if self.console.console.open {
                    self.set_captured(false);
                    self.held.clear();
                    self.firing = false;
                    with_input(InputSystem::release_held_input);
                }
            }
            return true;
        }
        let Some(s) = &mut self.surface else { return false };
        let used = s.egui.on_window_event(&s.window, event);
        // A click the demo bar took is not a click on the game.
        // Its release still reaches the input system, or a button would stay held.
        if used
            && !self.captured
            && self.demo_hud.is_some()
            && matches!(
                event,
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    ..
                }
            )
        {
            return true;
        }
        self.console.console.open
            && (used
                || matches!(
                    event,
                    WindowEvent::KeyboardInput { .. } | WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. }
                ))
    }

    /// Playing in a match, before its post-game (where the board shows
    /// the way home, and `showMatchMenu` stays away).
    pub(super) fn in_play(&self) -> bool {
        matches!(self.feed, Some(Feed::Play(..))) && !self.opened_up
    }

    /// Escape in play: the mouse goes and the menu comes up; with the menu
    /// up, Escape resumes (`FPSHud`'s `pointerlockchange`, `MatchInputBindings`).
    /// While the menu is up the keys are its, not the game's.
    pub(super) fn pause_menu_key(&mut self, code: KeyCode, event: &winit::event::KeyEvent) -> bool {
        if !self.in_play() {
            self.pause_menu.close();
            return false;
        }
        // The settings have the keyboard; what reaches the window is not
        // the game's. Escape closes what the settings have open, then the
        // settings, back to the menu.
        if self.settings_open {
            if code == KeyCode::Escape
                && event.state == ElementState::Pressed
                && !event.repeat
                && let Some(view) = &mut self.native_settings
                && !view.back()
            {
                self.close_native_settings();
            }
            return true;
        }
        // The kit dialog has the keys; Escape backs out when it can.
        if self.loadout_picker.is_open() {
            if code == KeyCode::Escape
                && event.state == ElementState::Pressed
                && !event.repeat
                && let Some(outcome) = self.loadout_picker.escape()
            {
                self.loadout_outcome(outcome);
            }
            return true;
        }
        if code == KeyCode::Escape {
            if event.state == ElementState::Pressed && !event.repeat {
                if self.pause_menu.open {
                    self.pause_menu_action(PauseMenuAction::Resume);
                } else {
                    self.set_captured(false);
                    self.pause_menu.open();
                }
            }
            return true;
        }
        self.pause_menu.open && event.state == ElementState::Pressed
    }

    /// A key on the Esc menu.
    pub(super) fn pause_menu_action(&mut self, action: PauseMenuAction) {
        self.pause_menu.close();
        match action {
            PauseMenuAction::Resume => self.set_captured(true),
            PauseMenuAction::Loadout => {
                // `openLoadoutFromMenu`: the kit in use pre-selected.
                if let Some(Feed::Play(p, _)) = &self.feed {
                    let state = p.match_state();
                    let mode = state.gametype_name().to_owned();
                    let timing = state.loadout_mode().map(str::to_owned);
                    let current = loadout_picker::remembered(&mode);
                    self.loadout_picker.open(current, timing.as_deref(), true);
                }
            }
            PauseMenuAction::Settings => {
                self.native_settings = Some(washoi::client::settings::fps_settings_view::FpsSettingsView::new(true));
                self.settings_open = true;
            }
            PauseMenuAction::Leave => {
                println!("lobby: back to the lobby");
                self.leave = true;
            }
        }
    }

    /// Back from the native settings to the match menu.
    pub(super) fn close_native_settings(&mut self) {
        if let Some(mut view) = self.native_settings.take() {
            view.pause_controls();
        }
        self.settings_open = false;
        if self.in_play() {
            self.pause_menu.open();
        }
    }

    /// `joinGame`'s picker: a mode with kits waits for one before the join,
    /// with the mouse free to choose it.
    pub(super) fn offer_loadout(&mut self) {
        let Some(Feed::Play(p, _)) = &self.feed else { return };
        if !p.wants_loadout() || self.loadout_picker.is_open() {
            return;
        }
        let state = p.match_state();
        let mode = state.gametype_name().to_owned();
        let timing = state.loadout_mode().map(str::to_owned);
        let current = loadout_picker::remembered(&mode);
        self.pause_menu.close();
        self.loadout_picker.open(current, timing.as_deref(), false);
        self.set_captured(false);
    }

    /// The kit dialog closed: a kit is sent (and remembered) and the player
    /// goes back to the game; backing out returns to the Esc menu.
    pub(super) fn loadout_outcome(&mut self, outcome: LoadoutOutcome) {
        match outcome {
            LoadoutOutcome::Confirm(kit) => {
                if let Some(Feed::Play(p, _)) = &mut self.feed {
                    loadout_picker::remember(p.match_state().gametype_name(), kit);
                    p.choose_loadout(kit.primary, kit.secondary);
                }
                if self.in_play() {
                    self.set_captured(true);
                }
            }
            LoadoutOutcome::Cancel => {
                if self.in_play() {
                    self.pause_menu.open();
                }
            }
        }
    }

    /// F watching: chase the next player by name, then back to the free
    /// camera.
    pub(super) fn chase_next(&mut self) {
        let order = self
            .feed
            .as_ref()
            .map_or_else(Vec::new, |f| name_order(f.roster(), &self.drawn));
        let next = match self
            .follow_id
            .as_ref()
            .and_then(|id| order.iter().position(|o| o == id))
        {
            None if !self.free_camera || self.follow_id.is_none() => order.first(),
            None => None,
            Some(i) => order.get(i + 1),
        };
        self.follow_id = next.cloned();
        self.free_camera = self.follow_id.is_none();
    }

    /// The pads this frame (`GamepadInput.tick`, then `ControllerInput.update`):
    /// a press on a pad hands the controls to it, and in play the pad holds
    /// its actions, moves, turns and opens the menu; in the menu it walks
    /// the rail.
    pub(super) fn poll_pads(&mut self, dt: f32) {
        let pads = self.pads.poll();
        self.pad.sample(pads);
        let standard = self.pad.pad.as_ref().is_some_and(|p| p.mapping == "standard");
        let stick = (0..4).any(|i| self.pad.axis(i).abs() > 0.5);
        if standard && (!self.pad.pressed.is_empty() || stick) {
            self.device.follow_pad();
        }
        let typing = self
            .surface
            .as_ref()
            .and_then(|s| s.hud.as_ref())
            .is_some_and(|h| h.chat.is_input_active());
        let ctx = ControllerContext {
            enabled: self.device.gamepad(),
            in_play: self.captured,
            menu: self.pause_menu.open || self.loadout_picker.is_open(),
            blocked: self.surface.is_none() || self.settings_open || self.console.console.open || typing,
        };
        let cvars = PadCvars::read();
        let (controller, pad) = (&mut self.controller, &mut self.pad);
        let frame = with_input(|i| controller.update(f64::from(dt), pad, i, ctx, cvars));
        self.pad_move = frame.move_axis;
        if let Some(held) = frame.scoreboard {
            self.pad_scoreboard = held;
        }
        if frame.leave_play || frame.menu {
            // `leavePlay`: in a match the menu comes up, as for Escape.
            if self.in_play() && !self.pause_menu.open {
                self.pause_menu.open();
            }
            self.set_captured(false);
        }
        if frame.enter_play
            && !self.opened_up
            && !self.pause_menu.open
            && !self.settings_open
            && !self.loadout_picker.is_open()
        {
            self.set_captured(true);
        }
        for intent in frame.nav {
            if self.loadout_picker.is_open() {
                if let Some(outcome) = self.loadout_picker.nav(intent) {
                    self.loadout_outcome(outcome);
                    break;
                }
                continue;
            }
            if let Some(action) = self.pause_menu.nav(intent) {
                self.pause_menu_action(action);
                break;
            }
        }
        // `MatchInputBindings`: watching, fire or the next weapon chases the
        // next player; jump lets go.
        if !matches!(self.feed, Some(Feed::Play(..))) {
            let next = [fps::FIRE, fps::WEAPON_NEXT, fps::WEAPON_3];
            if frame.pressed.iter().any(|a| next.contains(a)) {
                self.chase_next();
            } else if frame.pressed.contains(&fps::JUMP) && self.follow_id.is_some() {
                self.follow_id = None;
                self.free_camera = true;
            }
        }
        // `controllerLook`: the turn, slowed by the aim's share (`in_pad_ads`).
        if self.captured && frame.look != (0.0, 0.0) {
            let ads = self
                .surface
                .as_ref()
                .and_then(|s| s.renderer.view_model.as_ref())
                .map_or(0.0, |v| v.view.ads_progress);
            let share = with_cvars(|c| {
                if c.exists("in_pad_ads") {
                    c.get_number("in_pad_ads")
                } else {
                    0.5
                }
            });
            let scale = 1.0 - ads * (1.0 - if share > 0.0 { share } else { 0.5 });
            self.camera.yaw += (frame.look.0 * scale) as f32;
            let limit = std::f32::consts::FRAC_PI_2 - 0.01;
            self.camera.pitch = (self.camera.pitch + (frame.look.1 * scale) as f32).clamp(-limit, limit);
        }
    }

    pub(super) fn set_captured(&mut self, captured: bool) {
        let Some(s) = &self.surface else { return };
        let grab = if captured {
            // Without a grab the game still plays: raw mouse motion turns
            // the view and the cursor stays hidden.
            if let Err(e) = s
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| s.window.set_cursor_grab(CursorGrabMode::Confined))
            {
                println!("the mouse could not be captured ({e}); playing without the lock");
            }
            true
        } else {
            let _ = s.window.set_cursor_grab(CursorGrabMode::None);
            false
        };
        s.window.set_cursor_visible(!grab);
        // On macOS a locked cursor keeps its arrow without this.
        shagger::input::native_cursor::set_cursor_hidden(grab);
        if self.captured && !grab {
            // Pointer lock lost: nothing stays held (`releaseHeldInput`).
            self.held.clear();
            self.firing = false;
            with_input(InputSystem::release_held_input);
        }
        self.captured = grab;
    }

    /// Talk mode's keys (`FPSHud`'s chat key handler): T, Enter or Y
    /// opens it while playing, Shift or Y to the team; while open, every key types, Enter sends and Escape
    /// cancels. True when the key was the chat's.
    pub(super) fn chat_key(&mut self, code: KeyCode, event: &winit::event::KeyEvent) -> bool {
        // `FPSHud`'s handler stands aside while the match menu, its
        // settings or the loadout picker are up, and outside play.
        let menus_up = self.pause_menu.open || self.settings_open || self.loadout_picker.is_open();
        let in_play = self.in_play();
        let Some(Feed::Play(play, _)) = &self.feed else {
            return false;
        };
        let Some(hud) = self.surface.as_mut().and_then(|s| s.hud.as_mut()) else {
            return false;
        };
        let chat = &mut hud.chat;
        if !chat.is_input_active() {
            if menus_up || !in_play {
                return false;
            }
            let pressed = event.state == ElementState::Pressed && !event.repeat;
            // From the event's own modifiers, as `e.shiftKey` and the rest.
            if let Some(team) = pressed.then(|| talk_key(code, super::modifiers())).flatten() {
                chat.open_input(team);
                // Nothing stays held while typing.
                self.held.clear();
                self.firing = false;
                with_input(InputSystem::release_held_input);
                return true;
            }
            return false;
        }
        if event.state != ElementState::Pressed {
            return true;
        }
        match code {
            KeyCode::Enter | KeyCode::NumpadEnter => {
                if let Some((text, team_only)) = chat.submit() {
                    play.say(&text, team_only);
                }
                for command in chat.take_console_commands() {
                    self.console.run(&command);
                }
            }
            KeyCode::Escape => chat.close_input(),
            KeyCode::Backspace => chat.backspace(),
            _ => {
                if let Some(text) = &event.text {
                    chat.type_text(text);
                }
            }
        }
        true
    }

    /// Playing with a weapon that fires only aimed (`hipfireBlocked`: the
    /// launchers) in hand.
    pub(super) fn hipfire_blocked_in_hand(&self) -> bool {
        matches!(&self.feed, Some(Feed::Play(play, _))
            if play.weapon_in_hand().is_some_and(|w| weapon_hipfire_blocked(canonical_weapon_id(w))))
    }

    /// This frame's actions as a player's input, through the player's
    /// bindings (`FPSActions`).
    pub(super) fn play_input(&self) -> PlayInput {
        let mut input = with_input(|i| PlayInput {
            forward: i.is_active(fps::MOVE_FORWARD),
            backward: i.is_active(fps::MOVE_BACKWARD),
            left: i.is_active(fps::MOVE_LEFT),
            right: i.is_active(fps::MOVE_RIGHT),
            jump: i.is_active(fps::JUMP),
            crouch: i.is_active(fps::CROUCH),
            // `FPSController`: with cl_autorun (on by default) the sprint key walks.
            sprint: i.is_active(fps::SPRINT) != with_cvars(|c| c.get_bool("cl_autorun")),
            // The first click captures the mouse rather than firing.
            fire: self.captured && i.is_active(fps::FIRE),
            alt_fire: i.is_active(fps::AIM),
            reload: i.is_active(fps::RELOAD),
            use_key: i.is_active(fps::USE),
            yaw: f64::from(self.camera.yaw),
            // The slide's dip and the ledge lift aim too (`sendInput`).
            pitch: f64::from(self.camera.pitch) + self.first_person.aim_pitch,
            // Picks go through `LivePlay::select_weapon`.
            weapon_slot: 0,
            auto_ads: false,
            // `FPSController`: the move stick's own angle, holding the keys
            // it points at too.
            move_axis: stick_direction(self.pad_move),
            assisted_aim: self.device.gamepad(),
        });
        let keys = stick_keys(input.move_axis);
        input.forward |= keys.forward;
        input.backward |= keys.backward;
        input.left |= keys.left;
        input.right |= keys.right;
        // `hipfireBlocked`: the trigger on a launcher puts the aim up.
        if input.fire && !input.alt_fire && self.hipfire_blocked_in_hand() {
            input.alt_fire = true;
            input.auto_ads = true;
        }
        input
    }

    pub(super) fn step(&mut self, dt: f32) {
        if matches!(self.feed, Some(Feed::Play(..))) {
            // `FPSController.update`: without the pointer lock, no keys play.
            let input = if self.captured {
                self.play_input()
            } else {
                PlayInput {
                    yaw: f64::from(self.camera.yaw),
                    pitch: f64::from(self.camera.pitch) + self.first_person.aim_pitch,
                    ..PlayInput::default()
                }
            };
            // Weapon select: number keys and the wheel, pressed this frame.
            let pick = if self.captured {
                with_input(|i| {
                    (
                        weapon_slot_pressed(i),
                        i.was_pressed(fps::WEAPON_NEXT),
                        i.was_pressed(fps::WEAPON_PREV),
                    )
                })
            } else {
                (0, false, false)
            };
            // `FPSController.syncKeysFromInput`: the ready key, pressed this frame.
            let ready = self.captured && with_input(|i| i.was_pressed(fps::READY));
            // No pick while the weapon slides (`setWeaponSwitchBlockedCheck`).
            let switching = self
                .surface
                .as_ref()
                .is_some_and(|s| s.renderer.switch_anim.is_switching());
            if let Some(Feed::Play(play, held)) = &mut self.feed {
                *held = input;
                if let Some(id) = play.select_weapon(pick.0, pick.1, pick.2, switching) {
                    self.predicted_weapon = Some(id);
                }
                if ready {
                    play.send_ready();
                }
            }
            self.velocity = Vec3::ZERO;
            return;
        }
        let key = |k| self.held.contains(&k);
        let axis = |plus, minus| f32::from(u8::from(key(plus))) - f32::from(u8::from(key(minus)));
        let speed = if key(KeyCode::ShiftLeft) { SPEED * 3.0 } else { SPEED };
        // A move stick flies its own way, jump and crouch up and down.
        let pad_held = |action| {
            with_input(|i| controller_buttons(i, action))
                .iter()
                .any(|b| usize::try_from(*b).is_ok_and(|b| self.pad.down.contains(&b)))
        };
        let (stick, pad_up, pad_down) = if self.captured && self.device.gamepad() {
            (self.pad_move, pad_held(fps::JUMP), pad_held(fps::CROUCH))
        } else {
            (None, false, false)
        };
        let (forward, right) = stick.map_or_else(
            || (axis(KeyCode::KeyW, KeyCode::KeyS), axis(KeyCode::KeyD, KeyCode::KeyA)),
            |m| (m.forward as f32, m.right as f32),
        );
        let vertical =
            axis(KeyCode::Space, KeyCode::KeyC) + f32::from(u8::from(pad_up)) - f32::from(u8::from(pad_down));
        let movement = self.camera.forward() * forward + self.camera.right() * right + Vec3::Y * vertical;
        let velocity = movement.normalize_or_zero() * speed;
        self.camera.position += velocity * dt;
        self.velocity = velocity;
    }

    /// A whole demo's keys (`DemoPlaybackKeys` outside a reel): Space
    /// pauses, the arrows seek and change speed, comma and period step a
    /// frame, N and P jump between frags, Tab and the digits pick whose
    /// eyes to watch through. Flying free, Space and the arrows move.
    pub(super) fn demo_key(&mut self, code: KeyCode, repeat: bool) -> bool {
        let (None, Some(Feed::Demo(demo)), Some(hud)) = (&self.reel, &mut self.feed, &mut self.demo_hud) else {
            return false;
        };
        let held = |k| self.held.contains(&k);
        let modifiers = Modifiers {
            shift: held(KeyCode::ShiftLeft) || held(KeyCode::ShiftRight),
            alt: held(KeyCode::AltLeft) || held(KeyCode::AltRight),
            ctrl: held(KeyCode::ControlLeft) || held(KeyCode::ControlRight),
            meta: held(KeyCode::SuperLeft) || held(KeyCode::SuperRight),
        };
        let context = DemoKeyContext {
            reel: false,
            free_camera: self.free_camera,
        };
        let Some(action) = demo_key_action(&dom_code(code), modifiers, context) else {
            return false;
        };
        let repeats = matches!(action, DemoKeyAction::Seek { .. } | DemoKeyAction::Frame { .. });
        if repeat && !repeats {
            return true;
        }
        hud.wake();
        let rate = demo.tick_rate();
        let order = name_order(demo.roster(), &self.drawn);
        let players = order.len();
        match action {
            DemoKeyAction::TogglePause => demo.toggle_play_pause(),
            DemoKeyAction::Seek { seconds } => demo.seek_to_tick(demo.playhead_tick() + seconds * rate),
            DemoKeyAction::Frame { step } => {
                demo.pause();
                demo.seek_to_frame(demo.current_frame() as isize + isize::from(step));
            }
            DemoKeyAction::Speed { step } => {
                demo.step_speed(step);
            }
            DemoKeyAction::Frag { step } => {
                if let Some(tick) = frag_seek_tick(&hud.frag_ticks(), demo.playhead_tick(), step, rate) {
                    demo.seek_to_tick(tick);
                }
            }
            DemoKeyAction::Player { step } => {
                if players > 0 {
                    let current = self
                        .follow_id
                        .as_ref()
                        .and_then(|id| order.iter().position(|o| o == id));
                    let at = current.unwrap_or(if step > 0 { players - 1 } else { 0 });
                    let next = (at as isize + isize::from(step)).rem_euclid(players as isize) as usize;
                    self.follow_id = Some(order[next].clone());
                    self.free_camera = false;
                }
            }
            DemoKeyAction::PlayerAt { index } => {
                if let Some(id) = order.get(index) {
                    self.follow_id = Some(id.clone());
                    self.free_camera = false;
                }
            }
            DemoKeyAction::Shot { .. } => return false,
        }
        true
    }

    /// The reel's keys (`DemoPlaybackKeys` in reel mode): Space pauses,
    /// Left and Right step through the shots, R plays it again.
    pub(super) fn reel_key(&mut self, code: KeyCode) -> bool {
        let (Some(reel), Some(Feed::Demo(demo))) = (&mut self.reel, &mut self.feed) else {
            return false;
        };
        let shots = self.surface.as_mut().and_then(|s| s.renderer.shot_world_mut());
        let mut world = SceneReelWorld {
            shots,
            players: &self.drawn,
        };
        match code {
            KeyCode::Space => reel.toggle_play_pause(demo),
            KeyCode::ArrowRight => reel.next_segment(demo, &mut world),
            KeyCode::ArrowLeft => reel.prev_segment(demo, &mut world),
            KeyCode::KeyR => reel.replay(demo, &mut world),
            _ => return false,
        }
        true
    }

    /// What the console's commands asked for this frame.
    pub(super) fn serve_console(&mut self) {
        for request in std::mem::take(&mut self.console.ctx.requests) {
            match request {
                ConsoleRequest::Say { text, team } => {
                    if let Some(Feed::Play(p, _)) = &self.feed {
                        p.say(&text, team);
                    }
                }
                ConsoleRequest::CallPause(timeout) => {
                    if let Some(Feed::Play(p, _)) = &self.feed {
                        p.call_pause(timeout);
                    }
                }
                ConsoleRequest::Disconnect | ConsoleRequest::DemoStop => self.leave = true,
                ConsoleRequest::DemoPlay => {
                    if let Some(Feed::Demo(demo)) = &mut self.feed {
                        self.reel = None;
                        demo.set_speed(1.0);
                        demo.seek_to_frame(0);
                        demo.play();
                    }
                }
                ConsoleRequest::DemoCinematic { minutes } => {
                    if let Some(Feed::Demo(demo)) = &mut self.feed {
                        let mut reel = CinematicDemoPlayer::new(REEL_TITLE_MS);
                        reel.load_and_play(
                            demo,
                            &mut SceneReelWorld {
                                shots: self.surface.as_mut().and_then(|s| s.renderer.shot_world_mut()),
                                players: &self.drawn,
                            },
                            minutes,
                        );
                        self.follow = None;
                        self.reel = Some(reel);
                    }
                }
                ConsoleRequest::DemoAnalyze => {
                    if let Some(Feed::Demo(demo)) = &self.feed {
                        let analysis = analyze_demo(
                            demo,
                            &mut SceneReelWorld {
                                shots: self.surface.as_mut().and_then(|s| s.renderer.shot_world_mut()),
                                players: &self.drawn,
                            },
                        );
                        let report = analysis_report(&analysis, &demo.header.map, demo.duration_ms);
                        self.console.console.log(LogType::Result, &report);
                    }
                }
            }
        }
        if self.console.ctx.quit {
            self.leave = true;
        }
    }
}

/// `FPSHud`'s chat key: T (Q3 messagemode habit) or Enter (modern habit)
/// opens talk mode, Y or either with Shift held opens it to the team
/// (`Some(true)`); with Alt, Ctrl or the system key held it is not chat's.
fn talk_key(code: KeyCode, mods: winit::keyboard::ModifiersState) -> Option<bool> {
    if mods.alt_key() || mods.control_key() || mods.super_key() {
        return None;
    }
    match code {
        KeyCode::KeyY => Some(true),
        KeyCode::KeyT | KeyCode::Enter | KeyCode::NumpadEnter => Some(mods.shift_key()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::talk_key;
    use winit::keyboard::{KeyCode, ModifiersState};

    #[test]
    fn talk_mode_opens_to_everyone_or_with_shift_or_y_to_the_team() {
        let none = ModifiersState::empty();
        assert_eq!(talk_key(KeyCode::KeyT, none), Some(false));
        assert_eq!(talk_key(KeyCode::Enter, none), Some(false));
        assert_eq!(talk_key(KeyCode::NumpadEnter, none), Some(false));
        assert_eq!(talk_key(KeyCode::KeyY, none), Some(true));
        assert_eq!(talk_key(KeyCode::KeyT, ModifiersState::SHIFT), Some(true));
        assert_eq!(talk_key(KeyCode::Enter, ModifiersState::SHIFT), Some(true));
        assert_eq!(talk_key(KeyCode::KeyT, ModifiersState::CONTROL), None);
        assert_eq!(talk_key(KeyCode::Enter, ModifiersState::ALT), None);
        assert_eq!(talk_key(KeyCode::KeyR, none), None);
    }
}
