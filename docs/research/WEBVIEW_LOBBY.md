# The webview lobby: how the native client and the web lobby share one UI

Status: spec, November 3, 2026. Nothing here is built yet. It follows the
decision in RUST.md 23: after 0.1.1 the native menu becomes the system's own
webview showing the washoi.net lobby, with no bundled Chromium and no native
lobby kept as a fallback.

The idea in one paragraph: the native client opens a webview on the region's
lobby page, and that page is the same lobby code players use in a browser. The
page keeps its own lobby connection, account and chat, exactly as in a
browser. When the page would start a match, a demo or anything else that
needs the 3D renderer, it calls the native client through a small message
bridge instead. The native client hides the webview, plays, and shows the
webview again when the player goes back to the lobby. Settings and the
account key live in native files, and the page sees them through its local
storage, which the bridge keeps in step with those files.

Contents:

1. What the web lobby does today
2. Architecture
3. The window, the webview and the event loop
4. Loading the page: which URL, and a lobby-only entry
5. The bridge: transport, handshake, messages
6. Match handoff: play, watch, drop in, seat reissue, return
7. Accounts and the key
8. Settings
9. Links, navigation, downloads and the clipboard
10. Demos, game data, music, console, fullscreen, quit
11. Sharing the layout: one lobby, two shells
12. Version skew between the web and the native client
13. Security
14. Errors and offline
15. Platforms and packaging
16. What retires on the native side
17. Testing
18. Work plan by repository
19. Decisions to confirm

---

## 1. What the web lobby does today

The lobby is part of the game's web app (wash-fps `client/main.ts`), served at
each region's `playUrl` (`https://eu.washoi.net/`, `https://us.washoi.net/`,
from washoi.net `src/data/regions.ts` and `/regions.json`). `main.ts` starts
the Babylon renderer and the game first, then builds `LobbyPanel` behind the
start-up screen. Everything the native shell has to deal with goes through a
handful of places:

| Concern | Where in the web code | Stored in |
|---|---|---|
| Lobby connection and login | `FpsLobbyService.ts` (`ensureFpsLobbyConnection`), SHAG `src/lobby/LobbyClient.ts`, `AccountSession.ts` | — |
| Account key | `AccountSession.ts` | `localStorage["shag_player_key"]`, `["shag_player_key_unknown"]` |
| Settings sync with the account | SHAG `src/lobby/SettingsSync.ts`, `shared/settings/SyncedSettings.ts` | `engine_cvars`, `engine_binds`, `shag_keybindings`, `hud-user-prefs`, `ui_theme_mode`, `shag.loadout.*`; state in `shag_settings_sync`, backup in `shag_settings_backup` |
| Device-only settings | `Cvar.ts`, `RenderVars.ts`, `MovementVars.ts` | `engine_cvars` (non-synced names), `engine.renderVars`, `engine.movementVars`, `quality_preference` |
| Lobby-only preferences | `LobbyWelcomeDialog.ts`, `LobbyMusicKey.ts`, `LeaderboardView.ts` | `washoi.welcome.hideAutomatically`, `washoi.terms.agreed`, `washoi.lobby.musicMuted`, `shag.leaderboard.scope` |
| Starting a match | `MatchLobbyView.ts` on `lobby:connect` → `ConnectingOverlay.show(serverUrl, spectator, ticket, recoverTicket, requestSeat)` → the registered `NetworkConnector` (SHAG `engine/ui/components/NetworkConnector.ts`) | — |
| Watching, dropping in | `LobbyPanel.ts` room rows → `ConnectingOverlay`, `requestDropIn` | — |
| Seat reissue | `LobbyClient.requestSeatReissue(matchId)` | — |
| Back to the lobby | `FPSHud.ts` → `ReturnToLobby.ts`: navigate to the origin | `sessionStorage["shag.return_lobby"]` |
| Deep links | `LobbyPanel.runDeepLinkJoin`: `/lobby/<id>`, `?lobby=`, `?invite=` | `sessionStorage["shag.private_invite.<id>"]` |
| Demos | `DemosView.ts` → `DemoService.fetchAndLoad` (Babylon) | — |
| Map packages | `LobbyAssetPreparation.ts`, `LobbyDownloadIndicator.ts` | CacheStorage, `pk3_index` |
| External links | `LobbyPanel` devlog link (`https://washoi.net/devlog`), credits (`MarkdownLite.ts`) | — |
| Files out | `ProfileView.ts` key download (`washoi-key-<name>.txt`), clipboard copy of the key | — |
| Reloads | `ProfileView` after deleting the account, `MatchConnectionFailure.ts`, `FPSHud.ts`, the stale build notice | — |

There is already one native shell around this code: `CapacitorBridge.ts`
(SHAG `engine/core`) for Android and iOS. It shows the pattern to follow: one
module that knows whether it runs in a shell, and code that asks it instead of
assuming a browser.

The single most useful fact: every match the lobby starts goes through
`NetworkConnector`. The lobby UI already does not know how the game connects.
A native connector that forwards to the bridge covers play, watch, drop in and
the ticket recovery without touching the lobby views.

## 2. Architecture

```
┌──────────────────────── native process (washoi) ───────────────────────┐
│                                                                        │
│  one winit window                                                      │
│  ┌──────────────────────────────┐    ┌──────────────────────────────┐  │
│  │ webview (wry, child)         │    │ wgpu surface                 │  │
│  │  lobby page from playUrl     │    │  match, demo, end screen,    │  │
│  │  LobbyClient ── wss lobby    │    │  native console, settings    │  │
│  │  NativeBridge.ts             │    │                              │  │
│  └──────────────┬───────────────┘    └──────────────▲───────────────┘  │
│                 │ window.ipc.postMessage(json)       │                  │
│                 ▼                                    │                  │
│        shagger webview host  ──►  washoi bridge handlers ──► app state  │
│                 ▲                                    │                  │
│                 └──── evaluate_script(receive(json)) ┘                  │
│                                                                        │
│  native files: lobby-key, cvars.json, synced-storage.json (config dir) │
└────────────────────────────────────────────────────────────────────────┘
```

Who owns what:

