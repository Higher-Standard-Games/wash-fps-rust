# Native lobby and settings: plan

Status: steps 1 and 2 built, step 3's pages built (November 12, 2026), behind `ui_lobby native`
(`washoi --lobby native` once; the cvar persists). The defaults for the
questions in section 7: the welcome guide stays web-only until step 5, the
native lobby opens straight onto Play, and the web view goes at parity.

## 1. Why reverse the November 3 decision

On November 3 the lobby moved into the system web view
([WEBVIEW_LOBBY.md](WEBVIEW_LOBBY.md)), so the browser and native clients would
share one lobby. On November 4 the plain egui lobby was removed
(`1a60807`). Since then, the web view has caused a bug on every platform:

- **Linux, NVIDIA**: WebKitGTK's DMA-BUF renderer draws black. With it off,
  the CPU path repaints CSS filters on every frame, and the lobby ran at 4 fps.
  0.3.5 hides the blurs on that path. GNOME's MiniBrowser shows the same
  4 fps on this page, so this is WebKitGTK itself, not our embedding.
- **Linux, everywhere**: the AppImage needs WebKitGTK 4.1 on the system, and
  the web view needs X11, so Wayland desktops run the game on XWayland.
- **macOS**: the page's size did not follow the window in fullscreen on a
  notched MacBook (fixed in 0.3.4). Each screen change moved the game to
  another desktop, because winit closed the window when a screen's event
  loop ended (fixed by pumping the loop there; RUST.md).
- **Everywhere**: a second copy of the input, focus, fullscreen and cursor
  handling across the bridge; 150 to 300 MB for the web view's processes; and
  a second renderer to keep working on each OS.

The everything-in-a-match half of the client is already native and matches
the web look screen by screen: the loading screen, the Esc menu, the kit
picker, the HUD and the region and data screens. The removed lobby was
dropped partly for its plain look. This plan doesn't bring that look back:
it keeps only the old code's logic.

**What it costs.** There are two lobbies to keep in step for good, which was
the reason for November 3. A lobby change on the web reaches native players
only with a client release. The "update required" screen already handles
protocol skew.

## 2. What the page does today, and where each part goes

Web sources are wash-fps `client/` unless named otherwise. Line counts are
TS + CSS.

| Screen or job | Web source | Size | Native reuse |
|---|---|---|---|
| Rail: brand, nav keys, rooms list, player card, Profile / Settings / Console keys | `LobbyPanel.ts`, `LobbyRooms.ts`, `LobbyPanel.css` | 1,465 + 100 + 1,571 | rail, raised keys and twin layout from `match_loading_screen.rs`, shagger `ui/components/lobby_twin.rs`, `pause_menu::raised_key` |
| Play stage: mode tiles, tagline, Find, private lobby key | `LobbyPanel.ts` (`showPlayStage`) | (above) | new screen on the same kit |
| Quick play and queue | `LobbyPanel.ts`, SHAG `LobbyClient.ts` | — | shagger `lobby::lobby_client` (`join_queue`, `create_lobby`) |
| Room before a match: players, ready, map vote, bots, shuffle, teams | SHAG `engine/ui/components/MatchLobbyView.ts` | 1,081 | shagger `ui/components/match_lobby_view.rs` (513 lines, egui, logic done), restyled |
| Private lobbies, invites, deep links | `LobbyPanel.ts` (`showPrivateLobby`, `runDeepLinkJoin`) | — | `lobby_client` (`join_lobby` with invite token); links as a command-line argument |
| Lobby chat | `LobbyPanel.ts`, `ChatScroll.ts` | 43 | `lobby_client` (`chat_send`); text input from the in-match chat (`chat_key`) |
| Live matches: watch, drop in | `LobbyPanel.ts` | — | `lobby_client`; hands off through `LobbyOutcome::Match`, as now |
| Connecting, failure | SHAG `ConnectingOverlay.ts`, `MatchConnectionFailure.ts` | — | shagger `ui/components/connecting_overlay.rs` (555 lines) |
| Login, account key, first name | SHAG `lobby/AccountSession.ts`, `IdentityFields.ts`, `IdentityFrame.ts` | 315 + 232 + 746 css | `lobby_client::auth`, the key file the web view already mirrors |
| Profile: key download and copy, rename, delete account | `ProfileView.ts` | 835 | removed `profile_view.rs` (103 lines) for the requests; key as a file in the config folder plus clipboard |
| Leaderboard: boards, scopes | `LeaderboardView.ts`, `Leaderboard.css` | 510 + 544 | `lobby_client::request_leaderboard` and `request_stats`; removed `leaderboard_view.rs` (186 lines) |
| Demos: list, featured, download, play | `DemosView.ts`, `Demos.css` | 1,026 + 991 | `client/demo_service.rs`, `demo_download.rs` (already native); removed `demos_view.rs` (793 lines) for the list logic |
| Settings: Gameplay, Controls, Controller, Video, Audio | `settings/*.ts`, `FPSSettingsView.css` | about 3,100 + 2,115 | `client/settings/settings_model.rs` (1,285 lines, every cvar and its page), `controller_model.rs`; removed `fps_settings_view.rs`, `controls_page.rs` (key capture) |
| Settings sync with the account | SHAG `lobby/SettingsSync.ts` | 274 | shagger `lobby::settings_sync` (498 lines, ported); it runs natively instead of in the page |
| Welcome and terms | `LobbyWelcomeDialog.ts`, `LobbyWelcomeGuide.ts` | 462 + 458 css | removed `lobby_welcome_*.rs` for the flow; terms text in `assets/docs` |
| Credits | `credits/CreditsDialog.ts`, `MarkdownLite.ts` | 289 | removed `credits/markdown_lite.rs` (431 lines) |
| Lobby music, its mute key | `LobbyMusicKey.ts` | 59 | shagger audio |
| Full-server notice | `LobbyFullNotice.ts` | 132 | small new screen |
| Map packages before a match | `LobbyAssetPreparation.ts`, `LobbyDownloadIndicator.ts` | 210 | already native (`data_sync_screen.rs`, the package store) |
| Update notice, Quit key, client version | `native/ShellControls.ts` | — | the update check is native already (`update_check.rs`) |
| Console key | page opens the console | — | the native console |