- **The page owns the lobby.** The lobby WebSocket, login, rooms, queues,
  chat, leaderboard, profile and settings sync with the account all stay in
  the web code and run unchanged. The native Rust lobby client stops being
  used by the menu.
- **The native client owns everything 3D, plus the device.** That covers
  matches, demos, the end screen, the window, fullscreen, game data, and the
  files on disk.
- **The bridge carries intents, not state.** The page says "play this match
  with this ticket", and the native client answers "the match ended, here is
  the room". Neither side mirrors the other's model, except the small set of
  storage keys in section 8.

The other way round, where Rust owns the lobby connection and the page is only
a view, was rejected. It would mean every lobby message crosses the bridge,
the web lobby would need a second mode for its whole data layer, and every
lobby feature would be built twice again.

## 3. The window, the webview and the event loop

**One window for the session.** Today the region screen, the data sync
screen, the menu and each match take turns on the event loop
(`window::run_on_event_loop`, `run_app_on_demand`). The webview has to
survive a match, because the page holds the lobby connection. So the client
becomes one app with one window and a state machine:

```
Region → DataSync → Lobby (webview shown) ⇄ InGame (webview hidden)
                         └─────────────── RegionChange ───┘
```

- **The webview** is built with `WebViewBuilder::build_as_child(&window)` and
  sized to the window's inner size on every `Resized`.
  - In the lobby, the webview is visible and the wgpu surface draws nothing,
    or a still background so a resize never flashes garbage.
  - In a match, the client calls `webview.set_visible(false)` and
    `webview.focus_parent()`, and the renderer runs as today.
- **Focus.** The webview takes keyboard focus while it is visible. On the way
  into a match the client hands focus back with `focus_parent()` and grabs
  the cursor. On the way out it releases the cursor before showing the
  webview again.
- **Hidden is not destroyed.** While hidden, the page keeps running: the lobby
  socket stays up, invites and chat keep arriving, and the room stays
  attached. This matches the browser, where the lobby page lives under the
  canvas for the whole match.
  - WebView2 and WebKit throttle timers on hidden pages. W0 measured it
    (section 18.1): left alone, WebView2 slows a hidden page's timers to
    once a minute after five minutes.
  - So the webview is built with throttling off: Chromium's switches on
    Windows, wry's `BackgroundThrottlingPolicy::Disabled` on macOS. With
    that, the hidden page's timers and socket kept going for 10 minutes on
    every platform, and the `tick` event is not needed.
- **Linux.** On Linux, `build_as_child` works on X11 only and needs
  `gtk::init()`, with `gtk::main_iteration_do(false)` pumped in
  `about_to_wait`. On Wayland the choices are:
  - (a) run winit on XWayland while the webview is in use (`WAYLAND_DISPLAY`
    unset for the process, or winit's X11 backend forced);
  - (b) `build_gtk` into a separate GTK window that the client shows and
    hides instead of the child.

  Decided: (a) for 0.2.0. Wayland is the default session on current
  Ubuntu, Fedora, GNOME and KDE Plasma 6, so it is common, but those
  desktops all ship XWayland and run X11 apps without setup. The costs are
  small: a blurrier window under fractional scaling, and no native Wayland
  window features. (b) is the fix if a player hits a desktop without
  XWayland or the scaling bothers people.
- **macOS.** The webview is an `NSView` subview of the window's content view,
  beside wgpu's `CAMetalLayer`. Hiding the subview is enough.
- **Windows.** WebView2 is a child HWND. Its user data folder must be set
  (`WebContext::new(Some(cache_dir/"webview"))`), otherwise WebView2 tries
  to write beside the executable.

## 4. Loading the page: which URL, and a lobby-only entry

The browser page boots the whole game: Babylon, the renderer, the camera, the
player, audio preloads. In the native shell all of that is dead weight, and
some of it, such as WebGPU initialisation, can fail inside a webview for
reasons that have nothing to do with the lobby.

**Recommendation: a lobby-only entry, built and deployed with the web app.**

- A second Vite entry: `client.html` → `client/native/main.ts`.
  - It runs the lobby part of `main.ts` (`configureLobbyTwin`,
    `LoadingScreen`, `LobbyPanel`).
  - It registers the client cvars and the settings pages.
  - It never imports `Game`, Babylon or `DemoService`.
- Served at `<playUrl>client` (for example `https://eu.washoi.net/client`),
  from the same deploy, so it is always the same build as the browser lobby.
  The browser lobby's query deep links work there:
  `/client?lobby=<id>&invite=<token>`. Not `/lobby/client`: `/lobby/<id>`
  is the room deep link, so a room named `client` would collide.
- The import graph has been checked: `LobbyPanel` itself pulls no Babylon.
  `DemoService` does (`Ray`, `Vector3`, `Scene`, `CameraSystem`), so in
  native mode `DemosView` must go through the bridge (section 10).
  `MatchLobbyView`, `GamePanel`, `DebugConsole`, `UIStyles`,
  `FPSSettingsView`, `ProfileView`, `LobbyAssetPreparation` and
  `LeaderboardView` are clean.
- A lint rule or test keeps it that way: build `client.html` and fail if the
  chunk graph contains `@babylonjs`.

The alternative is the same `index.html` with a "native" flag that skips the
renderer. It needs no new route, but it puts `if (native)` checks through
`main.ts` and loads the game's main chunk anyway. Not recommended.

The native client builds the URL from the region list: `playUrl` plus
`client`. `regions.json` gets an optional `clientUrl`, so the path can move
without a client release.

## 5. The bridge: transport, handshake, messages

**Transport.** wry gives one string channel each way:

- page → native: `window.ipc.postMessage(text)`, received in
  `with_ipc_handler(|req: Request<String>|)`;
- native → page: `webview.evaluate_script("window.__washoiNative.receive(<json>)")`.

All messages are JSON objects:

```ts
// page → native
{ "v": 1, "id"?: string, "type": "match:play", ...payload }
// native → page: a reply carries the request's id; an event has none
{ "v": 1, "id"?: string, "type": "match:ended", ...payload }
{ "v": 1, "id": "...", "type": "error", "code": "busy", "message": "..." }
```

`id` is present on requests that want an answer. The page side wraps this in
a promise API, with a timeout per request.

**Injected before the page runs.** `with_initialization_script` (main frame
only) defines `window.__washoiNative`:

```js
window.__washoiNative = {
  shell: "washoi-native",
  bridgeVersion: 1,
  clientVersion: "0.2.0",
  platform: "windows" | "macos" | "linux",
  capabilities: ["match", "demo", "storage", "external", "file-save", ...],
  storage: { /* the mirrored keys, section 8 */ },
  receive(msg) { /* replaced by NativeBridge.ts when it loads */ },
};
```

It also marks the document before first paint, so CSS can switch without a
flash: `document.documentElement.dataset.shell = "native"`.

**Page side:** `client/native/NativeBridge.ts` in wash-fps.

- `NativeBridge.isNative()` and `NativeBridge.has(capability)`.
- `request(type, payload)` returns a promise, and `on(type, handler)`
  listens for events.
- In a browser `isNative()` is false and nothing changes, the same way
  `CapacitorBridge.isNative()` works.
- The message types live in one TypeScript file, mirrored by a serde enum in
  Rust. Shared JSON fixtures prove both sides agree (section 17).

**Messages, version 1:**

| Direction | Type | Payload | Answer |
|---|---|---|---|
| page → native | `hello` | `{ webVersion, bridgeVersion }` | `{ clientVersion, bridgeVersion, capabilities }` |
| page → native | `match:play` | `{ matchId, serverUrl, map, gametype, admissionTicket, team?, lobbyId? }` | `ok` once connected, or `error` (`connect_failed`, `invalid_ticket`, `missing_data`) |
| page → native | `match:watch` | `{ serverUrl, map?, matchId?, lobbyId? }` | `ok` / `error` |
| native → page | `match:seat_wanted` | `{ matchId }`: a watcher pressed Join in game | — (the page calls `requestDropIn`, and the ticket comes back as `match:play`) |
| native → page | `match:ticket_needed` | `{ matchId }`: the server refused the ticket | page answers with `match:ticket { admissionTicket }` from `requestSeatReissue` |
| native → page | `match:started` | `{ matchId }` | — |
| native → page | `match:ended` | `{ matchId, lobbyId?, reason: "returned" \| "left" \| "disconnected" \| "kicked", message? }` | — |
| page → native | `demo:play` | `{ filename, url, map, reel? }` | `ok` when playback starts, `error` |
| native → page | `demo:ended` | `{}` | — |
| page → native | `data:status` | `{ maps: string[] }` | `{ ready: { [map]: boolean } }` |
| page → native | `storage:set` | `{ key, value \| null }` | — |
| native → page | `storage:changed` | `{ entries: { [key]: string \| null } }` | — |
| page → native | `cvars:describe` | `{ page: "video" }` | `{ fields: [...] }` (section 8) |
| page → native | `cvar:set` | `{ name, value }` | — |
| page → native | `open:external` | `{ url }` | — |
| page → native | `file:save` | `{ suggestedName, mime, text }` | `{ saved: boolean }` |
| page → native | `clipboard:write` | `{ text }` | — |
| page → native | `console:exec` | `{ line }` | `{ output: string[] }` |
| page → native | `window:fullscreen` | `{ on?: boolean }` (toggle when absent) | — |
| page → native | `region:choose` | `{}` | — (the native region screen opens) |
| page → native | `app:quit` | `{}` | — |
| native → page | `native:update` | `{ version, url }` (from `update_check`) | — |
| native → page | `tick` | `{}` (only while hidden, only if W0 needs it) | — |

`bridgeVersion` only moves for a change that breaks something. New messages
are added under new capabilities (section 12).

## 6. Match handoff: play, watch, drop in, seat reissue, return

**Play.** `MatchLobbyView` receives `lobby:connect` and calls
`ConnectingOverlay.show(serverUrl, false, ticket, recoverTicket)`, which calls
the registered `NetworkConnector`.

- In native mode, `client/native/main.ts` registers a
  `NativeNetworkConnector`.
- Its `connect(serverUrl, { admissionTicket, watchOnly, requestSeat })` sends
  `match:play` or `match:watch`. It resolves when the native client answers
  `ok`, and rejects with the native error text.
- The overlay's existing retry and "invalid match ticket" recovery then work
  as they do now: the error string carries `invalid match ticket` when the
  server refused the ticket.
- `isConnected()` is true between `match:started` and `match:ended`.
- `claimSeat(serverUrl, ticket)` sends `match:play` with the same server, and
  the native client takes the seat on the connection it already has
  (`washoi watch` → join), as the browser does.

The native side, on `match:play`:

1. Checks data for the map (it is normally all there after the data sync
   screen; see section 10).
2. Hides the webview, runs the existing `view::start` path with
   `watch = (serverUrl, name, ticket)` and `play = true`, and sends
   `match:started`.
3. If the game server refuses the ticket, sends `match:ticket_needed`. It
   waits up to 10 s for `match:ticket`, then gives up with
   `match:ended { reason: "disconnected" }`.

**Watch and drop in.** `match:watch` is the same without a ticket. A watcher
pressing Join in game sends `match:seat_wanted`. The page runs
`requestDropIn(matchId)`, the lobby sends `lobby:connect`, and that arrives
back as `match:play` for the server already being watched.

**Return.** The native end screen's "Back to the lobby" (wash-fps-rust
`cedd6aa`), leaving with Esc, a kick or a dropped connection all end in
`match:ended`.

1. The native client releases the cursor, shows the webview and focuses it.
2. Then it sends `match:ended`, so the page acts on it while visible.
3. The page does what the browser's `returnToLobby()` does: it clears
   `shag.return_lobby` and reloads the lobby entry. A reload is the
   simplest exact match for the browser. In a browser the single-match flow
   navigates to the origin, which reconnects and logs in again, and the
   lobby has already reset the room.

   A later improvement, once it is proven safe, can skip the reload and just
   close the room view.

**Who is the player?** The name and team the native client sends to the game
server come from the `match:play` payload and the page's identity, never from
a separate native login.

## 7. Accounts and the key