Everything in a match, including demo playback and the Esc menu, stays as it
is. The in-match Settings key opens the native settings instead of the page.

**Out of scope**: features beyond the lobby's layout and core flows, for
example the demo viewer's video export. The native lobby ports the layout and
what a player needs to get into matches, settings, boards and demos. Anything
past that is added later, piece by piece, if it's wanted.

## 3. Shape

- **One screen module per area** in wash-fps-rust, named after the web file
  (`lobby_panel.rs`, `leaderboard_view.rs`, `demos_view.rs`, `profile_view.rs`,
  `settings/fps_settings_view.rs` and its pages), each naming its TypeScript
  source at the top, as with every port.
- **Engine pieces go to shagger** (RUST.md 14.1): the raised key, the rail, a
  scroll list, a text field, toggles, sliders, the key-capture field and the
  twin layout. Most of these exist in pieces in `pause_menu.rs`,
  `match_loading_screen.rs` and `lobby_twin.rs`. They become one widget set,
  `shagger::ui::components`.
- **The lobby runs as a screen on the event loop**, like the region and data
  screens, and ends in the same `LobbyOutcome` (match, demo, quit). `main.rs`'s
  loop between lobby and match doesn't change.
- **The lobby connection lives for the session.** It stays open through
  matches, so the room, chat and queue survive the round trip as they do in
  the browser.
- **A switch while both exist**: cvar `ui_lobby native|web`, default `web`
  until the native lobby covers section 2, then `native`. The web view is
  removed one release after that, along with WebKitGTK from the Linux build.

## 4. Look

Each screen is built against the web lobby in a browser at the same size,
1280×720 and 1920×1080 (Playwright screenshots of `eu.washoi.net/client`),
compared side by side, as the loading screen was. The blurs and glows that
cost WebKitGTK so much are cheap here: one blurred texture, drawn once.

## 5. Order

The work is done in pieces: each step lands on main behind the switch, with
tests, and can ship.

1. **Shell and play.** Widget set in shagger; rail and nav; play stage with
   mode tiles, Find and quick play; login with the key; lobby chat; rooms
   list with watch and drop-in; handoff to the match and back. With this
   step, a player can live in the native lobby. *Built:* shagger's
   `ui/components/raised_key.rs` and `text_field.rs` (the Esc menu now
   draws its keys with them too), `client/lobby_panel.rs` and
   `lobby_rooms.rs`, and `src/lobby_screen.rs`. The other tabs offer the web
   lobby until their step lands. `washoi menu <lobby> --screenshot F --play`
   presses Quick play headless and joins the match it is handed.
2. **Rooms.** The room view (restyle `match_lobby_view.rs`), private lobbies,
   invites, bots, the full-server notice, deep links. *Built:* the room as
   the web draws it (status radar, roster in VS, team and FFA layouts, map
   vote with levelshots, Leave, Add bot, Switch, Shuffle, Rejoin, Ready),
   room chat in the rail (`client/room_chat_view.rs`), seat reissue for
   Rejoin, `client/levelshots.rs` shared with the loading screen, the
   outlined ghost word (shagger `ui/components/outline_text.rs`), and the
   foot's version, Devlog and Quit keys. `washoi menu ws://127.0.0.1:9
   --screenshot F --room room.json` draws a room from its saved state.
   Deep links and the full-server notice move to step 5.
3. **Settings.** All five pages on `settings_model.rs`, key capture, the
   controller page, native settings sync, and the in-match Settings key.
   This is the biggest single step. *Built:* `client/settings/`
   `fps_settings_view.rs` (head, armed resets, toast, page keys in the
   rail), `settings_fields.rs` (card, slider, row of keys, swatches,
   drop-down, notice, cost pips) and a module per page as in the browser.
   Keys are captured from egui's events under the browser's code names.
   The Controller page reads the pads itself while shown; the browser's
   Use controller keys are left out, since the native device follows the
   last input. Each side's 3D figure on Gameplay is its name in the side's
   outline for now. `washoi menu ... --page settings/video/effects` opens a
   page for screenshots. Settings sync was already native (step 1). Still
   to do: the in-match Settings key, which opens the web page today.
4. **Profile, leaderboard, demos.**
5. **The rest and the switch.** Welcome and terms, credits, music, the
   update notice. A parity review against section 2, then the default flips
   to `native`.

As a rough size, steps 1 and 3 are each about the size of the kit picker plus
the Esc menu several times over. Steps 2, 4 and 5 are smaller. Each step
comes back for your review in the game before the next one starts.

## 6. Testing

- Screens render headless to PNG (as `washoi loading --screenshot` does now)
  for side-by-side checks against the browser.
- Lobby flows run against the local stack (lobby, bot server) with a scratch
  data directory: login, quick play, a private room with a bot, a match and
  back, a settings round trip through the account.
- The CI jobs that build all four platforms are unchanged.

## 7. Questions for review

1. Should the web lobby keep features the native one leaves out for now? For
   example, it could ship without the welcome guide until step 5.
2. Should the native lobby open straight onto Play, skipping the boot screen
   the page shows? The native loading and data screens already cover start-up.
3. Is it fine to drop the web view entirely once native reaches parity,
   including the WebKitGTK dependency on Linux?