The page keeps the key exactly where the browser does:
`localStorage["shag_player_key"]`. The native client keeps the master copy in
its config dir (`lobby-key`, as today), so a wiped webview profile or a
reinstall does not lose the account:

- **At start**, the init script's `storage` snapshot carries `shag_player_key`
  from the file. `NativeBridge.ts` writes it to local storage before
  `AccountSession` reads it, but only when local storage has none or a
  different one. The file wins, because the file is what the 0.1.x native
  menu and `washoi lobby` used.
- **On every change**, the storage shim (section 8) sends `storage:set` for
  `shag_player_key`, and the native client writes `lobby-key` atomically.
  A change is a new account from `welcome`, a restored key, or a deleted
  account (`null`, which removes the file).
- `shag_player_key_unknown` is mirrored the same way, so "try restoring the
  old key" still works after a webview wipe.
- The 0.1.x native players keep their accounts: their `lobby-key` file is
  injected on the first webview start.
- The key never goes into a URL, a log line or a crash report on either side.

## 8. Settings

**The rule:** native files are the store, and the page's local storage is a
mirror of the keys the native client also uses. Web code keeps reading and
writing local storage as it does now. That includes `Cvar.ts`, the binding
classes, `SettingsSync` and the loadout picker. The account sync therefore
keeps working without a single change.

**Mirrored keys:**

| Local storage key | Native file | Notes |
|---|---|---|
| `engine_cvars` | `cvars.json` | the same JSON object; native-only cvars are kept by the web side because `Cvar.loadFromStorage` preloads unknown names and `saveToStorage` writes them back |
| `engine_binds`, `shag_keybindings`, `hud-user-prefs`, `ui_theme_mode`, `shag.loadout.*` | `synced-storage.json` (one entry per key, as shagger `lobby/settings_sync.rs` already does) | bindings are W3C `KeyboardEvent.code` strings on both sides |
| `shag_settings_sync`, `shag_settings_backup` | `settings-sync.json`, `settings-backup.json` | so the sync decision on login sees the native edits |
| `shag_player_key`, `shag_player_key_unknown` | `lobby-key`, `lobby-key.unknown` | section 7 |

Not mirrored, so they stay in the webview profile only:

- the lobby-only preferences (welcome, terms, music muted, leaderboard scope);
- the browser renderer's `engine.renderVars` and `quality_preference`;
- `pk3_index`.

**Page → native.** `NativeBridge.ts` wraps `Storage.prototype.setItem` and
`removeItem` for the mirrored keys only. Each write still lands in local
storage, then posts `storage:set`. The native client debounces by 200 ms and
writes atomically (`shagger::core::atomic_file`). It then applies the change
to its live cvars and bindings, so a setting changed in the lobby is in force
for the next match without a restart.

**Native → page.** Settings can change while the webview is hidden: the
native console, the in-game settings overlay, or Alt+Enter remembering
fullscreen.

- On `match:ended` the native client first sends `storage:changed` with every
  mirrored key that changed since the match started.
- `NativeBridge.ts` writes those keys and calls a new
  `Cvar.reloadFromStorage()` and `CommandBindings.reload()` (SHAG, small).
- The page's `SettingsSync` then notices the edit on its next five-second
  check and uploads it as for any local edit.
- Because the return reloads the page anyway (section 6), the reload hooks
  matter only for a later no-reload return. They are still worth adding,
  since they make the order of events irrelevant.

**Only one sync to the account.** shagger's `SettingsSync` port runs only in
`washoi lobby` and other command line tools. In the webview shell the page's
`SettingsSync` is the only one talking to the lobby about settings. Two would
race on `settings:save`.

**Same name, same meaning.** A cvar both sides register must have the same
type, default and range. Otherwise the web side could clamp a native value,
or drop it for being equal to the web default. A golden test compares the
web's client cvar registry with the native one (section 17).

**The settings screen.** The lobby's settings button opens the web
`FPSSettingsView`, as in a browser.

- **Controls, Gameplay and Audio** edit cvars and bindings both sides share,
  and they flow through the mirror as above.
- **Video** is the exception: the browser's Video page drives Babylon's
  render variables, which mean nothing natively.
  - In native mode the page builds Video from `cvars:describe { page:
    "video" }`. That message returns the native renderer's fields as data:
    `{ name, label, kind: "toggle" | "slider" | "choice", min, max, step,
    options, value, help }`.
  - Changes go back as `cvar:set`.
  - The page draws these fields with the same `SettingsFields.ts`
    controls and layout as every other page, so the whole settings UI stays
    the web one, shared with the browser. Only the list of Video fields
    comes from the native client. That keeps one settings layout, with fields
    the native renderer actually has, and no web release when the native
    client adds a video setting.
- **During a match**, the native in-game settings (wash-fps-rust
  `client/settings`, egui) stay. They edit the same cvars and the same files.

## 9. Links, navigation, downloads and the clipboard

**Navigation policy** (`with_navigation_handler`, `with_new_window_req_handler`):

| Target | What happens |
|---|---|
| The region's own origin (`https://eu.washoi.net/…`), and in debug builds `http://localhost:*` | navigates inside the webview |
| Another region's origin, from `regions.json` | allowed only through `region:choose` (the native region screen), never by a stray link |
| Anything else (`https://washoi.net/devlog`, credits links, GitHub, Discord) | refused in the webview and opened in the system browser (`open:external` or the navigation handler itself) |
| `target=_blank`, `window.open` | same as "anything else" |
| `file:`, `data:`, `javascript:` top-level navigations | refused |

**The links that need a native behaviour:**

| Link or action today | In the native shell |
|---|---|
| Devlog, credits, site links | system browser |
| "Back to the lobby" (`ReturnToLobby.ts`, navigates to the origin) | `match:ended` → the page reloads `/client` (section 6) |
| Room and invite links the player copies to share (`/lobby/<id>?invite=…`) | stay `https://<region>/lobby/<id>?invite=…`, so a friend in a browser can open them; the page builds them from `location.origin` minus the `/client` path |
| Opening an invite someone sent to a native player | paste into the lobby (a small "Join by link" field in native mode, which runs the same `runDeepLinkJoin`); a `washoi://` URL scheme registered by the installer is a later step |
| `location.reload()` (account deleted, connection failure, stale build) | allowed, since it reloads `/client` |
| Key download (`link.download`, blob URL) | `file:save` → the system save dialog (`rfd`), defaulting to `washoi-key-<name>.txt` in Documents |
| Timedemo report download | not offered in native mode (the native client has `washoi bench`) |
| `navigator.clipboard.writeText` | works in WebView2 and WKWebView on a secure origin; `clipboard:write` is the fallback when it throws (WebKitGTK) |
| Browser recommendation banners (washoi.net `BrowserRecommendation`, any "use Chrome" notice) | hidden in native mode |

wry's `with_download_started_handler` refuses every download the page did not
ask for through `file:save`.

## 10. Demos, game data, music, console, fullscreen, quit

- **Demos.**
  - `DemosView`'s play button calls `DemoService.fetchAndLoad`, which needs
    Babylon. In native mode it sends `demo:play` instead.
  - The native client downloads the demo (`client/demo_download.rs`), plays
    it, and sends `demo:ended` when the player leaves playback. The page
    then shows the demos tab again, which is what `setExitHandler` does now.
  - Reels pass `reel: minutes`, as `LobbyChoice.reel` does today.
- **Game data.**
  - The native data sync screen runs before the webview and brings every
    package up to date. So in native mode `LobbyAssetPreparation` and the
    download indicator are switched off.
  - Where the page gates on assets ("is this map ready?"), it asks
    `data:status`.
  - If a match starts on a map the native client lacks (a map published
    after the client started), `match:play` answers `missing_data`. The page
    shows its usual "downloading" state, and the native client fetches the
    package first.
- **Music.**
  - The lobby playlist plays in the page, through its own Web Audio, as in a
    browser. It honours the mirrored `s_volume` and `s_musicenabled`, and the
    page-only "music muted" key.
  - `match:started` pauses it and `match:ended` resumes it. In game, the
    native client plays the game and endgame playlists.
  - Lobby music never plays natively, so the two never overlap.
- **Console.**
  - In the lobby, `~` opens the web `DebugConsole`.
  - In native mode, lines it does not know go to `console:exec`, and the
    native output is printed back. So `vid_restart`, `quit`, native `r_`
    cvars and `bind` all work from the lobby.
  - In game, the native console stays as it is.
  - This is the "console for anything offline" from RUST.md 23: when the
    page cannot load, the native error screen (section 14) has the console
    too.
- **Fullscreen.**
  - The webview has keyboard focus in the lobby, so the native Alt+Enter
    handler never sees the key there. The page catches Alt+Enter and F11 in
    native mode and sends `window:fullscreen`.
  - The Video page's fullscreen toggle sends the same message.
  - The native window remembers the state in a cvar, so the match opens the
    same way.
- **Quit.** A browser has no quit button. Native mode adds a "Quit" key
  beside "Credits", which sends `app:quit`. Closing the window works as
  always.
- **Updates.** The native update check (`update_check.rs`, from
  `washoi.net/client/latest.json`) sends `native:update`, and the page shows
  it in its own notice style.

## 11. Sharing the layout: one lobby, two shells

The goal is that the lobby a browser player sees and the lobby a native
player sees are the same pages, from the same deploy, with the same CSS. The
shell only adds, hides or reroutes a few controls.

- **One switch for CSS:** `html[data-shell="native"]`, set by the init script
  before first paint. Styles that differ are written against it in the
  existing stylesheets (`LobbyPanel.css`, `WashikiTheme.css`), not in a
  separate native stylesheet. Then a layout change in the lobby lands in both
  shells.
- **What the native shell hides:**
  - the download and package indicator;
  - browser recommendations;
  - the Babylon video settings (replaced, see section 8);
  - the timedemo download;
  - anything about pointer lock or "click to play".
- **What the native shell adds:**
  - a Quit key;
  - "Change region";
  - "Join by link";
  - the client version next to the web version tag ("client 0.2.0 · web
    v1.0.0-alpha.9");
  - the native update notice.
- **What it reroutes:** the controls in sections 6, 9 and 10. They are the
  same buttons with a different action behind them.
- **One rule for code:** web code never checks `isNative()` inline in a view
  when an existing seam exists. Seams are `NetworkConnector`, the storage
  shim, a `LinkPolicy.open(url)` helper for external links, a
  `FileSave.save()` helper, and the demo start in `DemosView`. Views ask the
  seam, and the seam asks the bridge. The list of native-only differences
  then stays short and in one folder (`client/native/`).
- **Screen size and scale.**
  - The webview fills the window at the system's DPI, like a browser tab at
    100 %. The lobby CSS already serves phones through Capacitor, so
    small windows are expected to work; W0 checks the smallest window
    size the native client allows.
  - The in-game HUD keeps its own `--ui-scale` and `--hud-zoom`, which do
    not affect the lobby.
- **Fonts and images** come from the web deploy (`/engine/fonts/...`), so the
  lobby looks the same in both shells. The native HUD keeps its own bundled
  copies of the same fonts.
- **The start-up screen.** The page's `LoadingScreen` shows while the lobby
  logs in, as in a browser. The native client shows its data sync screen
  before it, which should wear the same start-up look, so the hand-off from
  native to web does not change style.
- **The end screen** stays native (it is drawn over the 3D view). Its "Back
  to the lobby" lands on the web lobby, which is the browser's behaviour
  too.

## 12. Version skew between the web and the native client

The web lobby deploys whenever SHAG does, and the native client releases
when we cut one. Either can be newer.

- **`hello` first.** `NativeBridge.ts` sends `hello` on load and reads
  `capabilities`. A feature the client does not list is hidden or falls back
  to a plain message, for example "Update your client to watch demos".
- **The minimum client.** The page carries `MIN_NATIVE_BRIDGE` (an integer)
  and, optionally, `MIN_NATIVE_CLIENT` (a version). Below either, the page
  shows only an "Update the Washoi client" screen with the download link.
  This replaces the old native menu's update notice for clients too old to
  bridge at all.
- **The native client tolerates unknown messages.** Unknown types get
  `error { code: "unknown_type" }` and are logged once, and unknown fields
  are ignored. The serde enum has `#[serde(other)] Unknown`.
- **Breaking changes** bump `bridgeVersion`. The web keeps the previous
  version's handler for at least two native releases.
- **The game protocol is the same as today.** The native client already
  speaks to game servers directly and checks its protocol version against
  the server's. The bridge adds nothing there.

## 13. Security

The page is our own site, but it renders user text (names, chat, room
titles), so it is treated as able to go wrong.

- **The bridge answers only our origins.** The IPC handler checks
  `req.uri()`. On Linux wry gives the main frame's URL, which is enough
  because the bridge is main-frame only. Messages from any origin outside
  the region list, or from an iframe, are dropped.
- **The init script is main frame only**
  (`with_initialization_script_for_main_only`). Iframes never get
  `__washoiNative`.
- **The bridge is narrow on purpose.**
  - No message reads or writes arbitrary files.
  - `file:save` always goes through the system dialog.
  - `storage:set` accepts only the mirrored keys.
  - `console:exec` refuses `exec` of files and anything that writes outside
    the config dir. It runs only while the lobby is shown, never as a
    background call.
  - `open:external` opens only `https:` URLs.
- **Credentials.** The admission ticket and the account key cross the bridge.
  Neither is logged, and the native crash reporter strips both.
- **Devtools** are on in debug builds and with `developer 1`, otherwise off.
- **Navigation** follows the policy in section 9: no file or data URLs at the
  top level, and no other sites inside the webview.

## 14. Errors and offline

There is no native lobby to fall back to, by decision. What remains is a
native error screen (egui, the same style as the region and data sync
screens), shown when:

- the page does not finish loading within 15 s (`with_on_page_load_handler`
  plus a timer), or reports a load error;
- `hello` does not arrive within 10 s of load, which means an old web deploy
  without the bridge;
- the platform webview is missing, for example no WebView2 runtime, or no
  WebKitGTK on Linux.

It offers Retry, Change region, Console and Quit, plus the reason and a
download link where the reason is a missing runtime. The console there is the
native one, for anything offline, demos from disk included
(`demo <file>`).

## 15. Platforms and packaging

| | Windows | macOS | Linux |
|---|---|---|---|
| Engine | WebView2 (Chromium, Evergreen) | WKWebView | WebKitGTK 4.1 |
| Present by default | Windows 11, and Windows 10 with recent updates | always | no: a package dependency (`libwebkit2gtk-4.1-0`) or checked at start with a clear message |
| Size added to the client | about 1–2 MB (`webview2-com`, loader) | under 1 MB | under 1 MB in the binary, the system library outside |
| Child webview in the winit window | yes | yes (subview) | X11 only; Wayland via XWayland (section 3) |
| Data folder | `%LOCALAPPDATA%\Washoi\webview` | the app's WebKit store | `~/.cache/washoi/webview` |

Licences are MIT/Apache-2.0 for `wry`, and MIT for `webview2-com` and the gtk
crates. wry also brings five Servo crates under MPL-2.0 (`cssparser`,
`cssparser-macros`, `dtoa-short`, `selectors`, and `option-ext` through
`dirs`): file-level copyleft, used unmodified, so `deny.toml` allows MPL-2.0
for those crates only. gtk-rs 0.18 uses the unmaintained `proc-macro-error`
(RUSTSEC-2024-0370) at build time; it is ignored with a reason until wry
moves past GTK 3. wry 0.57.1 needs Rust 1.85, below the client's minimum.

Building on Linux needs `libwebkit2gtk-4.1-dev` (CI and the release job
install it); running needs `libwebkit2gtk-4.1-0`, which the client checks
at start (`the lobby needs a web view: …`).

Memory: a webview costs roughly 80–150 MB while the lobby shows. Hidden
during a match it stays resident. If W0 measures it as a problem, the client
can unload the page in a match and reload it after, because the return
reloads anyway. The cost is losing lobby chat during the match.

## 16. What retires on the native side

Once the webview lobby ships (0.2.0):

- **wash-fps-rust:**
  - `menu.rs`'s lobby tabs, `client/lobby_panel.rs`, `lobby_welcome_dialog.rs`
    and `lobby_welcome_guide.rs`;
  - `profile_view.rs`, `leaderboard_view.rs` and `demos_view.rs`;
  - the menu's `--screenshot --tab` modes.

  These were one-to-one ports kept only for the native menu. RUST.md's port
  rule notes them as retired, not missing.
- **Kept:**
  - shagger's `LobbyClient` and its `SettingsSync` port, used by `washoi
    lobby …` and the local-server tests;
  - the region screen, the data sync screen, the in-game settings, the
    console, the end screen, demo playback and download.

## 17. Testing

- **Bridge fixtures.** `tools/golden/bridge/*.json`: one file per message
  type and direction. A vitest suite in wash-fps and a Rust test in
  wash-fps-rust both parse every fixture and re-serialise it unchanged, so
  the two type definitions cannot drift.
- **Web unit tests** with a fake bridge:
  - `NativeNetworkConnector` drives `ConnectingOverlay` through play, watch,
    ticket recovery and failure;
  - the storage shim mirrors only the listed keys;
  - the link policy;
  - `client.html` contains no `@babylonjs` chunk.
- **The cvar registry test.** Dump the web client's registered cvars (name,
  type, default, range) to JSON from the lobby entry. The Rust test fails on
  any shared name whose fields differ.
- **Native integration, in CI on Linux (Xvfb and WebKitGTK).** This uses the
  local servers this repo already uses for the headless client tests:
  wash-fps `lobby:server`, `net:server --single-match` with
  `SHAG_TIMELIMIT`, and the probe server for the page.
  1. The client opens `/client` and gets `hello`.
  2. It queues against a bot and gets `match:play`.
  3. It plays to the end screen, presses Back to the lobby (a synthetic
     click), and checks that the page is back and logged in as the same
     account.
  4. It changes a setting in the lobby and checks that it lands in
     `cvars.json`.

  Screenshots at each step go to the parity folder.
- **By hand on Windows and macOS** before each release: the tester's
  checklist (the same list as above, plus fullscreen, Alt+Enter in the
  lobby, a key save, an external link, quit).

## 18. Work plan by repository

Steps in order. Each lands with its tests.

| Step | What | Repos |
|---|---|---|
| **W0 spike** | wry child webview in the winit window over the wgpu surface. Toggle visible and hidden with a 3D scene behind it. Focus round trip. Measure size, memory, and whether the lobby socket survives about 15 minutes hidden. On Windows, macOS, Linux X11 and Wayland via XWayland. Ends with the go-ahead or changes to sections 3 and 15. | shagger (spike branch), wash-fps-rust |
| **W1 shell and bridge** | `shagger::webview` (engine code: host, transport, origin policy, navigation policy, init script). The single-window app state machine in wash-fps-rust. `NativeBridge.ts`, `client.html` and `client/native/main.ts` with `hello`. External links, quit, fullscreen, the error screen. | shagger, wash-fps-rust, wash-fps, SHAG (Vite entry) |
| **W2 matches** | `NativeNetworkConnector`, `match:*`, seat reissue, watch and drop in, the return. Lobby music pause and resume. | wash-fps, wash-fps-rust |
| **W3 account and settings** | The key mirror, the storage shim and the mirrored files. `Cvar.reloadFromStorage` and `CommandBindings.reload`. The Video page from `cvars:describe`. The cvar registry test. | wash-fps, SHAG, wash-fps-rust |
| **W4 the rest** | Demos and reels, `data:status` and `missing_data`, console passthrough, key save, clipboard fallback, invite paste, update notice, `data-shell` CSS. | wash-fps, wash-fps-rust |
| **W5 release** | Retire the egui lobby (section 16), the CI integration test, RUST.md, then release 0.2.0. | wash-fps-rust |

The web changes go through pull requests on wash-fps and SHAG. washoi.net only
gains `clientUrl` in `regions.json`.

### 18.1 W0 results (November 4, 2026)

`tools/webview_spike` in wash-fps-rust (its own workspace, not part of the
game) is the spike: one winit window, a wgpu surface cleared to a moving
colour as a stand-in for the match, and a wry 0.57.1 child webview over it.

- It loads either a built-in lobby stand-in (bridge hello both ways, a
  one-second timer, a WebSocket pinging a local echo server every five
  seconds) or the live `https://eu.washoi.net/`.
- It hides the webview on a schedule, with `focus_parent()`, and shows it
  again.
- It reports page visibility, timer gaps, the socket and memory to JSON.
- `webview_spike.yml` runs it by hand on GitHub's Windows, macOS and Linux
  runners.

Runs: Linux X11 here (Xvfb, WebKitGTK 2.52, llvmpipe), and Windows Server
and macOS 14 on GitHub's runners (runs 41831741557 and 41836513742).

| | Windows (WebView2) | macOS 14 (WKWebView) | Linux X11 (WebKitGTK) |
|---|---|---|---|
| Child webview over the wgpu surface | works | works | works |
| Hide and show, with the 3D view behind | works; `visibilitychange` fires both ways | works | works |
| Bridge hello, page → native and back | works | works | works |
| The live game page loads and reaches "click to start" | yes | yes | yes |
| Timers while hidden, as wry sets them up | throttled to **once a minute** after 5 min (Chromium's intensive wake-up throttling) | about every 2 s | every 2 s |
| Timers while hidden, throttling off | every 1 s for 10 min | about every 2 s | (not needed) |
| WebSocket while hidden | stayed open; with throttling off, every 5 s ping answered for 10 min | stayed open, every ping answered for 15 min | stayed open, every ping answered |
| Memory, lobby stand-in (webview processes) | about 105–140 MB hidden, 250 MB shown | about 80 MB | about 330 MB (software rendering) |
| Memory, the full game page | about 850 MB | about 510 MB | about 1.6 GB (software WebGL) |
| Spike binary, release | 7.3 MB | 6.2 MB | 11.5 MB (with wgpu) |
| Window to first bridge message (stand-in page) | about 3–4 s (first WebView2 start) | about 2 s | about 0.2 s |

What this settles:

- **The design in section 3 holds on all three platforms.** One window, a
  child webview, hidden during play.
- **Throttling must be off.** WebView2's default would leave a hidden lobby
  page running its timers once a minute. The socket survives that, but
  request timeouts, settings uploads and presence would lag. The client
  passes `--disable-background-timer-throttling
  --disable-renderer-backgrounding
  --disable-features=...,IntensiveWakeUpThrottling` on Windows (keeping
  wry's own defaults in the same string), and
  `with_background_throttling(Disabled)` on macOS.
- **The lobby-only `/client` entry is needed for memory.** The full game
  page costs 500 MB to 1.6 GB in the webview, against 80 to 330 MB for a
  plain page.
- **Origins.** A page loaded with `with_html` reports `about:blank` as its
  origin. The real `/client` page will report its URL, which is what the
  origin check reads. The spike did not exercise a bridge from the live site,
  since it has none yet.

Not covered yet:

- **Keyboard focus by hand.** Typing in the page, then pressing keys in the
  match after `focus_parent()`. The runners have no input device to script.
  The window got focus back on hide on Windows (the `Focused(true)` event at
  the hide), and this goes on the tester's checklist.
- **Wayland through XWayland.** It is the same X11 path as tested. A run on
  a Wayland desktop goes on the tester's checklist too.
- **GitHub's Linux runner** has no Vulkan or GL adapter for wgpu. Linux is
  proven by the run here.

### 18.5 W5 progress (November 4, 2026)

- **Smoke test:** `.github/workflows/lobby_smoke.yml` builds the client on
  Ubuntu 24.04 (Xvfb), macOS 14 and Windows and runs
  `tools/lobby_smoke/run.sh`: the client starts `--offline` on a stand-in
  package, opens the live `/client` page (or the `page` input) and passes
  when the page says hello over the bridge (`lobby: the page said hello`
  in the log). The log and a screenshot are artifacts. It runs on pushes to
  main that touch the lobby and by hand. A full match stays a local test
  (the steps in 17): it needs the lobby, game and bot servers and the
  game data.
- **The egui lobby** (`menu.rs`'s lobby tabs, `lobby_panel.rs`, the lobby
  views and `washoi menu`) is no longer reached from `washoi` and is
  removed once the webview lobby has been tried on Windows and macOS.

### 18.4 W4 progress (November 4, 2026)

- **Demos** go through `demo:play` (`filename`, `base`, `reel`). The client
  downloads with the native menu's cache and checks, answers once the file
  is in, and plays it in the same window. Leaving playback sends
  `demo:ended`, and the page shows the Demos page again without a reload.
  `demo:cancel` stops a download. The Demos page takes a registered
  `DemoPlayer`; a browser registers none.
- **File save.** A `blob:` or `data:` link with `download`, in the page or
  clicked from code (`ProfileView.saveKeyFile`), becomes `file:save`. The
  client writes it to the Downloads folder (home when there is none) under
  a free name, and the page says where it went. Ran on Linux.
- **Clipboard.** `navigator.clipboard.writeText` works in WebKitGTK (Copy
  key says Copied ✓), as it does in WebView2 and WKWebView on a secure
  origin, so `clipboard:write` is not added.
- **Update notice.** Once the page has said hello, the client sends
  `native:update` with the newer version and its package URL. The page shows
  Download (opened in the browser) and Later, and both send
  `update:dismiss`.
- **Invite paste.** Pasting a room link outside a text field joins it. The
  page follows the link's `?lobby=…&invite=…` deep link on `/client`. A link
  for another region's host says to pick that region at start. Ran on Linux
  with xclip.
- **No second start click on any reload in a session.** This covers an
  invite as well as a return: sessionStorage `washoi.native.started`.
- **Babylon stays in the lobby-only bundle for now.** The import graph
  reaches it through the engine's own lobby pieces:
  - the 3D logo (`LogoCanvas`, a Babylon engine of its own);
  - lobby audio (`AudioManager`);
  - the `engine/core` barrel.

  The page is still free of the game renderer, the scene and the maps,
  which is where W0's 0.5 to 1.6 GB went. Under Xvfb with llvmpipe, the
  built `/client` page held about 1.3 GB, against 0.25 GB for a blank page,
  but software GL makes that number meaningless. Measure on real hardware
  with the spike workflow before splitting SHAG's audio and logo off
  Babylon. That work would be a SHAG change, not a wash-fps one.
- **Console passthrough is deferred.** The lobby page's console sets cvars
  through the mirror already. Native-only commands (connect, demo, timedemo)
  have no use in the lobby, and passing them through needs an
  unknown-command hook in SHAG's `CommandRegistry`.

### 18.3 W3 progress (November 4, 2026)

Landed: shagger `lobby/storage_mirror.rs` and `Cvars::declarations`, the
mirror in `lobby_webview.rs`, and wash-fps `client/native/NativeStorage.ts`.

- **The snapshot is applied once per webview session** (a sessionStorage
  flag). The init script is fixed when the webview is made, so on a later
  reload it is stale. Local storage is then current through `storage:set`
  and the `storage:changed` sent before `match:ended`.
- **Adoption:** a mirrored key the page has and the files lack (an account
  made in the webview) is sent to the client on that first load.
- **The Video page needs no `cvars:describe`.** The native client already
  registers the same `r_*` cvars as the web renderer (its settings are a
  port of the web view). The lobby-only page lacks them only because no
  renderer registers them there. The init script carries the client's
  registered cvar declarations (`cvars`), and the page registers the ones it
  does not declare itself. The shared Video page then works unchanged and
  writes `engine_cvars`, which lands in `cvars.json` and the live registry.
  `cvars:describe` and `cvar:set` are dropped from the message table's plan.
- Run on Linux: the 0.1.x `lobby-key` was injected and used (the local
  lobby did not know it, so the page set it aside as `lobby-key.unknown`
  and made a new account, both mirrored), and choosing Smooth on the Video
  page wrote `cvars.json`.
- Left: the web presets name a few cvars the native client does not
  register, so the page reads "your own mix" after a preset. The cvar
  registry test (section 17) will list them. `Cvar.reloadFromStorage` is not
  needed while the return reloads.

### 18.2 W1 and W2 progress (November 4, 2026)

Landed: shagger `webview` (behind the `webview` feature), the kept window
and the lobby screen in wash-fps-rust (`lobby_webview.rs`), and in wash-fps
`client/boot.ts`, `client/native/` (`NativeBridge`, `LinkPolicy`,
`ShellControls`, `NativeNetworkConnector`, the lobby-only `main.ts`).

Run end to end on Linux under Xvfb against the local stack (lobby server,
bot server, the probe server's `/client`): the page loads inside the game
window, `hello` is answered (the version line reads `client 0.1.1 · …`),
the QUIT key shows, a room with three bots starts its match, the page hides
and the match plays natively, the end screen's Back to the lobby shows the
page again, and the reloaded page skips the start click.

Found and fixed on the way:

- winit's Xlib error handler is process wide and failed the lobby screen's
  event loop on WebKitGTK's harmless `GLXBadWindow` when the page was
  hidden. shagger registers a winit error hook that leaves errors on other
  X connections to GTK.
- The reloaded page asked for the start click again; `match:ended` now
  leaves `washoi.native.after_match` in sessionStorage, and the page
  dismisses the start screen and shows a failed match's message.

Not yet: the client has no spectator-to-player seat change, so it does not
offer `seat` and the page leaves seat changes to the overlay (a new
connection). The welcome guide opens again on each return, as it does in
the browser after a single-match return.

## 19. Decisions (confirmed November 3, 2026)

1. **A lobby-only entry at `/client`** rather than the full game page with
   a flag (section 4). The owner preferred `/client` to `/native`.
2. **Keep the webview alive and hidden during a match**, as the browser
   does (section 3; revisit only if W0's memory numbers are bad).
3. **The return reloads the lobby page** first, exactly the browser's
   single-match flow; a no-reload return later.
4. **Linux Wayland via XWayland** for 0.2.0 (section 3), with the GTK window
   path kept for later.
5. **The settings UI stays the shared web one**; only the Video page's
   fields come from native data (`cvars:describe`, section 8).
6. **Invites:** paste-a-link in 0.2.0, and a `washoi://` URL scheme from the
   installers later (section 9).
