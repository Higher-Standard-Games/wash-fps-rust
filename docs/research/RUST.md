# A native Washoi client in Rust

Opened: October 31, 2026. Status: research rounds 1 to 6 done; milestone 0
done; milestone 1 done except demos; milestone 3 has map loading,
collision and the movement profile (section 23).

This file lives in wash-fps-rust and is the plan of record. It started in
wash-fps (`docs/research/RUST.md` on `staging`), and that copy is no longer
updated. Paths such as `client/V2NetworkClient.ts` refer to wash-fps, and
`src/engine/...` to SHAG, unless a section says otherwise.

**End goal:** a native Washoi game client, written in Rust, that connects to the
servers that run today (`wss://eu.washoi.net`, `wss://us.washoi.net`), plays a
full match against browser players, and keeps the shape of the SHAG/Washoi code
so that anyone who knows the TypeScript can find their way around the Rust.

This document is an investigation journal. Each round goes one layer deeper and
appends to it. Round 1 maps the whole client, writes down the network contract
exactly as the code implements it, and proposes the crate map, workspace layout
and milestone plan. Later rounds take one layer each down to function level.

Why it lives here: the end product is the Washoi client, so it sits in
`wash-fps/docs/research`. Most of the code it translates is SHAG
(`src/engine`, `src/shared`), and SHAG's research docs are linked where they
matter.

Every claim below was checked against SHAG `4168ec3` and wash-fps `3cba4e0`
(both October 30, 2026; engine `1.0.0-alpha.9.1`, Babylon.js `^9.31.2`).
Paths starting `src/` are in SHAG; the others are in this repository.

---

## Contents

1. [Verdict after round 1](#1-verdict-after-round-1)
2. [What "the client" is](#2-what-the-client-is)
3. [How coupled the code is](#3-how-coupled-the-code-is)
4. [Layer 1: the wire protocol](#4-layer-1-the-wire-protocol)
5. [Layer 2: prediction, reconciliation, interpolation and lag compensation](#5-layer-2-prediction-reconciliation-interpolation-and-lag-compensation)
6. [Layer 3: shared simulation and collision](#6-layer-3-shared-simulation-and-collision)
7. [Layer 4: assets and map formats](#7-layer-4-assets-and-map-formats)
8. [Layer 5: rendering](#8-layer-5-rendering)
9. [Layer 6: audio](#9-layer-6-audio)
10. [Layer 7: input, console, cvars and settings](#10-layer-7-input-console-cvars-and-settings)
11. [Layer 8: HUD, menus and lobby UI](#11-layer-8-hud-menus-and-lobby-ui)
12. [Module to crate map](#12-module-to-crate-map)
13. [Translating TypeScript patterns to Rust](#13-translating-typescript-patterns-to-rust)
14. [Proposed workspace: the SHAGGER engine and Washoi](#14-proposed-workspace-the-shagger-engine-and-washoi)
15. [Milestones and estimates](#15-milestones-and-estimates)
16. [Risks and things to settle](#16-risks-and-things-to-settle)
17. [Next rounds](#17-next-rounds)
18. [Round 2: the protocol at function level](#18-round-2-the-protocol-at-function-level)
19. [Round 3: the shared simulation](#19-round-3-the-shared-simulation)
20. [Round 4: assets](#20-round-4-assets)
21. [Round 5: the renderer](#21-round-5-the-renderer)
22. [Round 6: audio](#22-round-6-audio)

---

## 1. Verdict after round 1

It is doable, and the order to do it in is clear. The code splits cleanly into
three kinds:

- **Pure logic that translates almost line for line** (about 22,600 lines): the
  binary protocol, the message codec, game state, snapshot history, clock sync,
  the lobby client, the shared simulation (movement, slide, wall jump, ledge
  grab, swim, ladder, knockback), the collision world, lifts and triggers, demo
  reading, and the entity catalog. None of it imports Babylon or the DOM. This
  is what makes "connects to current servers" realistic: the client and server
  already share this code, so a Rust translation of it is a translation of the
  contract itself.
- **Logic wrapped around Babylon** (about 77,000 lines across SHAG and 24,000 in
  Washoi, in files that import `@babylonjs`): BSP mesh building, Q3 shader
  materials, lighting, effects, weapons, animation, camera. The logic carries
  over; the Babylon calls do not. This needs a new renderer.
- **DOM and CSS UI** (about 30,000 lines of TypeScript touching the DOM plus
  19,600 lines of CSS): HUD widgets, lobby, settings, scoreboard, chat, demos
  browser. None of it carries over as code. The HUD already reads from one data
  layer (`client/HUDState.ts`), so the data side ports and the drawing is new.

One finding resets expectations. The "partial WebGPU path" in SHAG is Babylon's
own `WebGPUEngine` (`src/engine/rendering/RenderingEngineFactory.ts`), plus
WGSL branches inside Babylon material plugins (34 places, for example
`src/engine/loader/bsp/BspBakedLightingPlugin.ts`). Those snippets are fragments
injected into Babylon's StandardMaterial and PBRMaterial shaders, not
stand-alone pipelines. They are a good reference for the math (lightmap layers,
Q3 stage colour, autosprite, soft particles, fog), but the surrounding shader,
the pipelines and the frame graph have to be written from scratch on `wgpu`.

Rough size: a playable "connect, move, shoot, see everyone, basic HUD" client
is about 25 engineer-weeks. Parity with the browser client, without the editor,
is about 55 to 75 engineer-weeks. Section 15 breaks this down.

The first three milestones need no renderer at all, and each one proves part of
the contract against live servers:

1. Decode every snapshot in every recorded `.shagdemo` in Rust and match the
   TypeScript decoder field for field.
2. A headless Rust client that logs into the lobby, joins a match as a
   spectator, and prints the scoreboard.
3. The same client playing: sending 125 commands a second, predicting its own
   movement, and reaching a high "confirmed without replay" count, which is the
   proof that the Rust movement matches the server's.

---

## 2. What "the client" is

The browser application starts in `client/main.ts`. It builds a Babylon engine
and scene, registers Washoi's services with the engine registries
(`client/registerServices.ts`), shows the lobby (`client/LobbyPanel.ts`), and on
`lobby:connect` loads the map and connects to a game server
(`client/FpsMapLauncher.ts`, `client/MatchMapLoader.ts`,
`client/FpsNetworkConnector.ts`).

During a match, the per-frame order lives in `FPSController.update`
(`camera/FPSController.ts`, around line 930):

1. `Game.processSnapshots()`: reconcile the local player against buffered
   snapshots (`client/V2NetworkClient.ts`).
2. `Game.renderRemotePlayers()`: interpolate and draw the other players
   (`client/RemotePlayerRenderer.ts`, `client/FrameEffects.ts`).
3. A fixed 125 Hz command loop: `Game.sendInput()` builds an `FPSInputCommand`,
   predicts it locally and sends it (`Game.ts`, around line 1361).
4. Camera placement with stair smoothing, view effects, weapon view model, HUD.

### In scope for the port

| Area | Where it is today |
| --- | --- |
| Protocol, game state, snapshot history | `src/shared/net`, `src/engine/network` |
| Lobby client (JSON over WebSocket) | `src/lobby/LobbyClient.ts`, `src/lobby/types.ts`, `client/FpsLobbyService.ts` |
| Shared simulation and collision | `simulation/`, `liquid/`, `src/shared/collision`, `src/shared/bsp`, `src/shared/server/simulation`, `src/engine/physics` |
| Prediction and interpolation | `client/V2NetworkClient.ts`, `client/InputHistory.ts`, `src/engine/network/client/*` |
| Assets | `src/engine/loader/{pk3,bsp,shader,texture,model,map}` |
| Rendering | `src/engine/{rendering,lighting,effects,sprites,weapon,animation}`, `liquid/`, parts of `client/` |
| Camera and view effects | `src/engine/{camera,viewport}`, `camera/FPSController.ts` |
| Audio | `src/engine/AudioManager.ts`, `src/engine/audio`, `client/FPS*Audio.ts` |
| Input, console, cvars | `src/engine/{input,console,core}`, `console/` |
| HUD, menus, lobby UI | `src/engine/ui`, `client/hud`, `client/settings`, `client/*View.ts`, `client/Lobby*.ts` |
| Game glue | `Game.ts`, `FPSPlayer.ts`, `world/`, `client/MatchStateManager.ts` |

### Out of scope

The game server, the lobby server and the bot server stay in TypeScript
(`server/`, `src/lobby/LobbyServer.ts`, `bot/`). So do SHAGGED
(`src/engine/editor`, 14,300 lines), the CLI tools, bot training, and the
Electron and Capacitor shells. Demo playback is in scope later; the decoder for
it comes free with milestone 1.

---

## 3. How coupled the code is

Line counts below skip tests and QA probes. "Babylon" counts lines in files that
import `@babylonjs`; "DOM" counts lines in files that touch `document`,
`window`, `HTMLElement` or `localStorage`. A file can be in both.

| Module | Files | Lines | Babylon | DOM | Port style |
| --- | ---: | ---: | ---: | ---: | --- |
| `src/shared/net` | 4 | 2,252 | 0 | 0 | direct |
| `src/shared/server` | 19 | 2,209 | 0 | 0 | direct |
| `src/shared/bsp` | 5 | 1,226 | 0 | 0 | direct |
| `src/shared/collision` | 3 | 603 | 0 | 0 | direct |
| `src/shared/demo` | 6 | 2,212 | 0 | 0 | direct |
| `src/shared/entities` | 7 | 627 | 0 | 0 | direct |
| `src/engine/network` | 37 | 6,201 | 0 | 1,557 | direct (timers become the frame clock) |
| `src/lobby/LobbyClient.ts` | 1 | 568 | 0 | 0 | direct |
| `simulation/` | 13 | 3,135 | 0 | 0 | direct |
| `shared/` (Washoi) | 38 | 2,361 | 0 | 0 | direct |
| `src/engine/physics` | 12 | 3,609 | 3,047 | 0 | direct (Babylon used only for `Vector3`) |
| `src/engine/movement` | 3 | 1,193 | 273 | 913 | direct |
| `src/engine/core` | 28 | 6,995 | 1,149 | 2,779 | mostly direct |
| `src/engine/loader/bsp` | 31 | 8,962 | 6,699 | 192 | parse direct, mesh building re-targeted |
| `src/engine/loader/pk3` | 11 | 3,265 | 1,094 | 2,044 | direct, fetch becomes HTTP + disk cache |
| `src/engine/loader/shader` | 19 | 4,873 | 2,325 | 0 | parser direct, materials rewritten |
| `src/engine/loader/texture` | 12 | 3,740 | 3,420 | 1,544 | decoders replaced by crates |
| `src/engine/loader/model` | 9 | 2,359 | 2,203 | 0 | MD3 parse direct, glTF by crate |
| `src/engine/loader/map` | 9 | 2,687 | 2,238 | 1,568 | re-targeted |
| `src/engine/rendering` | 49 | 10,678 | 10,448 | 2,207 | rewritten on wgpu |
| `src/engine/lighting` | 12 | 2,858 | 2,858 | 0 | rewritten |
| `src/engine/effects` | 12 | 4,247 | 4,019 | 2,128 | logic kept, drawing rewritten |
| `src/engine/sprites` | 5 | 1,571 | 1,488 | 0 | rewritten |
| `src/engine/weapon` | 37 | 12,331 | 11,387 | 285 | logic kept, drawing rewritten |
| `src/engine/animation` | 11 | 2,841 | 2,770 | 0 | rewritten around a skeletal crate |
| `src/engine/camera` | 19 | 3,464 | 2,730 | 1,127 | mostly direct (math) |
| `src/engine/viewport` | 17 | 1,433 | 721 | 0 | direct |
| `src/engine/input` | 7 | 1,518 | 991 | 1,222 | re-targeted to winit |
| `src/engine/AudioManager.ts`, `src/engine/audio` | 3 | 1,163 | 1,161 | 706 | re-targeted to an audio crate |
| `src/engine/console` | 78 | 8,490 | 4,614 | 921 | registry direct, commands case by case |
| `src/engine/ui` | 45 | 11,021 | 311 | 10,615 | rewritten |
| `src/engine/world`, `src/engine/player` | 4 | 1,136 | 925 | 942 | mostly direct |
| `liquid/` | 8 | 1,637 | 1,038 | 0 | sampler direct, visuals rewritten |
| `world/` | 14 | 3,216 | 2,299 | 500 | logic kept |
| `camera/` | 2 | 1,890 | 1,890 | 1,482 | logic kept |
| `console/` (Washoi) | 17 | 1,353 | 241 | 33 | direct |
| `client/` | 138 | 35,173 | 14,005 | 18,291 | split: logic kept, UI rewritten |
| `Game.ts`, `FPSPlayer.ts` | 2 | 2,628 | 2,628 | 2,305 | logic kept |
| **Total in scope** | | **≈168,000** | | | |

The direct-translation rows (the first ten, plus `src/engine/movement`) come to
about 22,600 lines. They are also the rows where a mistake means the server and
the client disagree, which is why they come first and come with parity tests.

---

## 4. Layer 1: the wire protocol

This section is the contract a Rust client has to meet, written from the code.
There are two separate protocols: the lobby (JSON text frames) and the game
server (binary frames). Both run over WebSocket.

### 4.1 Transport and addresses

- **Lobby:** `wss://eu.washoi.net/lobby` and `wss://us.washoi.net/lobby`
  (`washoi.net/src/data/regions.ts`, `scripts/deployment/deploy-eu.sh`). Local
  default `ws://localhost:9000` (`client/FpsLobbyService.ts`). The server is the
  `ws` package with `maxPayload: 64 * 1024` (`src/lobby/LobbyServer.ts:558`).
- **Game servers:** the lobby hands out the URL in `lobby:connect`. It is built
  as `${SHAG_PUBLIC_GAME_PROTOCOL}://${SHAG_PUBLIC_GAME_HOST}:${port}` with ports
  from 7777 upward (`src/lobby/GameServerSpawner.ts:66`). In production that is
  `wss://<region host>:<port>` with real certificates.
- Neither server checks the `Origin` header (no match for `origin` in
  `src/engine/server/network/transport/WebSocketTransport.ts` or
  `LobbyServer.ts`), so a native WebSocket client is accepted as it is.
- The browser transport is `src/engine/network/client/transport/WebSocketTransport.ts`:
  binary frames, `lossless = true`. `WebTransportTransport.ts` exists and is
  never instantiated; AGENTS.md says to keep it as the preferred future
  transport. The Rust transport trait should keep the same shape
  (`send(data, reliable)`, `lossless`) so a QUIC transport can drop in later.

**Rust:** `tokio-tungstenite` with `rustls` and `webpki-roots` for both
connections. The network task runs on its own thread and talks to the frame
loop through channels, which replaces the browser's event callbacks.

### 4.2 Primitive encoding

`src/shared/net/BinaryProtocol.ts`, classes `BinaryEncoder` and `BinaryDecoder`:

| Type | Encoding |
| --- | --- |
| integers, floats | little-endian; `u8 i8 u16 i16 u32 i32 f32 f64` |
| string | `u16` byte length, then UTF-8 bytes |
| bool | `u8`, nonzero is true |

Two details matter for exact compatibility:

- `writeString` coerces `undefined` to `""` and anything else through
  `String()`. Some events still put numeric ids into string fields; on the wire
  they are just text, so Rust reads them as `String`.
- Decoding throws on underflow. Optional trailing fields are read only when
  `decoder.hasMore()`. The Rust decoder needs the same `has_more()` and must
  treat underflow as an error for the whole message.

```rust
pub struct Reader<'a> { buf: &'a [u8], pos: usize }

impl<'a> Reader<'a> {
    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    pub fn string(&mut self) -> Result<String, DecodeError> {
        let len = self.u16()? as usize;
        Ok(std::str::from_utf8(self.take(len)?)?.to_owned())
    }
    pub fn has_more(&self) -> bool { self.pos < self.buf.len() }
    // u8, i8, i16, u32, i32, f32, f64, bool follow the same pattern.
}
```

### 4.3 Game server messages

`MessageType` in `src/shared/net/BinaryProtocol.ts`; every message starts with
its type byte. Codecs are in `src/engine/network/protocol/MessageCodec.ts`.

| Code | Name | Direction | Payload after the type byte |
| --- | --- | --- | --- |
| `0x01` | Welcome | S→C | `serverId: str, serverName: str, map: str, tickRate: u8, maxPlayers: u8, currentPlayers: u8` |
| `0x02` | JoinAccept | S→C | `clientId: str, playerId: str, entityId: str` |
| `0x03` | JoinReject | S→C | (not handled by the browser client; treat as failure) |
| `0x04` | LegacySnapshot | S→C | refused: "update server and clients together" |
| `0x09` | ServerMessage | S→C | `type: str, message: str` |
| `0x0a` | Pong | S→C | `clientSendTime: f64, serverTime: f64` |
| `0x0b` | Snapshot | S→C | `hasBaseline: u8, baselineTick: u32`, then the raw snapshot (4.4) |
| `0x0c` | ResumeToken | S→C | `token: str, graceMs: u32` |
| `0x81` | Join | C→S | `playerName: str, isBot: u8`, optional `admissionTicket: str` |
| `0x82` | Ready | C→S | empty |
| `0x83` | Input | C→S | the command (4.6) |
| `0x84` | Disconnect | C→S | |
| `0x85` | Ping | C→S | `clientSendTime: f64` |
| `0x86` | JoinGame | C→S | `teamFlag: u8` (0 none, 1 lobby-assigned, 2 picked in game), `teamId: str` if flag > 0, optional `admissionTicket: str` |
| `0x87` | Spectate | C→S | empty |
| `0x88` | RequestSpawn | C→S | empty |
| `0x89` | Chat | C→S | `text: str` (the browser prefixes `/say_team ` for team chat and cuts at 128 characters) |
| `0x8a` | SetLoadout | C→S | `primary: str, secondary: str` |
| `0x8b` | Resume | C→S | `token: str` |
| `0x8c` | ClientPrefs | C→S | `flags: u8`, bit 0 set means "do not auto-switch on pickup" |
| `0x08` | Ack | C→S | `tick: u32` |

`0x05`–`0x07` (PlayerJoined, PlayerLeft, GameStart) are defined but the client
does not handle them.

### 4.4 The connection sequence

From `src/engine/network/client/hosts/NetworkGameHost.ts`:

```text
client                                   game server
  | -- WebSocket open ------------------------> |
  | -- Join(name, isBot=0, admissionTicket) --> |
  | <------------------------------- Welcome -- |  client rejects if tickRate != 125
  | <---------------------------- JoinAccept -- |  local playerId and entityId
  | <--------------------------- ResumeToken -- |  seat is held graceMs after a drop
  | -- ClientPrefs ---------------------------> |  sent after every JoinAccept
  | -- Ping x5, 100 ms apart -----------------> |  then one every 30 s (ClockSync)
  | <------------------ Snapshot (full/delta) -- |  62.5 Hz
  | -- Ack(tick) -----------------------------> |  for every snapshot decoded
  | -- Input x125/s --------------------------> |
  | -- JoinGame(team) ------------------------> |  spectator to player
```

- Snapshots that arrive before both Welcome and JoinAccept are dropped and not
  acknowledged.
- The handshake has a timeout ("Timeout waiting for Welcome and JoinAccept").
- A duplicate JoinAccept is ignored.
- A watchdog checks every 500 ms for snapshots that stop arriving while the
  socket stays open (`ConnectionWatchdog.ts`). On a stall, or on a close
  mid-match, the client reconnects and sends `Resume(token)`, retrying every
  2 s until `graceMs` has passed.
- The music playlists (`data/game.playlist.json`, `data/endgame.playlist.json`)
  switch on match phase inside the network host. In Rust that belongs in the
  game layer, reacting to an event.

**Gap to raise with the server side:** nothing in this handshake carries a
protocol version. The only compatibility check is the tick rate. Today the
browser client and the server ship together, so it does not matter. A native
client that ships on its own schedule would misread a changed layout without
noticing. Section 16 suggests a small, backward-compatible fix.

### 4.5 Snapshots

`MessageCodec.encodeSnapshot` puts a 6-byte envelope in front of the raw format
that demos also record:

```text
[0x0b][hasBaseline: u8][baselineTick: u32]          envelope (SNAPSHOT_HEADER_BYTES = 6)
[tick: u32][serverTimestamp: f64][ackedInputSeq: u32]
[entityCount: u16]  entity × N
[playerCount: u16]  player × N
[eventCount:  u16]  event  × N
[pickupCount: u16][ceil(count / 8) bytes of bits]   optional: read only if bytes remain
```

`hasBaseline` must be 0 or 1, and `baselineTick` must be 0 when `hasBaseline`
is 0. A delta snapshot decodes against the stored snapshot with exactly that
tick, or not at all (`MissingSnapshotBaselineError`).

**Entities.** Every live entity is listed in every snapshot; an entity that is
missing has been removed. Each entry is `id: str, flags: u8`, then the fields
its flags select, in this order:

| Flag | Field | Bytes | When the flag is clear |
| --- | --- | --- | --- |
| `0x01` | position `x, y, z: f32` | 12 | copy from baseline |
| `0x02` | velocity `x, y, z: f32` | 12 | copy from baseline |
| `0x04` | grounded `u8` | 1 | copy from baseline |
| `0x08` | rotation `yaw: f32, pitch: f32` | 8 | copy from baseline |
| `0x10` | movement state, see below | 1 or 5 | copy the four booleans from baseline; the view fields are not carried |
| `0x20` | weapon meta `weaponId: str, settled: u8` | 3+ | copy from baseline |
| `0x40` | powerup mask `u8` | 1 | **no powerups**; never carried, the server sends it every snapshot |

Movement state byte: bit 0 crouching, bit 1 sliding, bit 2 wall jump charging,
bit 3 aiming, bit 7 "view follows". With bit 7 set, four more bytes follow:
keys (bits 0–1 strafe, bits 2–3 forward; 0 none, 1 right or forward, 2 left or
back), slide phase (`u8 / 255`), ledge grab phase (`u8 / 255`), and crouch view
offset (`i8 / 4`).

The server sends the movement state and the powerup mask for every entity that
has them, every snapshot, on purpose: a delta flag that sticks after a baseline
mismatch would leave a player crouched or glowing.

The TypeScript decoder builds loosely typed component maps (`position`,
`transform`, `velocity`, `movementState`, `weaponMeta`, `powerups`) and keeps
both `position` and `transform.position` for older callers. Rust should decode
into one typed struct per entity instead, and keep the carry-forward rules
above exactly:

```rust
pub struct NetEntity {
    pub id: EntityId,
    pub position: Option<Vec3>,       // f32 on the wire; widen to f64 for simulation
    pub velocity: Option<Vec3>,
    pub grounded: bool,
    pub yaw_pitch: Option<(f32, f32)>,
    pub movement: Option<MovementBits>,
    pub view: Option<ViewBits>,       // only from this snapshot's bytes, never the baseline
    pub weapon_meta: Option<WeaponMeta>,
    pub powerup_mask: Option<u8>,     // None means no powerups
}
```

**Players.** Each entry is `id: str, flags: u8`, then: `0x01 entityId: str`,
`0x02 name: str`, `0x04 score: i32`, `0x08 deaths: u32`, `0x10 ping: u16`,
`0x20 team: str`. A clear flag copies the field from the baseline.

**Events.** Each event is `code: u8` plus a fixed layout. Codes 23, 24 and 25
are extended versions of 10 and 4 that decode to the same event type, so old
demos keep decoding:

| Code | Type | Fields |
| --- | --- | --- |
| 1 | `combat_state` | `entityId: str, health: u16, armor: u16, dead: u8` |
| 2 | `match_state` | one JSON string (`MatchStateSnapshot`, `src/shared/server/gametype/GametypeTypes.ts:376`) |
| 3 | `weapon_state` | `entityId, currentWeapon: str, weapons: JSON str, ammo: JSON str, reloading: u8, lastFireTick: u32` |
| 4 | `weapon_fire` | `entityId, weaponId: str, origin xyz f32, dir xyz f32` |
| 24 | `weapon_fire` (seeded) | code 4's fields, then `pelletSeed: i32, pelletSpread: f32` |
| 5 | `weapon_charge` | `entityId, weaponId: str, charging: u8` |
| 6 | `projectile_spawn` | `entityId, defId: str` |
| 7 | `projectile_impact` | `defId: str, xyz f32, normal xyz f32` |
| 8 | `item_pickup` | `pickupId: i16, weaponId, entityId, droppedWeaponId: str, xyz f32` |
| 9 | `weapon_drop` | `entityId, weaponId: str, xyz f32, vel xyz f32` |
| 10 | `damage` | `entityId, attackerId: str, damage: u16, weaponId: str, xyz f32` |
| 23 | `damage` (located) | code 10's fields, then `hitLocation: u8` (0 body, 1 head, 2 legs) |
| 25 | `damage` (pained) | code 23's fields, then `painTier: u8` (0–3) |
| 11 | `death` | `entityId, killerId, weaponId, damageType: str, finalDamage: u16, xyz, vel xyz, kbDir xyz` (all f32) |
| 12 | `award` | `awardType, playerId, playerName, targetName: str, value: u16` |
| 13 | `weapon_chain` | `entityId: str, count: u8`, then `count × xyz f32` |
| 14 | `chat` | `senderId, senderName, team, text: str` |
| 15 | `pickup_notify` | `entityId, label: str` |
| 16 | `voltaic_beam` | `entityId: str`, origin, dir, end, normal (each xyz f32), `impactKind: u8` |
| 17 | `electro_rail` | same layout as 16 |
| 18 | `powerup_state` | `entityId, kind: str, seconds: u16` |
| 19 | `powerup_spawn` | `kind: str, xyz f32` |
| 20 | `agent_directives` | `entityId: str, directives: JSON str` |
| 21 | `lift_state` | `count: u8`, then per lift `modelIndex: u16, offset: f32, phase: u8` (bottom, up, top, down), `remaining: f32, cooldownCount: u8, cooldowns × f32` |
| 22 | `knockback_state` | `entityId: str, ageTicks: u8` (255 means no drag) |

Events are not delta-compressed, and a one-tick event rides only in the
snapshot that carries it. A dropped snapshot loses its events
(`SnapshotReceiver.ts` logs this). Over WebSocket nothing is dropped unless a
baseline is missing.

The four JSON payloads (`match_state`, `weapons`, `ammo`, `agent_directives`)
gain optional fields often. Rust should read them with `serde_json` into
structs where every field is `#[serde(default)]` and unknown fields are
ignored, mirroring the "absent on an older server" comments in
`GametypeTypes.ts`.

### 4.6 Baselines and acknowledgements

`src/engine/network/client/SnapshotReceiver.ts` and `SnapshotBroadcaster.ts`:

- The client keeps the last 128 decoded snapshots (entities and players only)
  keyed by tick, and drops snapshots whose tick is not newer than the latest.
- It sends `Ack(tick)` only for snapshots it decoded. The server deltas against
  an acknowledged snapshot.
- The server forces a full snapshot at least every 64 snapshots per client
  (`FULL_SNAPSHOT_INTERVAL = 64`), so a lost baseline heals within about a
  second at 62.5 Hz.
- History belongs to the connection, not the player's life: clear it on
  reconnect only.

### 4.7 Commands

`BinaryProtocol.encodeCommand` with the `Input` type byte in front:

```text
[0x83][playerId: str][sequence: u32]
[dirX: f32][dirY: f32][dirZ: f32][pitch: f32][yaw: f32]
[buttons: u16][tick: u32][weaponSlot: u8]
```

Buttons (`src/engine/network/components/InputComponent.ts`): FORWARD 1<<0,
BACK 1<<1, LEFT 1<<2, RIGHT 1<<3, JUMP 1<<4, CROUCH 1<<5, SPRINT 1<<6,
WALK 1<<7, FIRE 1<<8, ALT_FIRE 1<<9, RELOAD 1<<10, USE 1<<11, AUTO_ADS 1<<12
(set when fire and aim are both held with auto-ADS on).

What the client has to get exactly right (`Game.sendInput`,
`FPSController.update`):

- **Rate:** exactly 125 commands per second of real time, from a fixed-step
  accumulator capped at 0.25 s. Time dropped by the cap is never sent.
- **Direction:** world-space and already normalised, built in Babylon's
  left-handed, Y-up frame: forward is `(sin yaw, 0, cos yaw)`, right is
  `(cos yaw, 0, -sin yaw)`.
- **Pitch and yaw:** the camera rotation minus the visual weapon kick.
- **`tick`:** the client's view tick, which is the tick of the last snapshot
  received (`gameHost.currentTick`). The server uses it for lag compensation
  (4.9), so it is not decoration.
- **`weaponSlot`:** sent once on the command after a slot key is pressed, then 0.
- **Stripping:** while the match freezes movement, and while dead, the client
  clears the movement and action buttons itself, so prediction agrees with the
  server. While dead, fire or jump sends `RequestSpawn` once the cooldown has
  passed.
- Over a lossless transport each command is sent once. Over datagrams the last
  few are re-sent with each new one (`COMMAND_REDUNDANCY`), and the server skips
  sequences it has seen (`MessageCodec.peekCommandSeq`).

### 4.8 Clock sync

`src/engine/network/client/ClockSync.ts`: simplified NTP. `Ping` carries the
client's `Date.now()`, `Pong` echoes it with the server's `Date.now()`. Offset is
`serverTime - (send + receive) / 2`. A burst of five pings 100 ms apart at
join, one every 30 s after, a rolling window of 16 samples with the slowest
round trips dropped. The snapshot header's `serverTimestamp` is converted with
this offset for interpolation. Rust: `SystemTime` milliseconds as `f64` for the
wire, `Instant` for everything local.

### 4.9 What lag compensation expects from the client

The client does no lag compensation itself; it only has to report its view tick
honestly. On the server, `InputSanitizer.ts` clamps `viewTick` to
`max(0, cmd.tick | 0)`, `GameServer.ts` caps it at the current tick, and
`WeaponSystem.rewindFor` rewinds to `viewTick - interpTicks`, where
`interpTicks` is one snapshot interval (two ticks at 125/62.5 Hz). The rewind
budget is 100 ms (`src/engine/server/lagcomp/config.ts`). The background is in
[v0.9.26-lagcomp-rewind.md](v0.9.26-lagcomp-rewind.md) and SHAG's
`docs/research/lag-compensation-deep-dive.md`.

So a Rust client whose interpolation delay differs from the browser's
(`SnapshotBuffer` settles on the mean plus one standard deviation of arrival
intervals) would have its shots judged against a slightly wrong world. Port
`SnapshotBuffer` as it is, and check it with `npm run net:viewlag`
(`cli/measure-viewlag.ts`), which measures the view lag the server observes.

### 4.10 The lobby protocol

`src/lobby/types.ts`: JSON text frames, `{ "type": "...", ...payload }`.
Clients send `auth` (with a key, or without one to create an account named
`Player_<number>` whose key comes back once in `welcome`), then `queue:*`,
`lobby:*`, `chat:*`, `identity:*`, `settings:*`, `leaderboard:get`,
`stats:get`, `demos:list`, `featured:get` and `match:seat_reissue`. The lobby
answers with `welcome`, `lobby:*`, `matches:update`, `player:*`, `chat:message`
and the matching `*:page`, `*:error` and `*_response` messages.

The message that matters for joining a match is `lobby:connect`:
`{ matchId, serverUrl, map, gametype, admissionTicket }`. The ticket is a
private, single-use seat credential that goes into the game server's `Join` (or
`JoinGame` when dropping into a running match). If it expires before use,
`match:seat_reissue` gets a new one.

In Rust this is an `enum LobbyMessage` with `#[serde(tag = "type")]` and one
variant per message, generated by hand from `types.ts` (728 lines). Requests
that expect an answer carry a `requestId`, as `LobbyClient.ts` does today.

### 4.11 How to prove the Rust decoder right

There is a ready-made corpus: `.shagdemo` files store raw snapshots (no
envelope, no delta) per frame with an index table
(`src/shared/demo/DemoReader.ts`; zlib via `flate2` in Rust). Milestone 1:

1. A small TypeScript tool that reads a demo with `DemoReader` and writes each
   decoded frame as canonical JSON.
2. A Rust tool that decodes the same demo and writes the same JSON.
3. Diff them for every demo on the servers and in `public/data/demos`.

Demos cover full snapshots only. For deltas, add a TypeScript script that
encodes pairs of real frames with `MessageCodec.encodeSnapshot(state, baseline)`
and saves the bytes with the expected decode, then replay those in Rust tests.
The `protocol-change` skill already lists every place a snapshot change touches;
add "regenerate the Rust golden vectors" to it when the Rust client exists.

---

## 5. Layer 2: prediction, reconciliation, interpolation and lag compensation

### 5.1 Local player: `V2NetworkClient`

`client/V2NetworkClient.ts` (1,071 lines) is the heart of "feels the same as
the browser". It runs the same `FPSMovementProfile` as the server
(`simulation/MovementProfile.ts`), keeps every command in `InputHistory`, and on
each snapshot:

1. Reads the server state for the local entity: position, velocity, grounded,
   knockback age (event 22) and lift states (event 21), plus `ackedInputSeq`
   from the header.
2. If the stored prediction for that sequence is within 0.01 units on position
   and velocity, has the same grounded flag, knockback age and lifts, it is
   **confirmed** and nothing is replayed (`confirms()`, top of the file).
3. Otherwise it resets to the server state and replays the unacknowledged
   commands, at most two seconds of them.
4. The visible difference is stored as a visual offset that decays, so a
   correction does not teleport the camera. Snaps of 128 units or more count as
   placements and may turn the view to a spawn facing.
5. Ledge grab and wall jump replays can disagree at sub-tick trace points, so
   some snapshots are suppressed, with hard limits (500 ms of continuous
   suppression, 400 units of divergence).

The constants and rules are all at the top of the file and translate as they
are. The confirmed count on the net graph is the best single parity metric for
the whole Rust simulation: if Rust movement matches the server's, almost every
snapshot confirms.

`NetworkGameHost` still has an older prediction path (`reconcilePrediction`,
`SmoothCorrection`) used when `useExternalPrediction` is false. Washoi always
uses V2, so the Rust port skips the old path.

### 5.2 Remote players: `SnapshotBuffer` and `RemotePlayerLogic`

`src/engine/network/client/SnapshotBuffer.ts` (591 lines) buffers snapshots with
their local arrival time and renders at `now - interpDelay`, interpolating
between the two snapshots around that time and extrapolating from velocity when
the buffer runs dry. The delay follows arrival jitter. Booleans, keys, powerup
mask, water level and ladder are stepped, never interpolated (comments on
`EntitySnapshot`). `client/RemotePlayerLogic.ts` (234 lines) is pure and ports
directly. `client/RemotePlayerRenderer.ts` (2,162 lines) mixes logic with
Babylon meshes and is split during the port: state and animation selection
stay, drawing moves to the renderer.

---

## 6. Layer 3: shared simulation and collision

### 6.1 What runs on both sides

| Piece | Files | Lines |
| --- | --- | ---: |
| Movement profile and sub-systems | `simulation/*.ts` (MovementProfile, Acceleration, Slide, WallJump, LedgeGrab, Swim, Ladder, Knockback, MovementConfig) | 3,135 |
| Liquid sampling | `liquid/LiquidVolumeSampler.ts` and friends | part of 1,637 |
| Collision world | `src/shared/collision/{CollisionWorld,SharedCollisionWorld,WalkFlood}.ts` | 603 |
| Q3 brush trace | `src/engine/physics/SweepCollisionBackend.ts`, `FacetBevels.ts`, `PatchTessellator.ts`, `CollisionSpatialIndex.ts` | about 2,200 |
| Lifts, triggers, spawn points, brush models | `src/shared/bsp/*.ts` | 1,226 |
| Input sanitizing, components, physics constants | `src/shared/server/simulation` | part of 2,209 |
| Movement cvars | `src/engine/movement/MovementVars.ts` | part of 1,193 |

The client wraps the Q3 sweep backend in the same `SharedCollisionWorld` the
server uses (`simulation/collision/BSPCollisionWorld.ts`). The backend imports
Babylon only for `Vector3` (`SweepCollisionBackend.ts:13`), so in Rust it
becomes plain `glam::DVec3` code with no rendering dependency, which also lets
the Rust crate run headless from day one.

### 6.2 Determinism notes

- The simulation runs in double precision on both sides; the wire carries
  `f32`. Rust must simulate in `f64` (`DVec3`), not `f32`.
- JavaScript `Math.sin` and `Math.cos` in V8 come from an fdlibm port. Rust's
  `f64::sin` calls the platform C library, which differs between glibc, macOS
  and Windows in the last bit. The `libm` crate is a port of musl's libm, which
  also descends from fdlibm, so it is the better match. This is an assumption
  to confirm with the parity harness below, not a guarantee. The 0.01-unit
  confirm epsilon gives room, but differences can grow over long airborne
  sequences.
- The simulation uses `Math.sin` 18 times, `Math.cos` 15, `Math.sqrt` 15 (exact
  in IEEE, safe), `Math.abs`, `min`, `max`, `round` and `floor`. `Math.round`
  in JavaScript rounds half up toward positive infinity; Rust's `f64::round`
  rounds half away from zero, so `-2.5` gives `-2` in one and `-3` in the
  other. Use one `js_round` helper that follows the ECMAScript rule wherever
  the TypeScript uses `Math.round` (the naive `(x + 0.5).floor()` is wrong for
  `0.49999999999999994`).
- The only `Math.random` is spawn selection in `FPSSimulation.ts:139`, which is
  server business.
- Movement constants live in cvars (`MovementVars`, `sv_*` and `cl_*`). Most
  are developer-only, so a client never changes the simulation values in
  practice. Check this list in round 3.

### 6.3 Parity harness

SHAG already replays scripted input headlessly:
`npm --prefix src/games/fps run cli:game -- --exec 'sim replay "W+CROUCH 60; JUMP 1; W 30"; quit'`.
Round 3 should extend it to dump position, velocity and movement state per tick
as JSON, then run the same script through the Rust simulation on the same map
and diff tick by tick. Do this for every movement technique in SHAG's
`docs/research/MOVEMENT_TECHNIQUES.md` on the competitive maps.

---

## 7. Layer 4: assets and map formats

### 7.1 Where assets come from

PK3 files (zip archives) are not in any repository. The browser fetches
`data/pk3_manifest.json`, then the content-addressed index it names
(`pk3_index.<sha256>.json`), checks the index's hash and version
(`PK3_INDEX_VERSION = 2`, `src/engine/loader/pk3/PK3PublishedIndex.ts`) and
downloads packages on demand (`PackageDownload.ts`, `PackageStore.ts`), with an
optional mirror (`VITE_PK3_MIRROR_URL`). In production the data is published
next to the game build (`deploy-eu.sh` rsyncs `dist/data/` into the game
release); the exact public URL is to be confirmed in round 4.

A native client does the same over HTTPS (`reqwest`), stores packages in the
user's cache directory (`directories`), and keeps the index hash check. The pk3
lookup order (later packages override earlier ones) is in `PK3Loader.ts` and
`PK3PackageScope.ts` and translates directly; `jszip` becomes the `zip` crate.

### 7.2 Formats

| Format | Used for | TypeScript | Rust |
| --- | --- | --- | --- |
| IBSP (Q3) and FBSP (Warsow) | maps | `loader/bsp/BspLoader.ts` (1,246 lines, no imports but `devLog`) | direct translation over `&[u8]` |
| Q3 `.shader` scripts | materials | `loader/shader/Q3ShaderParser.ts` (1,149 lines, pure) | direct translation |
| MD3 + `.tag` files | items, older models | `loader/model/MD3Loader.ts`, `TagsLoader.ts` | parse by hand (small format) |
| glTF `.glb` | player models, weapons, effects (175 references) | Babylon glTF loader | `gltf` crate |
| TGA | textures | `loader/texture/TgaDecoder.ts` | `image` |
| PNG, JPEG, WebP | textures, levelshots | browser decode | `image` |
| KTX2 (ETC1S, UASTC) | texture sidecars (`r_ktx2`) | Babylon KTX2 decoder worker | `ktx2` + a Basis Universal transcoder to BCn or ASTC |
| KTX1 | some textures | `KTX1Loader.ts` | `ktx` or by hand |
| OGG Vorbis, WAV, MP3 | sounds and music | Web Audio decode | `symphonia` |
| `.shagdemo` | demos | `src/shared/demo/DemoReader.ts` | direct, with `flate2` |
| Playlists, entity defs, prefabs | JSON | various | `serde_json` |

Coordinates: Q3 is Z-up and right-handed, Babylon is Y-up and left-handed, and
SHAG converts once at the loader boundary as `(x, y, z) → (x, z, y)` with
triangle winding reversed (`loader/bsp/mesh/coordinates.ts`). The server works
in the converted frame too, so every position on the wire is in Babylon's frame.
**The Rust client should keep Babylon's frame everywhere** (glam's `_lh`
projection and view functions) instead of converting back to Q3 or to a
right-handed frame. That keeps the wire, the simulation and the translated code
identical and removes a whole class of sign bugs.

---

## 8. Layer 5: rendering

This is the largest single job. The Babylon renderer cannot come along, so the
work is a purpose-built `wgpu` renderer for exactly what Washoi draws.

### 8.1 What the current renderer does

From `src/engine/loader/bsp/BspMeshBuilder.ts` (2,222 lines), the shader
plugins, and SHAG's `docs/research/RENDERING_PIPELINE.md`:

- BSP faces, patches (tessellated), and triangle soups built into batched
  meshes, with spatial batching and PVS culling (`PVSCuller.ts`,
  `loader/bsp/mesh/pvs.ts`).
- Baked lighting: lightmap pages packed into array layers, deluxe maps, vertex
  lighting, the light grid for models (`BspBakedLightingPlugin.ts`,
  `BspVertexLightingPlugin.ts`, `LightGridSampling.ts`).
- Q3 shader stages (`Q3ShaderTypes.ts`): `blendFunc`, `alphaFunc`, `rgbGen`,
  `alphaGen` with waves and distance ramps, `tcMod` (scroll, scale, rotate,
  stretch, turb, transform), `tcGen` (base, lightmap, environment, vector),
  `animMap`, `clampMap`, `depthWrite`, `depthFunc`, `detail`, cube maps, and
  conditional blocks. Animation runs per frame (`ShaderAnimationSystem.ts`) or
  analytically in the shader (`Q3AnalyticAnimationPlugin.ts`).
- Autosprite and autosprite2, soft particles, glow overlays, sky portals and
  sky clouds, fog volumes and atmospheric fog, water with reflections
  (`liquid/`, `src/engine/rendering/water`).
- Dynamic lights from weapons and effects (`lighting/`, `weapon/PointLightPool.ts`),
  clustered on Babylon 9.
- Models: MD3 with tags, skinned glTF with animation groups and blending
  (`animation/`), weapon view model on its own camera.
- Effects: explosions, impacts, muzzle flashes, tracers, rail and voltaic beams,
  gibs, damage numbers, cartoon hit glow, teleport, quad glow, outlines.
- Post-processing (`docs/reference/post-processing.md`): cel outline, optional
  SSR, and others behind `r_*` cvars (`docs/reference/RENDER_VARS.md`).

### 8.2 The Rust renderer

- `wgpu` on Vulkan, Metal and DX12, presented through `winit`.
- WGSL throughout. Start from the WGSL branches already in the material plugins:
  they hold the exact lightmap layer addressing (lightmap U carries
  `u + 2 * layer`), stage colour, autosprite and fog math.
- One pipeline per Q3 stage blend and depth combination, cached by key. Q3
  shaders are data, so the Rust side builds a small stage program per shader at
  load time instead of Babylon's plugin injection.
- A fixed pass list: depth prepass, opaque world, sky, models, translucent
  world and effects (sorted), view model, post-processing, HUD. SHAG's
  `docs/research/babylon-renderer-new-paths-2026-10-26.md` already argues for a
  PVS-driven retained world draw set; that is the natural design here.
- Ragdolls and gibs use Havok through Babylon today (`client/RagdollSystem.ts`);
  `rapier3d` replaces it on the client only.

The groundwork studies for Babylon Lite
(`docs/research/babylon-lite-*.md`, `babylon-classic-to-lite-groundwork-plan-2026-10-26.md`)
already list which renderer data can be separated from Babylon (MD3 and patch
data, visibility queries, weapon motion, input frame ownership, settings
definitions). Most of those packages apply unchanged to a Rust renderer, and
round 5 should start from them.

### 8.3 Visual parity

Use the existing renderer QA probes as references: screenshot the browser
client at fixed camera positions on each map (the `browser-probe` skill), render
the same positions in Rust, and compare. The qfusion parity notes
(`v0.9.30-qfusion-lighting-parity.md`, `v0.9.30-qfusion-material-passes.md`)
describe the intended look.

---

## 9. Layer 6: audio

Today: Babylon AudioV2 on Web Audio (`src/engine/audio/AudioV2Sfx.ts`,
`src/engine/AudioManager.ts`, 1,163 lines) with an SFX bus, cached decoded
buffers read from PK3s, a cap on simultaneous instances, spatial sounds with the
inverse distance model (`spatialDistanceModel: "inverse"`, reference and far
distances and a rolloff from an attenuation preset), music playlists
(`MenuPlaylist.ts`, `data/*.playlist.json`), and about 1,050 lines of
game-specific players (`client/FPS*Audio.ts`, `FPSAnnouncer.ts`). Sounds are
OGG (135 references), WAV (41) and MP3 (6).

Rust: `kira` for mixing, buses, tweens and spatial tracks, with `symphonia`
decoding. Kira's spatial model differs from Web Audio's, so port the inverse
distance gain formula from Web Audio's `PannerNode` by hand and apply it as a
volume, with simple equal-power panning, to keep the same falloff. Check
Babylon's default panning model for AudioV2 spatial sounds in round 6 before
choosing between equal-power and HRTF. The `FPS*Audio` classes are event
handlers and port directly.

---

## 10. Layer 7: input, console, cvars and settings

- **Input:** `src/engine/input/InputSystem.ts` is action-based, with bindings
  stored as DOM `KeyboardEvent.code` strings ("KeyW", "ArrowUp", "Space").
  winit's `KeyCode` is modelled on the same W3C code list, so the bindings and
  saved configs carry over through a name table. Mouse look comes from
  `movementX/Y` under pointer lock; natively it comes from winit's raw
  `DeviceEvent::MouseMotion`, without browser acceleration or pointer lock
  quirks (see SHAG's `linux_mouse_hold.md` and
  `v0.9.31-fullscreen-keyboard-lock.md`). Sensitivity needs one calibration so a
  degree per count matches the browser.
- **Console and commands:** `src/engine/console` (78 files) is a registry with
  commands discovered from `*Command.ts` files. Rust has no file discovery, so
  registration becomes an explicit list (or the `inventory` crate). About half
  the command files touch Babylon (debug and render commands); gameplay and
  network commands port directly.
- **Cvars:** `src/engine/core/Cvar.ts` persists to `localStorage`. Natively,
  write a JSON or TOML file in the config directory, and keep the same keys
  (`engine.movementVars` and so on) so a later import from the browser is
  possible. Settings sync through the lobby (`settings:get`, `settings:save`,
  `src/lobby/SettingsSync.ts`) works unchanged once the lobby client exists.

---

## 11. Layer 8: HUD, menus and lobby UI

All UI is HTML and CSS today: 27 HUD widgets (`client/hud/widgets`), the lobby
(`client/LobbyPanel.ts`, `LobbyRooms.ts`, `LobbyWelcomeDialog.ts`), settings
pages (`client/settings`), scoreboard, chat, demos, leaderboard, profile,
credits, welcome and terms dialogs, and `src/engine/ui`. Every widget implements
`FPSHudWidget` (`getElement`, `update(state)`, `show`, `hide`, `destroy`) and
reads one `HUDState`.

Recommendation: `egui` (with `egui-wgpu` and `egui-winit`) for menus, lobby,
settings and the console, and the HUD drawn by the game renderer's own 2D batch
(text, quads, icons), because the HUD sits over the 3D view every frame and must
look exactly like the browser HUD. Keep the widget interface: each widget
becomes a struct with `update(&HudState)` and `draw(&mut Hud2d)`, so the file
list stays one to one.

The alternative is an embedded web view (`wry`) that reuses the existing HTML,
CSS and TypeScript for the lobby and menus. It saves most of the UI rewrite but
brings a browser engine back into the native client, two languages to debug,
and fragile compositing over a `wgpu` surface on some platforms. Worth a spike
in round 8; not the default.

---

## 12. Module to crate map

`shagger` and `shagger_shared` are the SHAGGER engine, the Rust counterpart of
SHAG; `washoi` is the game. Section 14 sets out which side each piece is on.

| SHAG / Washoi module | Rust module | Main crates | Translation |
| --- | --- | --- | --- |
| `src/shared/net/BinaryProtocol.ts` | `shagger_shared::net::binary` | (none) | 1:1 |
| `src/shared/net/{GameState,Entity,Component}.ts` | `shagger_shared::net::state` | (none) | 1:1, typed structs replace component maps |
| `src/engine/network/protocol/MessageCodec.ts` | `shagger::network::protocol` | (none) | 1:1 |
| `src/engine/network/client/{SnapshotReceiver,SnapshotBuffer,ClockSync,ConnectionWatchdog,NetGraph}.ts` | `shagger::network::client` | (none) | 1:1 |
| `src/engine/network/client/hosts/NetworkGameHost.ts` | `shagger::network::host` | `tokio`, `crossbeam-channel` | 1:1 minus the legacy prediction path |
| `src/engine/network/client/transport/*` | `shagger::network::transport` | `tokio-tungstenite`, `rustls`, `webpki-roots`; later `quinn` or `wtransport` | trait 1:1 |
| `src/lobby/LobbyClient.ts`, `types.ts` | `shagger::lobby` | `serde`, `serde_json` | 1:1 |
| `src/shared/server/*` | `shagger_shared::server` | `serde` | 1:1 |
| `src/shared/{bsp,collision,entities,demo}` | `shagger_shared::{bsp,collision,entities,demo}` | `flate2` | 1:1 |
| `src/engine/physics/{SweepCollisionBackend,FacetBevels,PatchTessellator,CollisionSpatialIndex,ContentFlags}.ts` | `shagger::physics` | `glam` (f64) | 1:1 |
| `simulation/`, `liquid/LiquidVolumeSampler.ts` | `washoi::simulation`, `washoi::liquid` | `glam`, `libm` | 1:1 |
| `shared/` (Washoi entities, items, bsp helpers) | `washoi::shared` | `serde` | 1:1 |
| `client/V2NetworkClient.ts`, `InputHistory.ts`, `RemotePlayerLogic.ts` | `washoi::client::net` | (none) | 1:1 |
| `src/engine/core/{EventBus,GameTimer,Cvar,GamePaths}.ts` | `shagger::core` | `directories`, `serde` | adapted |
| `src/engine/loader/pk3/*` | `shagger::loader::pk3` | `zip`, `reqwest`, `sha2` | 1:1 logic, new I/O |
| `src/engine/loader/bsp/BspLoader.ts` | `shagger::loader::bsp` | `bytemuck` | 1:1 |
| `src/engine/loader/bsp/BspMeshBuilder.ts`, `mesh/*` | `shagger::loader::bsp::mesh` | `glam` | logic 1:1, output becomes GPU buffers |
| `src/engine/loader/shader/Q3ShaderParser.ts`, `Q3ShaderTypes.ts`, `Q3StageEvaluation.ts` | `shagger::loader::shader` | (none) | 1:1 |
| `src/engine/loader/shader/*Plugin.ts`, `Q3ShaderMaterialFactory.ts` | `shagger::render::q3` | `wgpu` | rewritten, WGSL math reused |
| `src/engine/loader/texture/*` | `shagger::loader::texture` | `image`, `ktx2`, Basis Universal transcoder | replaced |
| `src/engine/loader/model/*` | `shagger::loader::model` | `gltf` | MD3 1:1, glTF by crate |
| `src/engine/{rendering,lighting,sprites}` | `shagger::render` | `wgpu` | rewritten |
| `src/engine/effects`, `src/engine/weapon` | `shagger::{effects,weapon}`, Washoi weapon effects in `washoi::weapons` (14.2) | `wgpu` | logic 1:1, drawing rewritten |
| `src/engine/animation` | `shagger::animation` | `gltf` | rewritten |
| `src/engine/{camera,viewport}` | `shagger::{camera,viewport}` | `glam` | mostly 1:1 |
| `src/engine/input` | `shagger::input` | `winit` | adapted |
| `src/engine/AudioManager.ts`, `src/engine/audio` | `shagger::audio` | `kira`, `symphonia` | adapted |
| `src/engine/console` | `shagger::console` | (none) | registry 1:1 |
| `src/engine/ui`, `client/hud`, `client/settings`, lobby views | `shagger::ui`, `washoi::client::hud`, `washoi::client::ui` | `egui`, `egui-wgpu`, `egui-winit` | rewritten |
| `client/RagdollSystem.ts`, `GibSystem.ts` | `washoi::client::ragdoll` | `rapier3d` | rewritten |
| `Game.ts`, `FPSPlayer.ts`, `camera/FPSController.ts`, `world/` | `washoi::game`, `washoi::camera`, `washoi::world` | | logic 1:1 |
| `client/main.ts` | `washoi` binary `main.rs` | `winit`, `tracing` | adapted |

Supporting crates: `tracing` for logs (`devLog` maps to a `developer` filter,
`debugLog(topic)` to targets), `anyhow` and `thiserror` for errors, `puffin` or
`tracy-client` for profiling.

Versions are not pinned here on purpose: pin the current releases of `wgpu`,
`winit`, `egui` and `kira` at the start of milestone 4, because these four move
quickly and must agree with each other.

---

## 13. Translating TypeScript patterns to Rust

| TypeScript in SHAG | Rust |
| --- | --- |
| `X.getInstance()` singletons (`AudioManager`, `CommandRegistry`, `MatchStateManager`, `HUDState`, `CollisionManager`, ...) | One `Client` context struct owns the systems; functions take `&mut` to what they need. Global singletons fight the borrow checker and hide the order that matters. |
| `EventBus` with unsubscribe handles | Typed event queues drained once per frame (`Vec<GameEvent>`), or `crossbeam-channel` across threads. The "always unsubscribe" rule disappears with ownership. |
| Babylon `Observable` | Same as above: an event queue or a returned list of changes. |
| Component maps keyed by string (`entity.getComponent("transform")`) | Typed structs with `Option` fields (see 4.5). |
| Optional fields "absent on an older server" | `Option<T>` and `#[serde(default)]`. |
| String ids for entities and players | An interned id (`Arc<str>`, or a `u32` from a per-connection interner) so maps hash cheaply. The wire stays strings. |
| Duck typing (`typeof sim.spawnPlayer === "function"`) | Traits, or just the concrete type: the Rust client has one simulation. |
| Promises, `background(...)`, `setInterval` | The network and downloads run on a `tokio` runtime; the frame loop polls channels. Timers become checks against the frame clock. |
| Service registries (`GameplayMapLauncher`, `NetworkConnector`, `GameFrameHook`, ...) | Traits implemented by the `washoi` crate and passed to `shagger` at startup, which keeps the "engine must not import the game" rule as a crate dependency rule the compiler enforces. |
| `performance.now()`, `Date.now()` | `Instant`, and `SystemTime` milliseconds as `f64` only where the wire needs them. |

The last row deserves a note: SHAG already separates engine and game by
convention and by an audit script (`docs/research/engine-game-split-*.md`). In
Cargo, `shagger` simply cannot depend on `washoi`, so the rule becomes free.

---

## 14. Proposed workspace: the SHAGGER engine and Washoi

SHAG gets a Rust counterpart named **SHAGGER**. Everything that is rendering,
core or otherwise not specific to Washoi goes into SHAGGER; Washoi is a game on
top of it, exactly as wash-fps sits on SHAG today. Folder names inside each
crate follow the TypeScript, so a SHAG path points straight at its SHAGGER file.

The code lives in two repositories, mirroring SHAG and wash-fps:

| Repository | Holds | TypeScript counterpart |
| --- | --- | --- |
| [`Higher-Standard-Games/shagger`](https://github.com/Higher-Standard-Games/shagger) | the engine: `shagger_shared`, `shagger` (with `src/babylon/`, 21.1), engine tools | `shag` |
| [`Higher-Standard-Games/wash-fps-rust`](https://github.com/Higher-Standard-Games/wash-fps-rust) | the game: `washoi`, the client binary, game tools | `wash-fps` |

```text
shagger/                         engine repository, its own Cargo workspace
├─ Cargo.toml                    workspace
├─ crates/
│  ├─ shagger_shared/            ← src/shared        (no GPU, no window, no async)
│  │  └─ src/{net,server,bsp,collision,entities,demo,settings}/
│  └─ shagger/                   ← src/engine (minus editor and server) + src/lobby/LobbyClient.ts
│     └─ src/{core,network,lobby,physics,movement,loader,render,lighting,
│             effects,sprites,animation,camera,viewport,input,audio,console,ui,babylon}/
├─ crates/*/tests/golden/        reference outputs made by the TypeScript tools, checked by cargo test
└─ tools/golden/                 Node scripts that run SHAG's own code to make those outputs

wash-fps-rust/                   game repository
├─ Cargo.toml                    depends on shagger by git rev
├─ src/                          ← wash-fps, client side
│  └─ {simulation,liquid,shared,world,camera,console,client,weapons,
│      game.rs,fps_player.rs,main.rs}
└─ tests/                        parity tests against sim:dump output and demo dumps
```

**Tooling stays in TypeScript.** The Rust repositories hold the client and
its tests, and nothing else (decided 2026-10-31). Every tool that the client
does not strictly need stays in, or is added to, SHAG and wash-fps:

- **Already in TypeScript, not ported:** packaging, encoding, the pk3 index
  build, the editor, `demo-verify`, the server and bots.
- **New TypeScript tools for parity:** the demo and delta dumps (18.6),
  `sim:dump` (19.7), `shader:audit` (21.7), and the offline audio render
  (22.8). They run SHAG's real code and write reference files.
- **On the Rust side:** only `cargo test` targets that read those files.
  There are no separate Rust tools. The headless client of milestones 2 and 3
  is the client itself, started with `--headless`.

**How the two repositories connect.** SHAG includes wash-fps as a submodule;
the Rust side turns that around, as Cargo expects. `wash-fps-rust` depends on
`shagger` by git revision:

```toml
shagger = { git = "https://github.com/Higher-Standard-Games/shagger", rev = "..." }
```

A developer working on both clones them side by side and adds a local
`[patch]` in an untracked `.cargo/config.toml`. CI builds `wash-fps-rust`
against the pinned revision, and also against `shagger`'s `main` branch, which
shows engine changes that break the game before the pin moves.

The golden vectors (18.6) and the `sim:dump` harness (19.7) are produced by
TypeScript in SHAG and wash-fps. Each Rust repository pins the TypeScript
commit it was checked against, under `reference/`.

Dependency rule, enforced by Cargo: `washoi → shagger → shagger_shared`, never
the other way. SHAG keeps "engine must not import the game" by convention and an
audit script (`docs/research/engine-game-split-*.md` in SHAG); in SHAGGER the
compiler keeps it.

### 14.1 What goes where

| SHAGGER (`shagger`, `shagger_shared`) | Washoi (`washoi`) |
| --- | --- |
| Binary reader and writer, message framing, snapshot envelope, baselines, acks, clock sync, watchdog, resume, transports | Washoi's event catalogue and `match_state` content (see 14.2) |
| Generic game state: entities, players, components, pickup mask | Entity id conventions (`player_`, `proj_`, `item_`) and what each entity means |
| Lobby client and lobby message types (SHAG owns `src/lobby`) | Lobby screens, rooms, welcome and terms dialogs |
| Collision world, Q3 brush trace, patches, lifts, triggers, BSP tables | Movement profile and its parts (slide, wall jump, ledge grab, swim, ladder, knockback), liquid sampling |
| PK3, published index, downloads, BSP, Q3 shaders, textures, MD3, glTF, sounds | Washoi data: entity defs, weapon catalogue, playlists |
| Renderer: world, Q3 stages, lighting, sky, fog, water surface, sprites, particles, post-processing | Washoi visuals built from those parts: weapon effects (rail, voltaic, blade), gibs, quad glow, cartoon hit glow |
| Skeletal animation, model instancing | Player model choice and animation states |
| Camera controllers, first-person view effects | `FPSController`, spectating, death camera |
| Input actions and bindings, console, cvars, settings storage | Washoi's action map (`FPSActions`), commands, settings pages |
| Audio engine, buses, spatial falloff, music playlists | `FPS*Audio`, announcer |
| UI toolkit: 2D HUD batch, egui integration, theme, fonts, loading screens | The 27 HUD widgets, scoreboard, menus |

### 14.2 Where SHAGGER should not copy SHAG one to one

A few things live in SHAG today but are Washoi's in substance. The plan keeps
the code one to one and only moves the file to the side it belongs to:

- **The snapshot event list.** `BinaryProtocol.ts` hardcodes Washoi's 25 event
  codes (weapon fire, voltaic beam, bomb-era `agent_directives` and so on), and
  `MatchStateSnapshot` carries the bomb objective. In SHAGGER the snapshot codec
  calls an `EventCodec` trait for the event section; Washoi implements it with
  the exact layouts of section 4.5 and 18.2. The bytes on the wire do not change.
- **`src/engine/weapon` (12,331 lines).** The framework (view model, bob, recoil,
  switch and reload motion, barrels, point light pool, effect geometry) is
  generic and goes to SHAGGER. The effects named after Washoi weapons
  (`rail`, `voltaic`, `WeaponSlash`, `BladeEnvironment`) go to `washoi::weapons`.
- **`src/engine/animation`.** The blender and rig code is generic; the humanoid
  player rig and player model assets are Washoi's.

SHAG's own split log (`ENGINE_GAME_SPLIT_*.md`) lists more of these; round 9
checks each against it.

`shagger_shared` stays free of GPU and async code so it can later serve a Rust
game server or a WebAssembly build without changes.

Where the code lives is still open (section 16). A separate repository keeps the
Rust toolchain and CI apart from the TypeScript gates, and pins the TypeScript
it was translated from as a submodule, the same way SHAG pins wash-fps today.
SHAGGER could later get its own repository, as SHAG has, once a second game uses
it.

---

## 15. Milestones and estimates

Estimates are engineer-weeks for one developer who knows Rust and this codebase,
including tests. They are round-1 guesses from line counts and coupling, not
measured; each later round should revise its own layer.

| # | Milestone | Proves | Weeks |
| ---: | --- | --- | ---: |
| 0 | Workspace, CI, reference checkout, TypeScript golden-vector scripts | the harness exists | 1 |
| 1 | `shagger_shared::net` + `MessageCodec`: decode every demo and every delta vector identically | the wire format | 2 |
| 2 | Headless client: lobby auth, `lobby:connect`, game handshake, acks, clock sync, watchdog, resume; joins as spectator and prints the scoreboard | live servers accept us | 2–3 |
| 3 | BSP parse, collision, lifts, triggers, liquid, the full movement profile; the `sim:dump` parity tests clean on all maps | the simulation matches | 4–6 |
| 4 | `V2NetworkClient`, `InputHistory`, `SnapshotBuffer`; headless client plays with scripted input and mostly confirms | prediction matches the server | 2 |
| 5 | PK3 index and downloads, cache, textures (incl. KTX2), Q3 shader parser, MD3, glTF, sounds | assets load natively | 3–4 |
| 6 | Window, input, camera; renderer v1: world with lightmaps, opaque and blended Q3 stages, sky, PVS | you can fly around a map | 6–8 |
| 7 | Players and weapons: skinned models and animation, view model, projectiles, hitscan and beam effects, explosions, dynamic lights | a match is watchable | 6–8 |
| 8 | Audio: SFX, spatial falloff, music playlists, announcer, all `FPS*Audio` | a match sounds right | 2–3 |
| 9 | Minimal HUD (vitals, ammo, crosshair, frags, timer, chat, scoreboard) and a minimal lobby screen | **playable MVP** | 3–4 |
| 10 | Full lobby, settings, console, cvars, settings sync, profile, leaderboard | the menus are complete | 6–8 |
| 11 | Remaining visuals: water, fog volumes, soft particles, glow, post-processing, gibs and ragdolls, quad glow, outlines | visual parity | 6–10 |
| 12 | Demo playback, highlight reels | demos | 3–4 |
| 13 | Packaging, signing, updates, crash reporting for Windows, macOS, Linux | shippable | 2–3 |

- **Playable MVP (milestones 0–9):** about 31 to 41 weeks as listed; with
  milestones 6 and 7 cut to "world plus coloured capsules for players, basic
  effects", about 25 weeks.
- **Parity without the editor (0–13):** about 48 to 66 weeks, plus about 15 per
  cent for the unknowns every port has, which lands at the 55 to 75 weeks in
  section 1.
- Work split for two people: one on milestones 1–4 (protocol and simulation),
  one on 5–7 (assets and renderer), meeting at 9.

---

## 16. Risks and things to settle

1. **No protocol version (high).** A native client released on its own schedule
   can misread a changed snapshot. Smallest fix, in the style the codec already
   uses: an optional trailing `protocolVersion: u16` on `Join` and on `Welcome`,
   absent meaning "browser, same build". The server can then refuse a stale
   native client with a clear `ServerMessage`. This is a server change and
   needs the `protocol-change` checklist.
2. **Simulation parity (high).** Covered by milestone 3's harness and the libm
   choice in 6.2. Until it is clean, the native client will feel worse than the
   browser, not better.
3. **Renderer scope (high).** The Q3 shader corpus on the current maps decides
   how much of 8.1 is needed. Round 5 should count the stage features actually
   used by the shipped pk3s, so unused features are not built.
4. **Protocol churn while porting (medium).** The protocol changed often during
   0.9 (codes 23–25, view bytes, lift and knockback state). Golden vectors
   regenerated in CI on every SHAG change will catch drift on the day it happens.
5. **Anti-cheat stance (medium).** A native client is easier to modify than the
   browser build is today. The server is authoritative and validates movement
   (`server/MovementValidator.ts`, `server/AuthorityGuards.ts`), so this mostly
   affects aim and wallhacks. Settle the policy before a public release, not now.
6. **Lobby and terms (low).** Accounts are key plus name; `welcome` carries a
   new key once. The native client must store it as the browser does and show
   the same terms and policy acceptance (`policyVersion`).
7. **Where the code lives (decision).** A new repository with the TypeScript
   pinned as a reference (recommended), or a `native/` folder in wash-fps.

---

## 17. Next rounds

| Round | Layer | Questions to answer |
| --- | --- | --- |
| 2 (done, section 18) | Protocol, function level | Full Rust type definitions for `GameState`, all events and `MatchStateSnapshot`; the golden-vector scripts written out; the lobby message enum; the exact JoinReject and ServerMessage cases the server sends |
| 3 (done, section 19) | Simulation | Each movement file mapped function by function; every cvar that reaches the simulation; the `sim replay` JSON dump format; a check of `libm` against V8 on recorded inputs |
| 4 (done, section 20) | Assets | The production data URL and caching headers; pk3 index entries in detail; which texture and model formats the shipped pk3s actually contain, by count |
| 5 (done, section 21) | Renderer | Q3 stage features used by shipped maps; pass list and pipeline keys; the WGSL fragments lifted into stand-alone shaders; which Babylon Lite groundwork packages apply |
| 6 (done, section 22) | Audio | Babylon AudioV2 defaults (panning model, distances), the attenuation presets, the bus layout |
| 7 | Input and console | Binding name table against winit; which console commands a player can use; cvar file format |
| 8 | UI | HUD widget inventory with each widget's `HUDState` inputs; egui versus web view spike |
| 9 | Game glue | `Game.ts` and `FPSController.ts` split into Rust modules; the frame order written down as code |


---

## 18. Round 2: the protocol at function level

Round 2 reads the server side of every message the client handles, because
several rules that a client has to follow are only visible there. Sources:
`server/SnapshotBuilder.ts`, `server/SnapshotBroadcaster.ts`,
`server/GameServer.ts`, `server/MovementValidator.ts`,
`client/SnapshotStateStore.ts`, `client/MatchStateManager.ts`, SHAG's
`src/engine/network/core/SnapshotEvent.ts`, `src/lobby/*` and
`src/shared/demo/DemoReader.ts`.

### 18.1 Findings that change the client

These were not visible from the codec alone. Each is something a Rust client
would get wrong if it only translated `BinaryProtocol.ts`.

1. **The server leaves out state you already acknowledged.** In
   `SnapshotBroadcaster.leaveOutKnown`, two kinds of "lasting" state are sent
   only until the client acks a snapshot that carried them: other players'
   `weapon_state`, and the scoreboard roster (`standings` and `spectators`)
   inside `match_state`. A `match_state` without the roster carries
   `rosterOmitted: true`. Every 125th snapshot (about 2 s) sends everything
   again as a safety net. So the client **must** keep the last `weapon_state`
   per entity and the last roster for the whole connection, and must store them
   before anything else runs on the snapshot, because the ack has already gone
   out (`client/SnapshotStateStore.ts`, `MatchStateManager.processEvent`).
   Your own `weapon_state` is always sent.
2. **Absent is not the same as removed.** With PVS on, the server drops entities
   the viewer cannot see (`filterStateForClient`). Dead players are left out of
   `entities` entirely but stay in `players` and still get `combat_state`
   (`SnapshotBuilder.ts` around line 250). Always present: your own entity,
   dropped weapons (`item_*`), and teammates while you are a dead S&D player
   spectating. The renderer should treat "not in this snapshot" as "do not draw",
   not as "destroyed with an effect".
3. **The wire carries no entity type.** The type is in the id:
   `player_<n>` (players, `ClientEntityKey.ts`), `proj_<n>` (projectiles),
   `item_<n>` (dropped weapons). Player ids in `players` use the same
   `player_<n>` form. Entity ids are never reused within a match.
4. **Some fields are zeroed for you.** `combat_state` for a player you are not
   allowed to see has `health` and `armor` set to 0; only `dead` is real. You
   see your own, your team's in team modes, and everyone's as a spectator.
   `knockback_state` arrives only for your own entity, chat only if you are a
   recipient, `agent_directives` only for you.
5. **Ping is measured from your acks.** The server times the round trip from
   a snapshot to its `Ack` (`recordAckRoundTrip`). Ack as soon as a snapshot
   decodes, on the network thread, not at the next rendered frame, or your
   ping on the scoreboard reads high. (Lag compensation uses the view tick, not
   this ping.)
6. **Pings are rate-limited.** A `Ping` within 25 ms of the last one gets no
   `Pong` (`PING_MIN_GAP_MS`). The ClockSync burst (100 ms apart) is fine.
7. **Commands are checked.** `sanitizeCommand` rejects non-finite values;
   pitch beyond ±π/2 or a direction longer than 1 is clamped and counted as a
   violation; `MovementValidator` drops commands over a per-tick cap and
   against a 2-second real-time budget. A Rust client must clamp pitch itself
   and normalise the direction exactly as `Game.sendInput` does.
8. **Sequences start at 1 and never go back.** The client's counter starts
   at 1 (`NetworkGameHost.commandSequence`). On `Resume` the server resets its
   own last-seen sequence, so the client simply continues counting.
9. **Joining is two steps.** `Join` always registers you as a spectator with an
   entity id reserved, and answers with Welcome, JoinAccept and ResumeToken.
   Playing needs `JoinGame`. The server sends positions and velocities already
   rounded to `f32` (`SnapshotBuilder.f32`), so its own history matches what
   the client decodes.
10. **`MatchPhase` in JSON is a number**: Warmup 0, Countdown 1, Active 2,
    PostGame 3, RoundEnd 4.

### 18.2 Rust types for the snapshot

Typed structs replace the component maps, keeping the decode rules of 4.5. The
event enum follows `SnapshotEvent` in SHAG one to one (it is the union the
server's `emit` is checked against), plus `lift_state`, which travels outside
that union.

```rust
// shagger_shared::net::state
pub struct GameState {
    pub tick: u32,
    pub server_timestamp: f64,      // server Date.now(), ms
    pub acked_input_seq: u32,       // last command the server SIMULATED for us
    pub entities: IndexMap<EntityId, NetEntity>, // wire order kept: the encoder iterates a Map
    pub players: IndexMap<PlayerId, NetPlayer>,
    pub events: Vec<GameEvent>,     // Washoi's catalogue, see 14.2
    pub pickup_active: Option<BitVec>, // None when the payload had no tail
}

pub struct NetPlayer {
    pub entity_id: EntityId,
    pub name: String,
    pub score: i32,
    pub deaths: u32,
    pub ping: u16,
    pub team: String,               // "udon" | "soba" | "ffa" | "spectator"
}

// washoi::shared::net::events
pub enum GameEvent {
    CombatState { entity_id: EntityId, health: u16, armor: u16, dead: bool },
    MatchState(Box<MatchStateEvent>),
    WeaponState(WeaponState),
    WeaponFire { entity_id: EntityId, weapon_id: String, origin: Vec3, dir: Vec3,
                 pellets: Option<(i32, f32)> },               // code 24
    WeaponCharge { entity_id: EntityId, weapon_id: String, charging: bool },
    WeaponChain { entity_id: EntityId, points: Vec<Vec3> },
    VoltaicBeam(BeamHit),
    ElectroRail(BeamHit),
    ProjectileSpawn { entity_id: EntityId, def_id: String },
    ProjectileImpact { def_id: String, pos: Vec3, normal: Vec3 },
    ItemPickup { pickup_id: i16, weapon_id: String, entity_id: EntityId,
                 dropped_weapon_id: String, pos: Vec3 },
    WeaponDrop { entity_id: EntityId, weapon_id: String, pos: Vec3, vel: Vec3 },
    Damage { entity_id: EntityId, attacker_id: String, damage: u16, weapon_id: String,
             pos: Vec3, hit_location: Option<HitLocation>, pain_tier: Option<u8> }, // 10, 23, 25
    Death { entity_id: EntityId, killer_id: String, weapon_id: String, damage_type: String,
            final_damage: u16, pos: Vec3, vel: Vec3, kb_dir: Vec3 },
    Award { award_type: String, player_id: String, player_name: String,
            target_name: String, value: u16 },
    Chat { sender_id: String, sender_name: String, team: String, text: String },
    PickupNotify { entity_id: EntityId, label: String },
    PowerupState { entity_id: EntityId, kind: String, seconds: u16 },
    PowerupSpawn { kind: String, pos: Vec3 },
    AgentDirectives { entity_id: EntityId, directives: serde_json::Value },
    LiftState(Vec<LiftState>),
    KnockbackState { entity_id: EntityId, age_ticks: Option<u8> }, // 255 on the wire = None
}

pub struct BeamHit { pub entity_id: EntityId, pub origin: Vec3, pub dir: Vec3,
                     pub end: Vec3, pub normal: Vec3, pub impact_kind: u8 } // 0..=2

#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
pub struct WeaponState {
    pub entity_id: EntityId,
    pub current_weapon: String,
    pub weapons: Vec<String>,                       // JSON string on the wire
    pub ammo: HashMap<String, Ammo>,                // JSON string on the wire
    pub reloading: bool,
    pub last_fire_tick: u32,
}
#[derive(Deserialize)] pub struct Ammo { pub clip: i32, pub reserve: i32 }
```

`Vec3` here is `f32` as decoded; the simulation widens to `f64`. `IndexMap`
keeps the server's order, which the golden-vector diff needs and which some
client code (the first player in a list) may rely on.

`match_state` is JSON with every field optional (`Partial<MatchStateSnapshot>`
in `SnapshotEvent.ts`):

```rust
#[derive(Deserialize, Default)] #[serde(rename_all = "camelCase", default)]
pub struct MatchStateEvent {
    pub phase: u8,                       // MatchPhase as a number
    pub time_remaining: f64,
    pub time_elapsed: Option<f64>,
    pub ready_count: u32,
    pub total_players: u32,
    pub gametype_name: String,
    pub score_limit: i32,
    pub team_based: Option<bool>,
    pub team_scores: Option<HashMap<String, i32>>,
    pub player_teams: Option<HashMap<String, String>>,
    pub team_names: Option<HashMap<String, String>>,
    pub standings: Option<Vec<Standing>>,       // absent when roster_omitted
    pub spectators: Option<Vec<SpectatorEntry>>,
    pub roster_omitted: bool,
    pub winner: Option<MatchWinner>,            // { kind: player|team|draw, id?, name? }
    pub overtime: Option<u32>,
    pub loadouts: Option<String>,               // "round" | "respawn"
    pub flow: Option<String>,                   // "single" | "continuous"
    pub round: Option<u32>,
    pub team_roles: Option<HashMap<String, String>>,
    pub round_state: Option<String>,
    pub objective: Option<Objective>,           // tagged by "kind"; only "bomb" today
    pub round_result: Option<RoundResult>,
    pub explosion: Option<BombExplosion>,
}
```

Unknown fields must be ignored (serde's default) so a newer server does not
break an older native client. The full field list for `Objective`
(`BombObjectiveSnapshot`), `RoundResult` and `BombExplosionEvent` is in
`src/shared/server/gametype/GametypeTypes.ts` lines 68, 317 and 361.

### 18.3 Server messages and refusals

`JoinReject` (`0x03`) is defined but the server never sends it. Refusals come
as a `ServerMessage` (type `"info"`, `"error"` or `"queue"`) or as a closed
socket. From `GameServer.ts`:

| What happens | How the client hears it |
| --- | --- |
| Join with an invalid or used ticket | socket closed with reason `invalid match ticket` |
| Server full | close code 1013, `Server full` (`WebSocketTransport.ts:195`) |
| Resume on a seat that is gone | error: "Your session has expired — rejoin from the lobby." |
| Resumed from another connection | old socket closed: `resumed on another connection` |
| JoinGame after the match ended | error: "This match has ended." |
| JoinGame on a seat taken | error: "That seat is no longer available." |
| JoinGame without a ticket on a lobby server | error: "A match ticket is required to play." |
| JoinGame too soon after the last | error: "Wait a moment before joining again." |
| S&D before choosing a kit | info: "Choose a loadout before joining." |
| Round in progress | info: "Round in progress — you are in from the next one." |
| Server full of players | queue messages, then info "You have been promoted from the challengers queue!" |
| Team chat without a team | info: "Team chat is only available to players on a team." |
| Match over | socket closed: `match over` or `match over — returning to lobby` |
| Nobody playing | info: "Nobody is playing, so this match is closing." |

The browser shows these texts as they are, so the Rust client can too. Mapping
the `"error"` ones to a typed reason would be nicer, but needs a code on the
wire; it belongs with the protocol version in 16.1.

### 18.4 The lobby, completed

`src/lobby/types.ts` is not the whole protocol. The server also sends
`lobby:chat_message` (room chat, `{ lobbyId, from, fromId, text, ts }`),
`lobby:shuffle_progress` (`{ votes, needed }`), `queue:left` and `stats:error`
(`{ requestId, reason }`), none of which have an interface there. And
`lobby:created` is reused as the full room state whenever you create, join or
drop into a room, not only on creation.

So the Rust enum gets a catch-all, and these four are added by hand:

```rust
#[derive(Deserialize)] #[serde(tag = "type")]
pub enum LobbyIn {
    #[serde(rename = "welcome")] Welcome(Welcome),
    #[serde(rename = "auth:error")] AuthError { code: String, online: Option<u32>,
        #[serde(rename = "maxPlayers")] max_players: Option<u32>,
        #[serde(rename = "retryAfterSec")] retry_after_sec: Option<u32> },
    #[serde(rename = "lobby:created")] RoomState { lobby: MatchLobbyInfo },
    #[serde(rename = "lobby:connect")] Connect(LobbyConnect),
    #[serde(rename = "lobby:chat_message")] RoomChat(RoomChat),
    #[serde(rename = "lobby:shuffle_progress")] ShuffleProgress { votes: u32, needed: u32 },
    #[serde(rename = "queue:left")] QueueLeft,
    #[serde(rename = "stats:error")] StatsError { #[serde(rename = "requestId")] request_id: String, reason: String },
    // ... one variant per remaining server message in types.ts (about 40) ...
    #[serde(other)] Unknown,
}
```

`add_lobby_bot` goes to the bot server only, and the `dev:*` messages are
development tools; the native client needs neither. The auth error codes are
`unknown_key`, `rate_limited`, `already_authenticated` and `lobby_full`.

### 18.5 The demo file, for the golden corpus

`DemoReader.ts`, all little-endian, strings as in 4.2, the whole file
optionally gzip (`1f 8b`):

```text
"SHAG" version:u8 (1..=6)
map:str gametype:str tickRate:u8 snapshotRate:u8 startTimeHigh:u32 startTimeLow:u32
v2+: matchId:str
v4+: visibility:u8 (0 public, 1 private)
playerCount:u8, then per player: playerId:str name:str team:str [v2+: accountId:str]
frames: tick:u32 frameLen:u32 raw-snapshot[frameLen] [v3+: inputLen:u32 inputs[inputLen]]
index: frameCount:u32, then frameCount × (tick:u32 offset:u32)
trailer: durationMs:u32 indexOffset:u32          (the last 8 bytes)
```

Each frame is a full raw snapshot with no envelope, decoded without a baseline.
Version 6 changes only the `match_state` JSON (bomb brush state), so the
decoder is the same from version 5 on.

### 18.6 Golden vectors: the plan

Three sets of fixtures, each produced by the TypeScript and checked by Rust:

1. **Demo frames.** A Node script reads every `.shagdemo` with `DemoReader`
   and writes one JSON line per frame. A `cargo test` decodes the same file in Rust
   and compares it with that JSON. The diff must be empty. This covers
   full snapshots and every event the recordings contain.
2. **Deltas.** A Node script takes consecutive demo frames, encodes frame *n*
   against frame *n − k* with `MessageCodec.encodeSnapshot(state, baseline)`
   for several *k*, and writes `{ baselineTick, bytes, expected }` per case.
   Rust decodes against the same baseline. This exercises every carry-forward
   rule in 4.5, including the movement view bytes and the powerup mask.
3. **Messages.** Every encoder in `MessageCodec.ts` called with edge values
   (empty strings, multi-byte UTF-8, the optional trailing fields present and
   absent), written as hex. Rust both decodes them and re-encodes the
   client-to-server ones byte for byte.

Canonical JSON for the diff: object keys sorted, `undefined` fields dropped,
and numbers compared after parsing rather than as text, because JavaScript and
Rust print some floats differently (for example `1e21`) while meaning the same
`f64`. The TypeScript decoder adds both
`position` and `transform` for every entity; the script flattens components to
the Rust shape before writing, so the diff compares meaning, not layout.

Where the scripts live: wash-fps `scripts/rust-golden/` once the Rust work
starts, run in CI after SHAG changes, so a protocol change fails the Rust
fixtures the same day (risk 16.4).

### 18.7 Changes to earlier sections

- 4.4: `JoinReject` is never sent; refusals are listed in 18.3.
- 4.5: absent entities may be out of view, not removed (18.1, point 2).
- 16: the protocol version proposal should also carry a refusal code (18.3).
- 16: `types.ts` is missing four server messages (18.4); worth adding there so
  both clients read the same list.

---

## 19. Round 3: the shared simulation

Round 3 follows one predicted step from `Game.sendInput` down to the brush
trace, on both sides, and records everything a Rust copy must reproduce.
Sources: `simulation/*.ts`, `client/V2NetworkClient.ts`,
`client/InputHistory.ts`, `server/systems/FPSMovementSubsystem.ts`,
`server/ReSim.ts`, SHAG's `src/shared/server/simulation/InputSanitizer.ts`,
`src/shared/collision/*`, `src/engine/physics/{SweepCollisionBackend,SweepBackendAdapter}.ts`.

### 19.1 One predicted step

Per command, in this order (`V2NetworkClient.processInput`):

1. `sanitizeMovementOrZero`: clamp |direction| to 1 (with `Math.hypot`),
   clamp pitch to ±π/2, wrap yaw to (−π, π], mask buttons to 16 bits. The
   server runs the same function on ingest (`sanitizeCommand`).
2. Store the input in `InputHistory` under the next sequence number.
3. Step the lifts (`collision.lifts.step(dt, [localState])`).
4. Step knockback drag (`applyKnockbackDrag`), **before** movement, reading the
   grounded state the step starts from.
5. `MovementProfile.simulate(position, velocity, input, dt = 1/125, collision)`.
6. Save the prediction for that sequence: position, velocity, grounded,
   knockback age, the full `MovementProfile.saveState()`, and the lift states.
   Reconciliation compares against this and replays from it (5.1).

Inside `simulate` (`MovementProfile.ts` lines 449 to 1030):

```text
sample water level and ladder volume at the start position
simulateCore:
  sim clock  ← input.simStep × 8 ms        (falls back to sequence, then to dt)
  wallJump.beginStep
  deep water or ladder → reset wall jump and ledge grab, exit slide
  wall jump charging   → WallJump.handleCharging, return early
  ledge grab active    → LedgeGrab.update, return early when it owns the step
  climb decision (ladderIntent)
  gravity (×1.25 when falling; off while swimming or climbing)
  input: walk/run speed (+ weapon speedBonus), crouch, jump (slope check by
         trace, normal.y > 0.85), jump off ladder
  movement branch: water jump | ladder | swim | ground (friction, wade, accel)
                   | air (forward accel, strafe-bunny accel, or normal air accel)
  wall jump detection in air
  crouchslide, crouch state
  ground-plane clip (OVERCLIP 1.001) then collision.stepSlideMove
  map triggers: teleporters first, then jumppads (re-aimed from the current
                position on every tick inside the pad)
  speed tracking, smoothed Y
sample water level and ladder again at the end position
```

Every timer (slide, wall jump, ledge grab, water jump, ladder detach) is an
absolute time on the **sim clock**, which is `simStep × 8 ms`. The client uses
the command sequence as `simStep`; the server uses its own step count. Only
differences between times matter, so the bases never have to agree. A Rust
port must keep that rule: no `Instant::now()` anywhere in the simulation.

### 19.2 File by file

| File | Lines | What it holds | Rust |
| --- | ---: | --- | --- |
| `MovementProfile.ts` | 1,073 | the step above; hulls; trigger volumes; `saveState`/`loadState`; `getDeterminismFingerprint` | `washoi::simulation::movement_profile`, one `struct` with the same fields |
| `Acceleration.ts` | 198 | `applyFriction`, `groundAccelerate`, `calcAccelFalloff`, `airAccelerateForward`, `strafeBunnyAccelerate` (`STRAFE_BUNNY_ACCEL = 50`), `normalAirAccelerate` | free functions over `&mut DVec3` |
| `Slide.ts` | 242 | crouchslide state machine and crouch view timing (fade 400/250 ms, crouch down 140, up 260, offset −24) | `struct Slide` with `save_state`/`load_state` |
| `WallJump.ts` | 385 | detection with 100 ms grace, charge, boost and instant variants, cooldown | `struct WallJump` |
| `LedgeGrab.ts` | 318 | detection by traces, grab and pull, 300 ms retry delay | `struct LedgeGrab` |
| `Swim.ts` | 217 | water friction and acceleration, wade scale, swim intent, idle sink, water jump check | free functions + `LiquidMovementProfile` |
| `Ladder.ts` | 187 | ladder friction, intent, acceleration, climb delta | free functions + `LadderMovementProfile` |
| `Knockback.ts` | 48 | drag after a hit: 50-tick delay, 75-tick ramp, horizontal 0.96, extra gravity 600 | free functions |
| `MovementConfig.ts` | 149 | `MovementConfig` and the result types | `struct MovementConfig` (see 19.3) |
| `FPSSimulation.ts` | 207 | local practice simulation, random spawn pick | only for offline play; not needed to connect |
| `Vec3Helpers.ts` | 60 | small vector helpers | `glam::DVec3` methods |
| `liquid/LiquidVolumeSampler.ts` and `LiquidTypes.ts` | part of 1,637 | water level 0 to 3 from point contents | free functions |
| `src/shared/server/simulation/InputSanitizer.ts` | 130 | 19.1 step 1 | `shagger_shared::server::input_sanitizer` |
| `src/shared/bsp/LiftSystem.ts`, `TriggerTable.ts`, `BrushModelState.ts` | about 900 | movers, trigger volumes, inline brush models | `shagger_shared::bsp` |

The three state machines (`Slide`, `WallJump`, `LedgeGrab`) each already have
`saveState`, `loadState` and `reset`. In Rust they become plain `Clone`
structs and the save and load pair becomes a copy, which removes a class of
"forgot a field in saveState" bugs.

### 19.3 Configuration: nothing to sync

The movement tuning is one constant, `DEFAULT_FPS_MOVEMENT_CONFIG`
(`MovementProfile.ts:96`), and both the client (`new FPSMovementProfile()` in
`V2NetworkClient.ts:268`) and the server (`FPSMovementSubsystem.getProfile`)
build it with no overrides. The `sv_*` and `cl_*` movement cvars
(`MovementVars`) do not reach the shared simulation; on the server they only
drive fall damage, and on the client only the camera (stair smoothing, view
height). The one runtime input is `speedBonus` (for example +25 with the
blade), set from the weapon the player holds.

So the Rust client copies the constant, and a test compares it with the
TypeScript: `getDeterminismFingerprint()` returns every config field as
sorted `key=value` pairs, and Rust can build the same string. One string
comparison in CI proves the tuning matches.

Other constants that must match: hulls `(-16,-24,-16)..(16,32,16)` standing,
`..(16,8,16)` crouched, `(-16,-14,-16)..(16,14,16)` swimming
(`MovementProfile.ts:142`); `TELEPORT_EXIT_LIFT = 25`; the player constants in
`src/engine/physics/PlayerPhysicsConstants.ts`; and the liquid and ladder
profiles (`DEFAULT_LIQUID_MOVEMENT`, `DEFAULT_LADDER_MOVEMENT`).

### 19.4 Collision as the simulation sees it

The simulation only knows `CollisionWorld` (`src/shared/collision/CollisionWorld.ts`):
`trace`, `stepSlideMove`, `pointContents`, `isInLadder`, plus `lifts` and
`brushModels`. `SharedCollisionWorld` implements it on top of a `SweepBackend`
(the Q3 brush trace), and adds ground detection after each slide move.

Three things to copy exactly, or the Rust client will disagree with the server:

1. **Two content-flag vocabularies.** The BSP backend uses raw Q3 bits
   (`WATER = 0x20`, `LAVA = 0x8`, ...); the shared code uses its own compact set
   (`WATER = 1 << 1`, `LAVA = 1 << 2`, ...). `SweepBackendAdapter.toSharedContents`
   translates, and the comment there notes that raw `LAVA` equals shared
   `SLIME`. Keep the two as separate Rust types (`RawContents`, `Contents`) so
   the compiler refuses to mix them.
2. **The slide-move hull ignores crouch and swim.** `SharedCollisionWorld.stepSlideMove`
   uses the hardcoded `PLAYER_RADIUS` and `PLAYER_HEIGHT` for anything whose
   radius is not well under the player's, so the crouched and swimming bounds
   only affect the other traces. This looks like a bug, but the server does the
   same, so the port must do the same. Fixing it is a gameplay change for both
   sides.
3. **Coordinates.** The backend works in Q3 space internally and converts at
   its edges (`SweepBackendAdapter`); the simulation sees Babylon space. Port
   the adapter as it is.

The backend itself (`SweepCollisionBackend.ts`, 1,783 lines: BSP node and leaf
traversal, brush and patch facet traces, clip brush index, ladder index,
`clipVelocity`, `stepSlideMove`, `pointContents`) imports Babylon only for
`Vector3`. It reads planes as `f32` from the BSP and computes in `f64`
(`Float64Array` for facet planes), so Rust stores `f32` from the file and
widens on use.

Player-versus-player separation runs only on the server
(`FPSMovementSubsystem`, "post-movement separation"). The client does not
predict it, so bumping into someone causes a small correction in the browser
too. The Rust client does the same, deliberately.

### 19.5 How the server consumes commands

The server steps a player **once per command, not once per tick**
(`FPSMovementSubsystem.ts` around line 810):

- A tick runs the next queued command. With none queued, the player pauses and
  the tick becomes debt; later ticks with a backlog run two commands to repay
  it.
- Past a debt limit the server guesses by repeating the last command (action
  buttons stripped after a grace window), and the late real command is then
  dropped.
- A queue that stays deep for a second gets drained by one command.
- A sequence at or below the newest one seen starts a new stream: that is how a
  resumed connection's numbering is accepted.
- `ackedInputSeq` is the last command physics accounted for, real or guessed.

What this means for the client: send exactly one command per 8 ms of real time,
smoothly. Bursts are tolerated but delay you; long gaps make the server guess.
This is why `FPSController` runs a fixed 125 Hz accumulator capped at 0.25 s.
Correction to 18.1 point 8: the browser client keeps counting after a resume,
and the server would also accept a restart from 1; either works.

### 19.6 Determinism audit

| Operation | Where | Risk | Rust |
| --- | --- | --- | --- |
| `+ − × ÷`, `Math.sqrt`, `abs`, `min`, `max`, `floor` | everywhere | none: IEEE-exact on both sides | `f64` operators |
| `Math.sin`, `Math.cos` | 18 calls: jump direction, water jump, ladder and swim intent, ledge grab, wall jump (`COS_22_5`, `SIN_22_5`) | low to medium: V8 uses an fdlibm port; platform libm differs by an ulp | the `libm` crate, checked against V8 on a table of angles |
| `Math.hypot` | `InputSanitizer` (every command), `LedgeGrab.ts:146` and `:271` | medium: V8's `Math.hypot` is not C `hypot`; it scales by the largest value and sums with compensation | port V8's algorithm into a `js_hypot` helper; never call `f64::hypot` |
| `Math.round` | `stateChecksum`, ledge and slide code | rounds ties up, unlike `f64::round` | `js_round` helper (6.2) |
| `angle % twoPi` | `wrapAngle` | none: JS `%` and Rust `%` on `f64` are both the IEEE remainder of truncated division | `%` |
| `Math.random` | `FPSSimulation.ts:139` only | none for the client | not ported into prediction |
| Map iteration order | trigger volumes, lifts | low: insertion order in JS | `Vec` in file order |

**A finding worth fixing upstream.** The browser predicts with its own `f64`
yaw, pitch and direction, but the server simulates the `f32` values it decoded
from the command (`BinaryProtocol.encodeCommand` writes `f32`; nothing in
`client/` or `Game.ts` calls `Math.fround`). So every predicted step starts
from slightly different inputs than the server's. The 0.01-unit confirm
tolerance mostly hides it, but long air-strafing chains or a wall jump at a
corner can tip over. The Rust client should round the sanitized direction,
pitch and yaw to `f32` before predicting, which makes its inputs identical to
the server's. The same one-line change in `V2NetworkClient.processInput`
(`Math.fround` on the three fields) would help the browser too; it is worth a
measurement with the net graph's confirmed count before and after.

### 19.7 A parity harness that actually exercises the map

Section 6.3 suggested `sim replay`. Reading it in round 3 rules it out:
`headless-game.ts` runs `sim replay` through `CharacterPhysicsSystem` with no
collision world and no map, starting in mid-air, so it tests neither the
movement profile the client predicts with nor any collision.

Two existing pieces come closer:

- `server/ReSim.ts` and `ReSimBundle.ts` replay a recorded usercmd log through
  `FPSMovementSubsystem` and checksum every tick (`stateChecksum`: position and
  velocity ×100 rounded, grounded, health). `cli/demo-verify.ts` checks a
  `.shagverify` bundle against its golden checksums. It covers the server's
  command scheduling, but runs without a collision world today.
- Demo version 3 records inputs, but aggregated per frame (buttons OR-ed, the
  last aim kept; `DemoInputs.ts`), so it cannot drive an exact re-simulation.

So round 3 recommends one new Node tool, `sim:dump`. A Rust `cargo test` runs
the same inputs and compares each step:

```text
input:  map name, spawn position and yaw, a command list (one line per 8 ms step:
        dirX dirZ pitch yaw buttons), optional speedBonus
run:    SharedCollisionWorld over the map's pk3 (NodePk3 resolveMapBspViaIndex),
        lifts and trigger volumes from the BSP, new FPSMovementProfile(),
        the exact V2NetworkClient step order of 19.1
output: one JSON line per step: position, velocity (full f64), grounded,
        knockback age, saveState(), lift states, water level, onLadder
```

Command lists come from three places: hand-written scripts for each technique
in SHAG's `MOVEMENT_TECHNIQUES.md` (strafe jump, bunny hop, crouchslide, wall
jump, ledge grab, ladder, swim, teleporter, jumppad, lift); a fuzzer that
produces random but plausible inputs; and real play, captured by logging
`InputHistory` from the browser client in a local match. Rust and TypeScript
must agree to the bit on every step; any difference is printed with the step
number and the first field that differs.

This harness needs no server and no network, so it can run in CI on every
SHAG change, next to the golden protocol vectors of 18.6.

### 19.8 What the Rust simulation crate looks like

```rust
// washoi::simulation
pub struct MovementProfile {
    config: MovementConfig,            // copied from DEFAULT_FPS_MOVEMENT_CONFIG
    state: MovementProfileState,       // everything saveState() returns, Clone
    jumppads: Vec<JumppadVolume>,
    teleporters: Vec<TriggerVolume>,
    pub speed_bonus: f64,
}

impl MovementProfile {
    pub fn simulate(&mut self, pos: DVec3, vel: DVec3, input: &MovementInput,
                    dt: f64, world: &dyn CollisionWorld) -> MovementResult { /* 19.1 */ }
    pub fn save_state(&self) -> MovementProfileState { self.state.clone() }
    pub fn load_state(&mut self, s: &MovementProfileState) { self.state = s.clone(); }
    pub fn determinism_fingerprint(&self) -> String { /* 19.3 */ }
}

pub struct MovementInput {
    pub direction: DVec3,   // already sanitized and rounded to f32 (19.6)
    pub pitch: f64,
    pub yaw: f64,
    pub buttons: u16,
    pub sequence: u32,
    pub sim_step: u32,      // = sequence on the client
}
```

`CollisionWorld` is a trait in `shagger_shared::collision`, implemented by
`SharedCollisionWorld<B: SweepBackend>` with the Q3 backend in
`shagger::physics`. The simulation never sees the renderer, so `washoi`'s
simulation module and its harness build without `wgpu`.

### 19.9 Estimate update for milestone 3

Round 1 estimated 4 to 6 weeks. With the harness design above it splits into:
BSP parse and collision backend, about 2 weeks; movement profile and the three
state machines, about 1.5 weeks; lifts, triggers, liquid and ladder, about
1 week; `sim:dump`, the scripted cases and the fuzzer, about 1 week; chasing
the last differences (hypot, sin, ordering), up to 1 week. That is still 4.5 to
6.5 weeks, now with each part's acceptance test named.

### 19.10 Changes to earlier sections

- 6.2: `Math.hypot` is used by the simulation after all (the sanitizer on every
  command, and the ledge grab), so `js_hypot` is required, not optional.
- 6.3: replaced by 19.7.
- 18.1 point 8: corrected in 19.5.

---

## 20. Round 4: assets

Round 4 follows a map from the published manifest down to a texture upload,
and fills in the asset facts round 1 left open. Sources: SHAG's
`src/engine/loader/{pk3,map,bsp,texture}`, `src/cli/pk3-index-build.ts`,
`src/cli/map-texture-encoding.ts`, `scripts/convert-ktx2.js`, `config/*.json`,
wash-fps `scripts/deployment/*`, and SHAG's research notes
`DOWNLOAD_IMPROVEMENTS_ALPHA4.md` and `per_map_texture_packages.md`.

Nothing below was fetched live, and it does not need to be: the repos hold the
deploy scripts, the planned server config, the index builder and the package
configs. 20.9 says what that settles.

### 20.1 Where the data lives

- **Origin.** The game is served at `https://eu.washoi.net/` and
  `https://us.washoi.net/`, and the data at `https://<region>.washoi.net/data/`.
  On the hosts that is `/srv/washoi/releases/game/data/`, which the deploy
  scripts sync from the persistent `/srv/washoi/assets/data/`
  (`deploy-eu.sh:183`, `activate-us.sh:20`). The deploy leaves out `demos/`
  and the retired `washoi_textures.pk3`.
- **Mirror.** `VITE_PK3_MIRROR_URL` is unset in production, and the 2026-10-22
  upload note says no mirror is configured. A native client still keeps the
  same rule (`Pk3Mirror.ts`): the origin decides, and a mirror copy is used only
  when a HEAD to the origin agrees on the size.
- **Caching headers** (`DOWNLOAD_IMPROVEMENTS_ALPHA4.md`, US, 2026-10-24): the
  pk3s and `pk3_index.json` are sent with
  `Cache-Control: public, max-age=3600, must-revalidate`, `ETag` and
  `Last-Modified`. The browser asks for the manifest and the index with
  `cache: "no-cache"`, so they are revalidated on every start.
- **Which region.** Data is per region. A Rust client takes its data from the
  region whose lobby it joined, because each region's index only lists that
  region's packages.

### 20.2 Startup: manifest, index, then lazy packages

```text
GET data/pk3_manifest.json                (revalidate)
    { "files": [ "map_wdm2.pk3", ... ], "indexHash": "1a2b3c4d",
      "indexFile": "pk3_index.<sha256 of the index text>.json" }
GET data/<indexFile>                      (revalidate; immutable by name)
    { "version": 2, "listChecksum", "lastUpdated",
      "packages": [ { "pk3", "size", "fileCount", "sha256" } ],
      "files":   { "<lowercase path>": "<owner pk3>" },
      "shaders": { "<shader name>":    "<owner pk3>" },
      "maps":    { "<map name>":       "<owner pk3>" } }
GET data/<pk3>?sha256=<hex>               (on demand, per map)
```

`parsePublishedIndex` (`PK3PublishedIndex.ts`) refuses the whole snapshot on
any problem, and the Rust parser must refuse the same cases:

- `indexHash` does not match the text;
- the version is not 2;
- a package name is not `^[a-z0-9][a-z0-9_.-]*\.pk3$`;
- a package is listed twice, or the index and manifest lists differ;
- a `sha256` is not 64 lowercase hex characters;
- a dictionary key is not lowercase with forward slashes;
- an owner is not in the manifest;
- `maps[m]` is not the owner of `files["maps/<m>.bsp"]`;
- `listChecksum` is not the hash of the sorted `pk3:size` pairs joined by `|`.

When validation fails, the browser falls back to downloading and scanning every
archive. A native client should not copy that fallback. It should report the
error and retry later instead.

**Two different hashes.** The index file *name* uses SHA-256, but `indexHash`
and `listChecksum` use `pk3IndexHash`, which is djb2 with XOR. It runs over
UTF-16 code units with JavaScript's 32-bit wrapping, and is printed as unpadded
lowercase hex. In Rust:

```rust
pub fn pk3_index_hash(text: &str) -> String {
    let mut h: i32 = 5381;
    for unit in text.encode_utf16() {
        h = (h << 5).wrapping_add(h) ^ unit as i32;
    }
    format!("{:x}", h as u32)
}
```

Iterating `chars()` or bytes instead of `encode_utf16()` gives the same result
only for ASCII. A golden test against the real manifest belongs with 18.6.

**Package integrity.** Each package carries its SHA-256 in the index.
`PackageStore` adds it as `?sha256=` (a cache key, not authentication), checks
the digest after download, and keeps packages in CacheStorage
(`shag-packages-v1`, 512 MB on disk, 128 MB in memory). The download has three
timeouts: 15 s for headers, 60 s when idle, and 15 min in total. It is retried
twice for 5xx, 408 and 429 responses, waiting 500 ms between tries.

Natively the cache is simpler and stronger:

- store each package as `<cache dir>/packages/<sha256>.pk3`;
- check the digest while the download streams (`sha2`);
- write to a temporary file and rename it into place;
- remove the least recently used files beyond the 512 MB budget.

A package that is already on disk with the right name needs no HTTP request at
all, so the hourly revalidation goes away. Then memory-map the file (`memmap2`)
and open it with `zip::ZipArchive`, so nothing is read until it is used.

### 20.3 Which packages a map needs

`MapLoader.buildBspMap` and `prepareMapTextureBank` give the order, and the
Rust map loader keeps it:

1. **Map package:** `index.maps[name]`, for example `map_wdm2.pk3`. Read
   `maps/<name>.bsp`.
2. **Texture bank:** `tex_<name>.pk3`. It must hold `media-manifests/<name>.json`,
   with `map` equal to the name and an optional `sharedDependencies` list of
   `{ package, dynamic, texturePrefixes }`. Mounting it also sets the
   **package scope**: the other maps' `tex_*.pk3` banks become invisible to
   lookups, so a texture is never taken from a different map's bank.
3. **Shared banks:** for example `tex_billboard.pk3`. A dynamic bank gets
   `?pk3v=<index lastUpdated>`, so its artwork can change without a client
   build.
4. **Overrides:** a file or shader in the bank whose index owner is another
   package that is not loaded yet pulls that owner in.
5. **Textures named by the BSP:** for each BSP texture, the package that owns
   its shader, or the first of `<name>.webp`, `.tga`, `.jpg`, `.png`, `.ktx`,
   with and without a `textures/` prefix.
6. **Entity models**, from the BSP entities through the entity definitions,
   then `washoi_weapons.pk3`.
7. **Shaders:** every `scripts/*.shader` in the mounted packages.
8. **Sky:** the packages for sky textures, resolved after the shaders, because
   the sky names come from shader `skyParms`.

In Rust, `PK3PackageScope` becomes a field of a `Vfs` struct, and every lookup
goes through `Vfs::find(path)`. Later packages win, scoped banks are skipped,
and paths are lowercased with forward slashes. Make the scope a value passed
into the loader rather than global state, so a background map preload cannot
change the scope of the map being played.

### 20.4 What the shipped packages contain

These counts come from the configs that build the packages, not from the
archives.

| Content | Package(s) | Format on the wire | Count | Source |
| --- | --- | --- | ---: | --- |
| Map geometry | `map_<name>.pk3` | IBSP v46 or FBSP v1 | 27 maps listed | `config/maps.json` |
| Map textures | `tex_<name>.pk3` | **WebP** (from 690 TGA and 194 PNG sources) | 884 textures: 347 colour, 235 normal, 178 gloss, 100 alpha, 24 sky | `config/map-texture-encoding.json` |
| Billboards and branding | `tex_billboard.pk3` (dynamic) | PNG | 11 entries | `config/billboard-package.json` |
| Level previews | `data/levelshots/<map>.jpg` (outside pk3s) | JPEG made from `levelshots/<map>.png` | one per map | `pk3-index-build.ts` |
| Sounds | sound package | OGG Vorbis 272, WAV 18 | 290 | `config/sound-package.json` |
| Player model | `washoi_player_model.pk3` | glTF `.glb` plus one `.glb` per animation | 1 model | `config/player-model-vanguard.json` |
| Weapons | `washoi_weapons.pk3` (85.9 MB) | `.glb` | 21 `.glb` paths in wash-fps | `config/weapon-models.json` |
| Items and projectiles | Warsow data packages | MD3 | 9 `.md3` paths in wash-fps | `shared/entities/definitions` |
| KTX2 sidecars | `data/ktx2cache/` (optional) | KTX2, ETC1S or UASTC with zstd | not known (20.9) | `scripts/convert-ktx2.js` |
| ETC1 `.ktx` | legacy packages | KTX1 | fallback only | `PK3TextureLoader.ts:196` |

**WebP is the main texture format.** Round 1 listed TGA first. In fact, the map
banks re-encode every listed TGA and PNG as WebP under the same path
(`map-texture-encoding.ts`). The qualities are 85 for colour, 85 for alpha with
a lossless alpha channel, 92 for normals, 90 for gloss and 95 for sky. Shaders
still name `foo.tga`, and the loader tries `foo.webp` first
(`TextureExtensions.ts`, `PACKAGED_TEXTURE_EXTENSION`). The Rust lookup must
strip the extension and try the same order.

The `image` crate (0.25) decodes both lossy and lossless WebP in pure Rust
(`image-webp`), so the first texture path needs no C library. TGA is kept for
older data and user maps.

**Texture classes decide colour space.** In the encoding list, `colour`,
`alpha` and `sky` are sRGB. `normal` and `gloss` are linear data.
`convert-ktx2.js` makes the same split, using `_normal`, `_norm`, `_n.` and
similar name patterns to pick UASTC with a linear transfer function. The Rust
loader needs one function that maps a texture path to `Rgba8UnormSrgb` or
`Rgba8Unorm`. Getting it wrong shows up as washed-out or overly dark lighting,
not as an error.

### 20.5 BSP details that matter for the port

- **Two formats in one loader.** IBSP v46 has 44-byte vertices and 128×128
  lightmaps. FBSP v1 (Warsow) has 80-byte vertices: position, uv, four
  lightmap uvs, normal and four RGBA colours. It also has four lightmap slots
  per surface for light styles, and lightmaps whose size is set by the map,
  512×512 by default. Both have 17 lumps.
- **A wrong comment.** The comment on `BspVertexFBSP` in `BspLoader.ts:139`
  says 68 bytes, but the parser and the collision path both use 80, which
  matches the RBSP layout. Port the code, not the comment.
- **Coordinates.** Convert once at the loader edge (7.2). The collision lumps
  keep their Q3 values, and `SweepBackendAdapter` converts (19.4).
- **BSP texture names have no extension.** Every lookup goes through the
  extension order in 20.3, step 5.

`BspLoader.ts` (1,246 lines) imports nothing but the logger, so it translates
directly into a `shagger::loader::bsp` module over `&[u8]` using `bytemuck` or
`zerocopy` structs. It is the first thing worth porting: the collision crate of
19.4 and the `sim:dump` harness of 19.7 both need it.

### 20.6 Textures: what a native client decodes

| Path | Browser | Native plan |
| --- | --- | --- |
| WebP from `tex_*.pk3` | browser image decode | `image` (WebP), then mipmaps on the GPU with a compute or blit pass |
| TGA, PNG, JPEG | `TgaDecoder.ts`, browser decode | `image`; port `TgaDecoder.ts` only if `image` disagrees on Q3 quirks (origin bit, RLE) |
| KTX2 sidecar (`r_ktx2 1`) | Babylon KTX2 worker, transcodes Basis to a GPU format | phase 2: `ktx2` crate plus `basis-universal` bindings, to BC7 on desktop |
| KTX1 `.ktx` (ETC1, a few uncompressed) | `KTX1Loader.ts` | decode on the CPU to RGBA8, as SHAG's software path does: 115 published textures exist only as KTX (decided November 2026, section 23) |
| `r_picmip` | reduces map textures | `image::imageops::resize` before upload, same rules |

Desktop VRAM is plentiful, so plain RGBA8 uploads from WebP are fine for the
first native milestones. KTX2 adds a C++ dependency (the Basis transcoder) for
memory and load-time savings that only matter later. Mark the KTX2 path behind
the same `r_ktx2` cvar, defaulting to 0 natively until it is measured.

### 20.7 Models and sounds

- **glTF:** the player, weapons and effects. shagger's
  `loader::model::glb` reads `.glb` with serde, embedded textures included,
  which use the same decoders as 20.6. (The plan named the `gltf` crate; it
  rejects Washoi's `EXT_texture_webp` textures, which carry no core
  `source`, so the few parts of the format the models use are read
  directly.) Animations
  come as separate `.glb` files, one per clip
  (`player-model-vanguard.json`), and are retargeted by bone name. Port the
  retarget table from SHAG's player model code rather than re-deriving it.
  The player model's meshes are `KHR_draco_mesh_compression`ed; they are
  decoded with `draco-core` (pure Rust, Apache-2.0, decoder features only).
- **MD3:** items and projectiles (`MD3Loader.ts`, 743 lines, plus
  `TagsLoader.ts`). It is a small fixed-layout format, and is translated by
  hand like the BSP.
- **Sounds:** OGG Vorbis and WAV only. Milestone 8 decodes them with
  `lewton` (MIT or Apache-2.0) and a small WAV reader instead of
  `symphonia`, which is MPL-2.0 and outside the license allow list.

### 20.8 Crates and module layout for assets

```text
shagger::loader
  pk3::published_index   PK3PublishedIndex.ts   serde, pk3_index_hash
  pk3::package_store     PackageStore.ts        reqwest (stream), sha2, tempfile, memmap2, zip
  pk3::mirror            Pk3Mirror.ts           reqwest
  pk3::vfs               PK3Index + PK3Loader + PK3PackageScope
  bsp                    BspLoader.ts           bytemuck
  shader                 Q3ShaderParser.ts      (none)
  texture                TextureExtensions, TgaDecoder, encoding classes   image
  model::md3             MD3Loader.ts, TagsLoader.ts
  model::gltf            Babylon glTF loader    gltf
washoi::assets           entity model paths, media manifest rules, map list
```

The package store runs on the tokio runtime that already carries the
WebSocket (4.x), and hands finished archives to the main thread through a
channel. Archive reads and decodes run on a small thread pool (`rayon`), so a
map load never blocks a frame.

### 20.9 What the repos settle without the live servers

- **KTX2 sidecars.** The deploy runs `npm run build` (`deploy-eu.sh:179`), not
  `build:prod`, so `tools:textures` (`convert-ktx2.js`) never runs on deploy.
  `data/ktx2cache/` exists only if someone generated it by hand into the
  persistent `assets/data`. The plan treats KTX2 as not shipped.
- **Index and manifest.** Both are built from repo code on every deploy
  (`prebuild` runs `tools:pk3-index`). The Rust `pk3_index_hash` can therefore
  be tested against the TypeScript function on any text, including non-ASCII.
- **Cache headers.** The planned Caddy blocks for `eu.washoi.net` and
  `us.washoi.net` (`washoi.net/docs/research/deploy.md`) give `/data/*`
  `public, max-age=31536000, immutable` with `encode zstd gzip`. The US
  measurement of 2026-10-24 found `max-age=3600, must-revalidate` instead, so
  the live config differs from the plan. The Rust client works either way:
  - it revalidates the manifest;
  - it keys packages by SHA-256;
  - it treats `Content-Length` as unknown when `Content-Encoding` is set, as
    `PackageDownload.response` does.
- **Still open:** exact file counts inside the shipped pk3s, because the
  archives are not in git. The counts in 20.4 come from the configs that build
  them, which is enough for planning.

### 20.10 What stays in TypeScript

The whole media pipeline stays in SHAG and wash-fps, and is not ported:

- packaging (`package-*.mjs`, `map-media.ts`);
- WebP encoding and the encoding list;
- `convert-ktx2.js`;
- `pk3-index-build.ts`, the manifest and index generation;
- the editor's publish path;
- the deploy scripts.

The Rust client reads the same `data/` directory that the browser reads, from
the same servers, so one asset build serves both clients.

What Rust must have is the **read side**, because it runs on the player's
machine:

- the index parser and its hash;
- the download cache;
- zip reading;
- the BSP, MD3 and shader parsers;
- image decoding (through crates);
- the lookup rules of 20.3.

That is the `shagger::loader` list in 20.8: about 5,700 of the 26,300 lines in
`src/engine/loader`. The rest is Babylon-side material and mesh
building, which round 5 covers with the renderer.

### 20.11 Changes to earlier sections

- 7.1: the public data URL is `https://<region>.washoi.net/data/`, and package
  integrity uses SHA-256 per package. The index hash itself is djb2, not
  SHA-256 (20.2).
- 7.2: WebP is the main map texture format (20.4). MP3 is not shipped. ETC1
  KTX1 is the only copy of 115 published textures, so the native client
  decodes it (corrected in section 23).
- 15, milestone 5 (assets, 3 to 4 weeks): unchanged, but KTX2 moves to a later
  optional step, and the BSP loader moves earlier, into milestone 3.

---

## 21. Round 5: the renderer

Round 5 reads the renderer as it runs today and turns it into a `wgpu` design.
Rendering is the one layer that cannot be translated line by line, because
SHAG draws through Babylon's scene, materials and cameras. Sources: SHAG's
`src/engine/{loader/bsp,loader/shader,rendering,lighting,effects,weapon,animation}`,
`MapRenderer.ts`, and the renderer research notes of 2026-10-25 to 10-27
(`babylon-classic-*`, `babylon-lite-*`, `RENDERING_PIPELINE.md`).

### 21.1 License rule

The project owner set this rule on 2026-10-31: **code ported or copied from
Babylon.js is Apache License 2.0; all other code is MIT.** Babylon.js itself is
Apache-2.0, and SHAG and wash-fps are MIT.

Babylon-derived code lives in its own folder inside the `shagger` crate,
with the license next to it. It is not a separate crate or repository:

```text
crates/shagger/
  Cargo.toml            license = "MIT AND Apache-2.0"
  LICENSE               MIT
  src/babylon/
    LICENSE             Apache-2.0 text
    NOTICE              each ported file, its Babylon source path and version (9.31.2)
    mod.rs
    standard_light.wgsl Blinn-Phong terms of StandardMaterial (world look)
    fog.wgsl            Babylon EXP fog
    image_processing.wgsl exposure, contrast, tone map, vignette
```

- Every file under `src/babylon/` starts with a short header that says it is
  Apache-2.0 and names the Babylon file it came from.
- Babylon-derived code goes in this folder and nowhere else. The rest of
  `shagger` calls into it, but never copies its code.
- `shagger_shared` and `washoi` stay `MIT`.
- The shipped client binary includes both license texts. `cargo deny` checks
  the dependency licenses against an allow-list.

The split also decides how some things are built. Where we only need the same
*result*, writing our own code from the published spec keeps it MIT:

- glTF PBR from the Khronos sample BRDF;
- ACES or Reinhard tone mapping from their papers;
- a plain Gaussian bloom.

Where the *look* must match pixel for pixel, porting Babylon's shader is
faster, and the code goes into `src/babylon/`. The world's Blinn-Phong
lighting is the main case: Washoi maps were tuned against it. 21.9 records
the choice for each item.

SHAG's own material plugins are MIT, even though they inject into Babylon
shaders. Our snippets can be lifted as MIT, but the Babylon shader code around
them cannot (21.6).

### 21.2 One frame today

The frame order assembled from the code:

| # | Pass | Owner | Target | Notes |
| ---: | --- | --- | --- | --- |
| 1 | Sky portal | `rendering/SkyPortal.ts` | render target | only when a portal sky is visible; sky meshes on their own layer mask |
| 2 | Water reflection and refraction | `rendering/water/*` (`MirrorTexture`, refraction RTT, cached `ReflectionProbe`s) | render targets | only when water is visible; probes are cached, not per frame |
| 3 | Main camera, group 0 | sky box or dome first, then opaque world, models, players | post-process input target | opaque sorted front to back by Babylon |
| 3a | Depth copy | `rendering/SceneDepthCapture.ts` | float depth texture | copies the main pass's depth to camera-space Z between opaque and transparent, for soft particles and water |
| 3b | Main camera, transparent | world blended stages, effects | same | sorted back to front by Babylon |
| 4 | Main camera, group 1 | fog volumes (`BspFogVolume.ts:544`), hit effects, debug | same | drawn after group 0, depth kept |
| 5 | Group 2 | bomb site markers | same | |
| 6 | Glow | `effects/GlowEffect.ts` (`GlowLayer`) | blur chain | emissive surfaces |
| 7 | Weapon camera | `weapon/WeaponViewManager.ts:236` | same | a second camera after the world camera, with its own FOV (`cg_weaponfov`); weapon meshes in group 1 |
| 8 | Post | `effects/PostProcessingManager.ts` (`DefaultRenderingPipeline`: bloom, image processing with contrast 1.05 and exposure 1.0, vignette, chromatic aberration), `ColorCorrectionPostProcess`, `CelOutlinePostProcess` | swap chain | each one behind `r_*` cvars |
| 9 | HUD and menus | DOM (round 8) | browser compositor | not part of the GPU frame today |

Dynamic lights go through one `ClusteredLightContainer` (`lighting/DynamicLightCluster.ts`),
so materials see a single stable light no matter how many effects are lit.
That design exists to avoid recompiles, and it carries over directly as a
storage buffer of lights.

### 21.3 The Rust frame graph

The same order, written as fixed passes rather than Babylon groups and cameras:

```text
per frame:
  visibility: PVS cluster → candidate world batches, frustum cull, model cull
  [portal sky]          → sky_target          (only when visible)
  [water captures]      → reflection/refraction targets (only when due)
  main pass  (HDR colour Rgba16Float, Depth32Float, MSAA per cvar):
      sky
      opaque world      (lightmapped, vertex-lit, alpha-tested)
      opaque models     (players, items, projectiles)
  depth resolve         → camera-space Z texture   (when soft particles or water need it)
  main pass, continued (load colour and depth):
      translucent world and effects, sorted back to front by surface centre
      fog volumes
      markers
  view model pass  (load colour, clear depth, own projection with cg_weaponfov)
  post: glow, bloom, image processing, outline, colour correction  → swap chain (not sRGB:
        image processing already writes display-space bytes, as Babylon's canvas holds them)
  HUD (egui)                                                       → swap chain
```

Two places where Rust should not copy Babylon:

- **The view model clears depth** instead of relying on group ordering. That
  is the classic Q3 approach. It removes the depth fights the browser avoids
  with group 1, and keeps the weapon out of world geometry.
- **The depth copy is a resolve step**, not the main-pass split SHAG had to
  engineer around Babylon's depth textures (`SceneDepthCapture.ts` header).
  In wgpu the main depth texture can be created with `TEXTURE_BINDING`, and
  the second half of the pass reads it.

### 21.4 Pipelines: what goes into the key

The alpha.7 note found that the WebGPU stall after loading a map was
render-pipeline creation: about 105 pipelines, against 3 to 5 shader compiles
(`babylon-classic-frame-floor-2026-10-26.md`, `babylon-classic-alpha7-results-2026-10-27.md`).
Light-layout changes after warmup then built new pipelines mid-match. A native
renderer can design that problem away.

Put only what wgpu requires in the pipeline key, and everything else in
uniforms:

```rust
#[derive(Hash, Eq, PartialEq, Clone, Copy)]
pub struct PipelineKey {
    pub program: Program,        // WorldLightmap, WorldVertexLit, Sky, ModelSkinned,
                                 // ModelRigid, Md3, Sprite, FogVolume, Beam, Particle, ViewModel
    pub blend: BlendKey,         // Opaque, Add, Blend, Filter, or the (src, dst) pair
    pub depth_write: bool,
    pub depth_func: DepthFunc,   // LessEqual or Equal
    pub cull: Cull,              // Front, Back, None (Q3 "disable" = None)
    pub alpha_test: AlphaTest,   // None, Gt0, Lt128, Ge128 (a shader override constant)
    pub samples: u8,
}
```

- **In uniforms, not in the key:**
  - every `tcMod` (folded into one 2×3 UV matrix per stage per frame, on the
    CPU as `ShaderAnimationSystem` does);
  - `rgbGen` and `alphaGen` (a colour and an alpha per stage per frame);
  - `tcGen` mode;
  - fog;
  - the dynamic light count, from the light buffer's length.
- **Variants:** `alpha_test` and the program's optional features (deluxe,
  vertex colour) use WGSL `override` constants. wgpu folds those at pipeline
  creation, so they cost pipelines but not shader modules.
- **Precompile at map load:** collect the key of every surface the map can
  draw, plus the fixed set for models, effects and the view model. Build them
  all behind the loading screen with `wgpu::PipelineCache`, which is
  persisted to disk on Vulkan. The pipeline count stops depending on what
  happens in the match.

### 21.5 Map surfaces

| Piece | TypeScript | Port |
| --- | --- | --- |
| Face building: polygons, meshes, patches, billboards | `bsp/mesh/faceBuilders.ts`, `vertices.ts`, `tangents.ts`, `coordinates.ts` (1,407 lines in `mesh/`) | direct; the output is `Vec<Vertex>` and `Vec<u32>` instead of Babylon `VertexData`. Patch level is fixed at 8 (`faceBuilders.ts:89`), with one shared grid per patch so there are no cracks |
| Batching by material and space | `BspMeshBuilder.ts` (2,222 lines), `mesh/spatialBatching.ts` | logic direct; one vertex and index buffer for the whole map, with draw ranges per batch |
| PVS | `PVSCuller.ts`, `mesh/pvs.ts` | direct. Add the face-level PVS the 2026-10-26 note measured (20 to 46% fewer triangles): rewriting an index buffer on cluster change is cheap in wgpu |
| Lightmaps | `mesh/lightmaps.ts`, `BspBakedLightingPlugin.ts` | lightmap pages become one `texture_2d_array`, with the layer in lightmap U as `u + 2 * layer`. Only the **primary light style** is used (`BspBakedLightingPlugin.ts:70`), so FBSP slots 1 to 3 can be parsed and ignored |
| Deluxe maps and gloss | `r_deluxe`, `DELUXE_MAP_SUPPORT.md` | in the WorldLightmap program as an `override` |
| Vertex lighting | `BspVertexLightingPlugin.ts` | WorldVertexLit program |
| Light grid for models | `LightGridSampling.ts` (79 lines) | direct: trilinear, invalid sites skipped, weights renormalized, in Q3 space |
| Texture arrays | `rendering/TextureArrays.ts` | not needed: wgpu builds arrays at upload |
| Sky | `texture/SkyboxLoader.ts` (1,288 lines), `mesh/skyClouds.ts`, `SkyPortal.ts` | farbox cube first; clouds and portals later |
| Fog volumes | `BspFogVolume.ts` (913 lines), `AtmosphericFogPlugin.ts`, `WorldFogController.ts` | after the first milestone |
| Autosprite | `Q3AutospritePlugin.ts`, `mesh/autosprite*.ts` | the vertex attributes and the vertex-shader math port directly |
| Brush models (movers) | `mesh/brushModelBuilder.ts` | direct, drawn with a model matrix from the lift state (19.4) |

### 21.6 Shaders: what can be lifted

SHAG's WGSL lives in Babylon `MaterialPluginBase` plugins: ten of them across
`loader/shader`, `loader/bsp` and `rendering`. Each plugin supplies GLSL and
WGSL snippets that Babylon splices into its StandardMaterial or PBR shader at
named points such as `CUSTOM_FRAGMENT_BEFORE_FOG`. They are fragments, not
shaders.

- **Liftable (MIT):** the snippet bodies. These include:
  - the lightmap layer addressing and baked lighting terms;
  - stage colour;
  - autosprite turning;
  - distance ramps;
  - the environment and cube map lookups;
  - soft-particle fade;
  - Warfork's atmospheric fog blend rules (`AtmosphericFogPlugin.ts`, which
    follows `r_backend_program.c`).

  They hold the math that matches qfusion, and they move into our own WGSL
  programs nearly unchanged.
- **Not liftable as MIT:** the Babylon shader around them. That covers
  StandardMaterial's light loop, the specular terms, and Babylon's fog,
  image processing and PBR. Either port it into `shagger/src/babylon/`
  (Apache) or write it from scratch.
- **Effect materials:** twelve `ShaderMaterial`s with our own GLSL (rail,
  voltaic, muzzle flash, impact sparks, explosions, tracers, unified water,
  water actor reflection, glow overlay, filter blend, fog volume, splash
  droplets) are MIT. Translate them to
  WGSL by hand, or with `naga`'s GLSL front end as a first draft.
- **NodeMaterial:** the one `NodeMaterial` (`Q3NodeMaterialBuilder.ts`,
  multi-stage shaders) is a graph built in code. Its *meaning* is the Q3 stage
  list, so the Rust side does not port the graph. It generates the same stages
  into the WorldLightmap program's stage loop (21.7).

### 21.7 Q3 shader stages

The parser (`Q3ShaderParser.ts`, 1,149 lines) and the types
(`Q3ShaderTypes.ts`) port directly. The feature set the renderer must cover is
the parser's:

- **Stage settings:** `blendFunc` (`add`, `blend`, `filter`, or a factor
  pair), `alphaFunc`, `rgbGen` and `alphaGen` (including waves and distance
  ramps), `depthWrite`, `depthFunc`, `detail`, and conditional blocks for
  `deluxe` and `textureCubeMap`.
- **Texture mapping:** `tcMod` (`scroll`, `scale`, `rotate`, `stretch`,
  `turb`, `transform`), `tcGen` (`base`, `lightmap`, `environment`,
  `surround`, `vector`), `animMap`, `clampMap`, `cubeMap`, and the Warsow
  `material` directive.
- **Shader-wide settings:** `deformVertexes` (`wave`, `normal`, `bulge`,
  `move`, `autosprite`, `autosprite2`), `skyParms`, `fogParms`, `cull`, `sort`,
  `portal`, `polygonOffset`, `nomipmaps`, `nopicmip`, and Warsow templates.

**How stages draw.** SHAG collapses the common patterns into one draw:

- base × lightmap;
- base × lightmap + glow;
- filter blends;
- additive only.

Other multi-stage shaders also become a single draw, a NodeMaterial graph
(`Q3ShaderMaterialFactory.ts`). Some materials use more than one pass
(`ResolveMaterialOptions.passIndex`). The Rust renderer does the same:

- a WorldLightmap program with up to four stage slots, each with its own
  texture, UV matrix, colour and blend-into-previous rule;
- extra passes only where SHAG uses them.

Keeping SHAG's choices gives the same look, even where SHAG differs from
classic Q3's one pass per stage.

**Which features the shipped maps use is not known yet.** The `.shader`
scripts live in the pk3s, and only two are in git. Before building the
WorldLightmap program, run a small Node audit tool, `shader:audit`. It runs
`Q3ShaderParser` over every package in `assets/data` and counts each keyword,
blend pair, tcMod and deform type, per map and in total. It prints JSON next to
`config/map-texture-encoding.json`. Features with zero uses are not built. This
needs a machine with the data directory (a dev box or the EU host), not the
live web servers.

### 21.8 Models and effects

- **glTF players** (`animation/`, 2,841 lines: `PlayerModelManager`,
  `AnimationBlender`, `HumanoidRig`, `ProceduralLayers`, `PlayerAnimationController`):
  - the logic ports directly over our own skeleton types;
  - skinning is the standard four-bone linear blend in the vertex shader,
    with bone matrices in a storage buffer;
  - animations are separate `.glb` clips (20.7), sampled with glTF's own
    interpolation rules, then blended and layered as `AnimationBlender` does;
  - procedural aim stays on the CPU.
- **Lighting models:** sample the light grid at the model origin (ambient plus
  one directional, like Q3), and add the clustered dynamic lights. That
  replaces Babylon's PBR plus scene lights for the first milestone.
- **MD3:** vertex frames lerped on the CPU or in the vertex shader, with tags
  for attachments.
- **View model:** `WeaponViewManager.ts` (2,275 lines) is mostly motion, bob,
  sockets and state, and ports directly. Only the camera setup changes (21.3).
  Its muzzle projection through the weapon camera (lines 255 to 280) must be
  kept exactly, or tracers start from the wrong pixel.
- **Effects:** `effects/` (4,247 lines) and `weapon/` (12,331 lines with
  subfolders) mix Babylon object management with timing and shape logic.
  The logic ports directly, and the Babylon parts are replaced. The renderers that matter for a
  match are beams (rail, voltaic), tracers, muzzle flashes, impact sparks,
  explosions and projectile glows. Each becomes one instanced draw with a
  small storage buffer, which is also what the Lite study proposes
  (`babylon-lite-native-performance-2026-10-26.md`).
- **Particles:** Babylon `ParticleSystem` is used for debris and rain only.
  These are later work, using our own CPU-simulated instanced quads.
- **Ragdolls and gibs:** Havok today; `rapier3d` later (8.2).

### 21.9 Babylon pieces: port or rewrite

| Piece | Choice | Where and license | Why |
| --- | --- | --- | --- |
| StandardMaterial lighting (diffuse, Blinn-Phong specular, lightmap combine) | port the WGSL terms | `src/babylon/`, Apache-2.0 | maps were tuned against it; it must match |
| Babylon EXP fog | port | Apache-2.0 | small, and world fog uses it outside Warfork fog |
| Image processing (exposure, contrast, vignette, tone mapping) | port | Apache-2.0 | the whole picture shifts if it differs |
| Bloom, chromatic aberration, GlowLayer | rewrite later, compare by screenshot | MIT | effects, not tuning |
| Clustered light container | rewrite: a light storage buffer with a forward loop, clustered later if needed | MIT | desktop GPUs handle a few dozen lights forward; the important part, no recompiles, is design, not code |
| glTF PBR | rewrite from the Khronos reference | MIT | players are tinted and lit by the grid anyway |
| Mirror texture, reflection probe | rewrite | MIT | they are just a camera and a target |
| Camera matrices (left-handed, Y up) | `glam` `*_lh` functions | MIT | 7.2 |
| glTF loading | own serde reader (`loader::model::glb`); the `gltf` crate refuses `EXT_texture_webp` textures | MIT | 20.7 |

### 21.10 First playable renderer

The cut for milestone 6 plus 7 (section 15). It is enough to play a match and
read it:

- **In:**
  - the world, lightmapped and vertex-lit, with deluxe;
  - Q3 stages for the features that `shader:audit` finds in use;
  - alpha test, autosprite, patches and the farbox sky;
  - PVS and frustum culling;
  - glTF players with skinning, lit by the scene lights (SHAG's players
    take no light grid and switch clips without blending), with their team
    tint and outline;
  - map items: weapons as glTF models; health, armour, ammo and the quad as
    SHAG's procedural sculptures, drawn with ported `StandardMaterial`s;
  - MD3 projectiles, with their shader stages and trails;
  - the view model with its own FOV;
  - beams, tracers, muzzle flashes, impacts and explosions;
  - dynamic lights as a forward storage buffer;
  - the HDR target with image processing as a post-process (exposure,
    contrast, the vignette and gamma);
  - pipeline precompile at load.
- **Out, until milestone 11:**
  - water reflection and refraction (water draws as a translucent surface);
  - fog volumes and atmospheric fog;
  - soft particles and the depth resolve;
  - sky clouds and portal skies;
  - environment cube maps;
  - bloom, glow, chromatic aberration, outlines and colour correction;
  - rain and debris particles;
  - ragdolls;
  - SSR.

### 21.11 Estimate update

Round 1 gave 6 to 8 weeks for milestone 6 and 6 to 8 weeks for milestone 7.
With the reading above:

| Work | Weeks |
| --- | ---: |
| wgpu device, surface, frame graph, pipeline cache and precompile | 1.5 |
| Map surfaces: faces, patches, batching, PVS, lightmaps, light grid, sky farbox | 2.5 |
| WorldLightmap and WorldVertexLit programs, Q3 stage evaluation, `shader:audit` | 2 |
| Ported StandardMaterial terms and image processing (`src/babylon/`), screenshot comparison harness | 1.5 |
| glTF skinning, animation blending, players, MD3 | 3 |
| View model, effect renderers, dynamic lights | 3 |
| **Total for the first playable renderer** | **13.5** |

That is inside the round 1 range of 12 to 16 weeks for milestones 6 and 7. The
remaining visuals of milestone 11 stay at 6 to 10 weeks. The biggest risk
left is no longer the code. It is visual parity: matching the browser
picture closely enough that players don't notice. The screenshot harness of
8.3 carries that, and it should be built in the first week.

### 21.12 Changes to earlier sections

- 8.2 said to start from the WGSL in the material plugins. It now says
  to lift only SHAG's own snippet bodies (21.6), and to port or rewrite
  Babylon's surrounding shader under the license rule (21.1, 21.9).
- 8.2 pass list: the view model clears depth, and the depth copy becomes a
  resolve step (21.3).
- 14, workspace: adds the `shagger/src/babylon/` folder, Apache-2.0.

---

## 22. Round 6: audio

Round 6 reads the audio path from a game event to the speaker. Sources:
SHAG's `src/engine/audio/AudioV2Sfx.ts` (455 lines) and `src/engine/AudioManager.ts`
(706 lines); wash-fps `client/FPS*Audio.ts`, `FPSAnnouncer.ts`,
`GameEvents.ts` and `shag/src/engine/loader/bsp/AmbientEntityHandlers.ts`; and
the published `@babylonjs/core@9.31.2` AudioV2 sources, read to settle
Babylon's defaults.

### 22.1 What reaches the speakers

```text
AudioContext destination
└─ main bus (Babylon)
   └─ sfx bus                 s_sfxvolume (0 when s_sfxenabled 0 or muted)
      ├─ sfx-effects          cg_volume_effects    weapons, impacts, world
      ├─ sfx-players          cg_volume_players    voices, steps
      ├─ sfx-hits             cg_volume_hitsound   hit and kill confirmation
      └─ sfx-announcer        cg_volume_announcer
HTMLAudioElement (not in the graph)   s_volume, s_musicenabled   music
```

- All volumes are linear gains from 0 to 1, and a sound's gain is the product
  of its play volume, its channel and the sfx bus.
- Babylon smooths every parameter change over 10 ms (`parameterRampDuration`).
- Music does not go through Babylon at all. It is a streamed
  `HTMLAudioElement`, faded by 0.02 every 50 ms (a 1.25 s fade at the default
  0.5 volume).
- There is no ducking, compressor or limiter anywhere. Loud moments simply
  sum, and the browser clips at the output.

### 22.2 Where a sound is placed

`AudioV2Sfx.spatialOptions` sets the distance model; everything else is
Babylon's default (`_SpatialAudioDefaults` in `abstractSpatialAudio.js`):

| Setting | Value | Source |
| --- | --- | --- |
| Distance model | `inverse` | SHAG |
| `refDistance` (minDistance) | 125 | SHAG `REFERENCE_DISTANCE` |
| `maxDistance` | 8000 (not used by `inverse`) | SHAG |
| `rolloffFactor` | the attenuation class (22.3) | SHAG |
| Panning model | `equalpower` | Babylon default |
| Cone | full circle, so no directivity | Babylon default |
| Doppler | none (Web Audio has none) | |
| Sound position | fixed when the sound starts; sounds do not follow entities | SHAG |
| Listener | the first active camera (the eye, not the weapon camera), updated every frame | SHAG `syncListener` |

The two formulas, from the W3C Web Audio specification, which Rust implements
directly as MIT code:

```text
distance gain (inverse):  g = ref / (ref + rolloff × (max(d, ref) − ref))
equal-power pan, mono source:
    azimuth a in degrees, from the listener's frame, folded to the front
    (a > 90 → 180 − a,  a < −90 → −180 − a)
    x = (a + 90) / 180          (stereo: (a + 90) / 90 for a ≤ 0, else a / 90)
    left = cos(x·π/2),  right = sin(x·π/2)
```

Stereo sources pan differently: the spec keeps both channels and moves gain
from one side to the other. The Rust mixer needs both cases, and the sound
package (290 files, 22.4) must be scanned once to find out which files are
stereo.

**A handedness trap that turns out not to matter.** Babylon's world is
left-handed, but Web Audio is right-handed. Babylon passes positions through
unchanged and points the listener with `RightHandedForwardReadOnly`
(`spatialWebAudioListener.js`). As a result, left and right come out correct,
but in front and behind are swapped. Equal-power panning folds the back onto
the front, so nobody can hear the swap. The Rust client must do the same: use
the left-handed right vector for panning, and ignore front and back. If HRTF
panning is ever added, on either client, the swap becomes audible and must be
fixed first.

### 22.3 Attenuation classes

`Attenuation` in `AudioV2Sfx.ts` uses Quake's classes as the rolloff factor:

| Class | Rolloff | Half volume at | Used by |
| --- | ---: | ---: | --- |
| None | 0 | everywhere full | |
| Distant | 0.5 | 375 | rocket and grenade fire, projectile impacts (default), quad spawn |
| Normal | 1 | 250 | default: other players' fire, steps, voices, reloads, teleports |
| Idle | 2.5 | 175 | plasma fire and plasma impacts |
| Static | 5 | 150 | map `target_speaker`-style entity sounds (`BspEntityHandlers.ts:346`) |
| Whisper | 10 | 137.5 | |

Half volume is where `g = 0.5`, which is `d = ref + ref / rolloff`. Maps can
set their own value, which is passed through.

### 22.4 What plays, and when

The sound package holds 290 files: 272 OGG Vorbis and 18 WAV
(`config/sound-package.json`, 20.4). About 27 call sites in wash-fps play
them:

| Module | Sounds | Trigger | 2D or 3D |
| --- | --- | --- | --- |
| `FPSCombatAudio.ts` | weapon fire (8 weapons, some with random variants), projectile impacts, hit confirmation by damage tier, pain and death, kill echo, empty clip | server events: `weapon_fire`, `projectile_impact`, `damage`, `death` | 2D for the viewed player, 3D for others |
| `FPSMovementAudio.ts` | steps by stride, ladder by rung spacing (44), jump grunts, landings, jumppads | derived each snapshot from player position, velocity and grounded state | 2D local, 3D others, with a `slot` per player |
| `FPSPickupAudio.ts`, `FPSPowerupAudio.ts` | pickups, quad pickup, spawn and fire | server events; quad fire is layered on your own shots | 2D, quad spawn 3D (Distant) |
| `FPSLiquidAudio.ts` | enter, exit, under water, splash by vertical speed, drowning | local prediction for the local player, snapshots for others | 2D local, 3D others |
| `FPSReloadAudio.ts` | reload | `weapon_state` changes | 2D viewed, 3D others |
| `FPSBombAudio.ts` | bomb timer beeps, blast, last-call lines | match state | 2D |
| `FPSAnnouncer.ts` | match flow, leads, awards | match state, through its own queue | 2D, announcer channel |
| `TeleportEffect.ts` | teleport in and out | events and local teleports | 3D others, 2D local |
| `AmbientEntityHandlers.ts` | map ambient loops and one-shots | map load | loop 3D or global |

**An observation, not a change for the port.** Your own fire sound plays
when the `weapon_fire` event arrives in a snapshot, so it lags your click by
about one round trip. The Rust client copies that for parity. Predicting it
locally would be an improvement for both clients, and it belongs upstream
first.

`ONE_SHOT_EVENTS` (`GameEvents.ts:263`) lists the events a demo seek skips.
The Rust event dispatcher keeps that same set, so seeking never plays a burst
of sounds.

### 22.5 Voice limits

| Rule | Value | Where |
| --- | --- | --- |
| Spatial one-shots playing at once | 48. Over the cap, cut the one **closest to its end** | `MAX_ACTIVE_SPATIAL_SOUNDS` |
| Per-slot exclusivity | a new sound in a slot (for example `<entity>:pain`) cuts the previous one | `slot` option |
| 2D sound instances per sound and channel | `maxInstances` per call: 8 by default, 1 to 6 at the call sites. Over it, the oldest started instance stops | Babylon `AbstractSound.play` |
| Announcer | one line at a time; the rest waits in a queue | `FPSAnnouncer.queue` |
| Muted while away | `setMuted` bumps an epoch, so sounds still loading never start | `AudioV2Sfx.setMuted` |

These rules decide what you hear in a busy fight, so the Rust mixer copies
them exactly. They are about 60 lines of logic.

### 22.6 Music

- **Playlists** are JSON files (`{ name, loop, shuffle, tracks: [{ file, title }] }`).
  The menu uses `data/menu.playlist.json`, and maps can have their own.
- **Shuffle** is Fisher-Yates with `Math.random`, done once when a playlist
  starts. The next track starts when the current one ends.
- **Switching playlists** stops the old one at once, with no crossfade. A single
  track fades in and out by the 0.02-per-50-ms steps above.
- **Mute** keeps one named playlist silent without turning music off.

Rust streams music through symphonia from the cached pk3 or the data URL,
decoding into a ring buffer, on the same mixer as the sound effects but on its
own bus. That also gives music the 10 ms parameter smoothing for free.

### 22.7 Library choice

| Option | Exact Web Audio panner and inverse model | Voice rules of 22.5 | Weight | Verdict |
| --- | --- | --- | --- | --- |
| `kira` | no: its spatial tracks use their own attenuation and panning curves | partly | medium | not for gameplay sound |
| `rodio` | no spatial model to speak of | no | small | no |
| **Own mixer on `cpal`** | yes, written from the spec | yes | about 800 lines | **chosen** |

The mixer, `shagger::audio`:

```text
decode     lewton (Vorbis) and an own WAV reader → f32 PCM, resampled once
           to the device rate (linear for now; rubato or a sinc later),
           cached per path like getBuffer()
mixer      runs on cpal's audio thread; no allocation, no locks on that thread
           voices: one-shot | loop | stream, each with gain, bus, optional
           position, slot, and end time
           buses:  sfx → effects / players / hits / announcer, music
           every gain change is ramped over 10 ms
control    game thread → audio thread through a lock-free queue (rtrb):
           Play, Stop, SetListener, SetBusGain, SetMuted
spatial    per voice per audio block: inverse distance gain, equal-power pan
           from the listener frame of 22.2
```

All of this is MIT. Nothing is ported from Babylon: the behaviour comes from
the Web Audio spec and SHAG's own rules. So the license rule of 21.1 does not
touch audio.

### 22.8 Parity test

Rust plays audio in real time, so the test renders offline. Both clients take
the same list of timed events (sound, position, listener position and
orientation, and volume) and render it to a stereo WAV:

- the browser with an `OfflineAudioContext`, through `AudioV2Sfx` with the
  same buses;
- Rust through the mixer without `cpal`.

Then compare RMS per channel in 10 ms windows. The script holds:

- one sound walked through distances 0 to 4000 for each attenuation class;
- one walked around the listener at 30-degree steps, to check panning and
  the front-back fold;
- a 60-voice burst, to check the cap and which sounds are cut.

The harness lives next to the golden protocol vectors of 18.6.

### 22.9 Estimate

Round 1 gave 2 to 3 weeks for milestone 8. With the reading above:

| Work | Weeks |
| --- | ---: |
| Mixer, buses, ramps, spatial model, voice rules, cpal device handling (hot-plug, sample rates) | 1.5 |
| Decode cache, music streaming, playlists, cvars | 0.5 |
| Port the `FPS*Audio` modules, the announcer and ambient entities | 1 |
| Offline parity harness | 0.5 |
| **Total** | **3.5** |

That is slightly over round 1's range, because round 1 assumed `kira` would
fit and did not count the parity harness.

### 22.10 Changes to earlier sections

- 9 (audio layer): replace "kira" with the custom mixer of 22.7, and add the
  music path.
- 15, milestone 8: 3.5 weeks.

---

## 23. Progress log

Newest first. Each entry names the commits and what proves them.

### Milestone 9, third slice: crosshair, vitals and ammo (November 2, 2026)

- **wash-fps-rust.** `HudState` gains `HUDState.ts`'s combat and weapon
  half (`processCombatState`, `processWeaponState`, `currentClip`) for the
  player on screen, fed from `combat_state` and `weapon_state`.
  `client/ammo_status.rs` ports `AmmoStatus.ts`. The weapons table gains
  `clipSize`, `maxAmmo` and `crosshair`. New widgets in the bottom bar:
  `VitalsBlock` (the red HP slab with its number and bar, the four armor
  plates, the critical flash) and `AmmoCounter` (clip or pool, a clip
  gun's reserve, the weapon's name, the bar). `client/crosshair.rs` ports
  `Crosshair.ts`: the weapon's style, the gap opened by speed, red for
  200 ms on a hit (300 on the head), the red cross when the gun is empty.
  The bottom bar hides for a spectator flying free.
- **Slabs with hard stops.** CSS gradients like `red 62%, dark 63%` are
  cut along the stop line (Sutherland-Hodgman on the clip polygon), so the
  edge stays sharp instead of blending across the slab.
- **Proof.** Unit tests (ammo status); screenshots of the dm14 demo behind
  a player (HP 70, Voltaic 71 with 60 in reserve, the dot red on a hit)
  and of `washoi play` (HP 100, blade).
- **Left out.** The flashes on a change, the tilts and the cockpit yaw,
  aiming down sights and the spread bloom in the crosshair, the weapon
  slots, the ammo warning, the powerup timer and the air meter.

### Milestone 9, second slice: playing in the window (November 2, 2026)

- **shagger `130f012`.** `NetworkGameHost::send_request_spawn` and
  `SessionRequest::RequestSpawn` (`Game.sendRequestSpawn`).
- **wash-fps-rust.** `client/live_play.rs` is the player's side of a live
  match: it joins, installs the map's prediction (collision, triggers,
  spawn facings), and on every frame drains the session, buffers our own
  snapshots for `V2NetworkClient` (reset on respawn, weapon speed bonus
  from `weapon_state`), and sends commands at 125 Hz with the newest
  snapshot tick, predicting from the f32 values the wire carries.
  `command_for` builds `sendInput`'s buttons and world direction. While
  dead or frozen the input is stripped; fire or jump asks to respawn after
  `GameEvents`' three seconds. The server's placement at a spawn turns the
  view to the spawn's facing (`faceSpawn`).
- **`washoi play <ws://server>`** opens the window first person: WASD,
  Space, C or Ctrl, Shift, the mouse to look, left and right buttons to
  fire, R, E and 1 to 9 for weapons. The eye is the predicted position
  plus the view height and the crouch offset; other players, sounds and
  the HUD come from the same feed as watching, with our entity as the
  view's. With `--screenshot` it runs forward and fires for `--time`
  seconds and prints where it ended up.
- **Proof, live against a local `net:server` on `washoi_dm2`** with three
  bots: the client joins, spawns facing the spawn point's way, runs about
  380 units in 1.5 seconds and draws the first-person frame with the HUD.
- **Not yet.** The view-model weapon, crosshair, health, armor and ammo;
  our own sounds (footsteps, jumps, weapon fire before the server echoes
  it).

### Milestone 9, first slice: the HUD's 2D layer and the top of the HUD (November 2, 2026)

- **shagger `e16cdc8`, `86d97bf`.** `ui/` holds the 2D layer section 11
  asked for: `Font` (swash, MIT or Apache-2.0; fontdue would bring back the
  unmaintained `ttf-parser`), a shelf-packed glyph atlas, and `Hud2d`, a
  frame of rectangles, outlines, gradients, convex polygons and text
  (letter spacing, alignment, hard shadows) in CSS pixels.
  `rendering/hud_2d_renderer.rs` draws it over the output after image
  processing (`begin_ui_pass`, `render_hdr_with_ui_to_rgba`).
- **wash-fps-rust.** `client/hud_state.rs` ports `HUDState.ts`'s
  scoreboard half (sorted by score, final standings after the match,
  spectators). `client/hud/washiki.rs` carries `WashikiTheme.css`: colours
  as `hsla`, `--hud-zoom`, the ink shadows, and the hand-cut slabs (a
  `clip-path` polygon over a `linear-gradient`, `skewX`, a hard
  `drop-shadow`). Widgets keep `FPSHudWidget`'s shape (`update`, then
  `size` and `draw`): `Timer` (idle, warning gold, critical red flash),
  `FragCounter`, `GametypeBadge`, `PhaseMessage` (warmup, 3-2-1, fight,
  result, overtime) and `Scoreboard` (free-for-all and team lists; Tab in
  the window, `--scoreboard` on a screenshot, locked open after the
  match). `client/fps_hud.rs` lays out the top-centre row as `FPSHud.ts`
  does. The fonts ship in `assets/fonts` (Orbitron Black and Rajdhani
  Bold, SIL Open Font License 1.1, its text beside them).
- **Proof.** Unit tests (CSS `hsla` and gradient angles, the atlas
  packing, the scoreboard's order); screenshots of the dm14 demo show the
  frag wing, clock and gametype tab, and the scoreboard over the match.
- **Then.** `HUDState`'s kill feed (`processDeath`, five entries for five
  seconds, names resolved from the roster), SHAG's `EventFeed` at the top
  left ("killer [weapon] fragged victim", six for six seconds and a
  one-second fade; Washoi's styles, so it lives in wash-fps-rust) and
  `FragMessage` ("FRAGGED name" floating up at the centre when the
  followed player scores). `FpsHud` keeps the state across frames.
- **Left out.** Scanline overlays, perspective tilts and blurred glows;
  the post-game stage and the duel's versus card. Vitals, ammo and the
  crosshair belong to a player in play, which the window does not have
  yet.

### Milestone 8, fifth slice: the bomb's sounds (November 2, 2026)

- **wash-fps-rust `9df76a1`.** `client/fps_bomb_audio.rs` ports
  `FPSBombAudio.ts`: the beep on the server's interval, the side-read
  lines on bomb phase changes, the round start and go, the spoken 3-2-1,
  and the last call for the side running out of time. Spectating, the
  lines are read for the side of the player followed. The detonation's
  sound belongs to `BombSiteMarkers.ts` and comes with the bomb's visuals.
- **Proof.** Unit tests: the beep follows the interval; the last call
  comes once a round, when the fuse leaves a defuse and five seconds.

### Milestone 8, fourth slice: music playlists (November 2, 2026)

- **shagger `cac2376`, then `e16cdc8`.** `audio/music.rs` ports `AudioManager.ts`'s music
  half: `Playlist` from `*.playlist.json`, `startPlaylist` (same name
  skipped, Fisher-Yates shuffle, music off remembers it), `stopPlaylist`,
  missing tracks skipped (the playlists list
  `Noodlesoup_Telefragged_Me.mp3`, which no package holds), no fade
  between tracks, the 0.02-per-50-ms fades, `s_volume`, `s_musicenabled`
  and muted playlists. MP3s decode with `rmp3` (CC0 around the CC0 C
  library; the `minimp3` crate pulls in `slice-ring-buffer`,
  RUSTSEC-2025-0044, and `nanomp3` needs Rust 1.89) and stream through an rtrb ring into new
  stream voices on the music bus, which the sound effects' `StopAll`
  leaves alone. A thread decodes ahead on a sound card; offline, `update`
  decodes, so a render to a file is the same every run.
- **wash-fps-rust.** `GameAudio` reads `game`, `endgame` and `menu`
  `.playlist.json` beside the packages (`tools/fetch_maps.sh` fetches
  them), loads the packages holding their tracks, plays the game playlist
  while watching (`DemoService.ts`) and the endgame's in PostGame
  (`Game.ts`'s `watchMatchPhase`).
- **Proof.** Unit tests (parse, shuffle keeps every track, a playlist
  with nothing to play stops); with `SHAGGER_TEST_MP3` set, a real track
  fades in to the 0.5 volume and a cut one ends its playlist. 20 s of the
  dm14 demo with and without the game playlist differ by the music.

### Milestone 8, third slice: the match state and the announcer (November 2, 2026)

- **wash-fps-rust.** `client/match_state_manager.rs` ports
  `MatchStateManager.ts` (phase, clock with `cl_invertClock`'s count-up,
  ready count, scores, teams, rosters kept when the server omits them,
  winner, overtime, flow, rounds, `isMovementFrozen`); objectives, round
  results and explosions stay JSON until Search and Destroy. The snapshot
  feed keeps one, so the HUD can read it next. `client/fps_announcer.rs`
  ports `FPSAnnouncer.ts` (the held voice and its queue, countdown, fight,
  lead, overtime, game over, ready-up), with `leadOf`; its random read
  choice draws from mulberry32 instead of `Math.random`. Spectating, the
  announcer calls the match flow but never a lead, as `Game.ts` does for a
  player not in the match. 95 sounds load.
- **Proof.** Unit tests port the TypeScript cases (a lead change waits for
  the voice, then speaks; a one-player team wins as its player; an omitted
  roster keeps the last). The screenshot path prints the match: the dm2
  demo of November 2 ends at "Active 8:05 (Deathmatch)".

### Milestone 8, second slice: movement, pickup and powerup sounds (November 2, 2026)

- **wash-fps-rust.** `FPSMovementAudio.ts` (steps by stride on the ground's
  surface flags, traced 48 units down through the map's brushes; ladder
  rungs; jump grunts; the jump pad cue), `FPSPickupAudio.ts` and
  `FPSPowerupAudio.ts` (pickups for the player on screen, the quad's
  pickup, fire layer and its spawn heard by everyone). The feed hands over
  each arrived snapshot's players as sent, so the movement sounds run per
  snapshot as `processFPSMovementAudio` does, not per frame; `pickup_notify`
  and `powerup_spawn` reach the sounds too. 68 sounds now load.
- **Proof.** The port of `FPSMovementAudio.test.ts`'s cases (a stride steps,
  a rising edge grunts, gone players are forgotten) and of the pickup label
  table; the dm2 demo renders 30 s of sound with all banks loaded.
- **Not yet.** The jump pad cue and the quad fire layer belong to the local
  player's prediction, which plays no sounds yet.

### Milestone 8, first slice: the mixer and the combat sounds (November 2, 2026)

- **shagger 50f95dd.** `shagger::audio` as 22.7 plans it: `spatial.rs` (the
  inverse distance model and equal-power panning from the Web Audio spec,
  with Babylon's right-handed listener forward), `mixer.rs` (the sfx bus,
  its four channels and music, 10 ms linear gain ramps, 128-frame quanta,
  and the voice rules of 22.5), `decode.rs`, `device.rs` (cpal, fed through
  an rtrb queue, so the audio thread neither locks nor allocates) and
  `audio_v2_sfx.rs` (`AudioV2Sfx`: `play_2d`, `play_3d`, `play_loop_3d`,
  volumes, mute and listener, to the sound card or an offline mixer).
  CI installs `libasound2-dev`; the MSRV job builds again.
- **Decisions.** `lewton` replaces `symphonia` (MPL-2.0); resampling is
  linear until the parity harness says it matters. Sounds play only once
  preloaded, as the combat bank is in SHAG. 22.2's mono pan formula is
  corrected to the spec's `(a + 90) / 180`.
- **wash-fps-rust.** `FPSCombatAudio.ts`, `FPSPlayerAudio.ts` (voice sets
  at the cvars' defaults, `playerVoiceForEntity`) and `FPSReloadAudio.ts`,
  plus `getPlayerTeamSlot`. `GameAudio` in `client/game_events.rs` plays a
  snapshot's `weapon_fire`, `projectile_impact`, `damage` (pain, and the
  hit confirmation for the viewed attacker), `death` (and the kill echo)
  and `weapon_state` (reload) as `GameEvents.ts` does, heard from the
  followed player. The window plays a demo's or match's sound on the
  sound card; `washoi demo ... --screenshot X --audio-out Y.wav` mixes it
  offline.
- **Proof.** Mixer tests: half volume where each attenuation class says,
  left and right for a left-handed camera, the 10 ms ramp, the 48-voice
  cap cutting the nearest end, slots, `maxInstances`. The dm2 demo's first
  20 s render to a WAV in 3.7 s with all 50 combat sounds loaded; from a
  free camera the channels differ (panning), following a Voltaic player
  their own fire plays 2D.
- **Not yet.** The browser parity harness of 22.8, music and playlists,
  the movement, pickup, powerup, liquid, bomb and announcer modules, map
  ambient speakers, cvars, and the local player's own sounds in `--play`.

### Milestone 7, fourth slice: other players' shots (November 2, 2026)

- **shagger 13c4be2.** `weapon/hitscan_effects.rs` ports
  `HitscanEffects.ts` as one owner of the streak, rail, Voltaic, muzzle
  flash and impact spark renderers: `fire_tracer` (the streak, then the
  impact light and sparks when it arrives), `fire_pellets`, `fire_rail`
  with its sparks, the remote Voltaic beam and its traced fallback,
  `update` and `clear`. TypeScript's singletons become arguments: shots
  trace through a `ShotWorld` (implemented for `SweepCollisionBackend`,
  SOLID with a point hull, NOIMPACT and NOMARKS honoured) and lights come
  from the caller's `PointLightPool`. `spread_utils` gains
  `generate_spread_directions`, `yaw_pitch_from_direction` and
  `compute_spread_seed`, so the riot gun's pellets are rebuilt from the
  event's seed. Only `sin` and `cos` go through fdlibm there, so a
  rebuilt pellet may differ from the server's in the last bit; it is
  drawn, not judged.
- **wash-fps-rust.** `client/game_events.rs` ports `WEAPON_EFFECT_COLORS`,
  `canonicalWeaponId` and `handleRemoteWeaponFire` (with the
  `voltaic_beam` truth check), `handleVoltaicBeam` and `handleElectroRail`.
  The snapshot feed now passes on `weapon_fire` and both beams, and tracks
  each player's weapon from `weapon_state` for the beams' colours.
  `MapRenderer` keeps the map's brushes for the shots to stop on.
- **Proof.** The demo test now also needs a shot fired. In the amphi1
  demo at 0.43 s, three machinegun streaks fly with 15 sparks and two
  impact lights; in dm2 at 1.5 s a remote Voltaic beam is live. No golden
  demo has a riot gun shot, so the pellets are proven only by the cone
  test against V8.
- **Differences.** A traveling pellet is a short streak, not a cone mesh;
  wall marks, the rendered-mesh pick for a shot starting inside a brush,
  and the blade's slash animation are not ported. Streaks start at the
  shooter's eye until remote players hold weapons.

### Milestone 7, third slice: impacts and rails (November 2, 2026)

- **wash-fps-rust.** The snapshot feed passes on the events with an
  effect, and `MapRenderer::play_effects` runs the effects' own clock and
  starts them as `GameEvents.ts` does: `projectile_impact` explodes with
  the projectile's look (older demos' weapon ids mapped, anything else a
  rocket) and its flash light, faded by `explosion_flash` and given back
  when out; `electro_rail` draws the Electrobolt rail in its effect colour
  with the server's endpoint, normal and impact kind. Held effects
  (`--explosion` and the like) keep the clock at 0 as before.
- **Trails.** A flying projectile pushes a trail point each frame
  (`updateTrail`); a finished trail keeps its uploaded ribbons and serves
  the next projectile of its type (shagger `ProjectileTrail::reset`), where
  SHAG disposes the meshes.
- **Proof.** In the dm2 demo at 2.55 s, plasma bolts burst against the
  far wall in green with their light, beside one still in flight.
- **Not yet.** Hitscan tracers and muzzle flashes (`weapon_fire`, which
  needs `handleRemoteWeaponFire`'s trace), the Voltaic beam and chain,
  impact sparks, pickups and the HUD. (Shots and beams came in the fourth
  slice.)

### Milestone 7, second slice: watching a live match (November 2, 2026)

- **wash-fps-rust.**
  - `client::snapshot_feed` holds what demo playback and a live match share:
    the players' buffer as `RemotePlayerRenderer.pushSnapshot` feeds it
    and the projectiles' as `processProjectileEntities` does.
  - `client::live_watch` connects as a spectator, waits for the welcome
    (which names the map) and pushes each snapshot as it arrives. Snapshots
    are stamped with their arrival; the server-time bracketing through clock
    sync is not wired yet, so the delay follows the arrival jitter, as the
    browser's does before its clock syncs.
  - `washoi watch <ws://server>` opens the window on the server's map and
    draws its players and projectiles live. `--follow N` (and F in the
    window) puts the camera behind a player.
- **Proof, live against a local `net:server` on `washoi_dm2`** with three
  idle bots and a Rust client running circles (`connect --play`): the
  spectator receives about 60 snapshots a second and draws all four,
  the runner animating as it runs, chased from behind.
- **Not yet.** The same as demos (effects, trails, pickups, HUD), the
  lobby's spectate path into the window, and a first-person view of the
  followed player.

### Milestone 7, first slice: demos play in the window (November 2, 2026)

- **wash-fps-rust.**
  - `client::remote_player_logic` ports `extractEntities`, `applyRoster` and
    `isClientKey`.
  - `client::demo_playback` reads a `.shagdemo` and feeds each frame to
    shagger's `SnapshotBuffer` at the moment it was recorded, then samples
    every player each frame. wash-fps's deterministic `DemoTimeline`,
    seeking and time scale are not ported; with no jitter the buffer
    brackets the frames as the timeline would, a delay behind.
  - `MapRenderer::set_live_players` poses and places the live players each
    frame on top of the static draws (their joint palettes after the static
    ones), styled as a spectator sees them (the FFA slot; per-player FFA
    colours are not ported).
  - Projectiles fly too: `projectile_spawn` names each `proj_` entity's
    type, a second buffer interpolates them (`processProjectileEntities`),
    and `MapRenderer::set_live_projectiles` draws every type's MD3 facing
    along its velocity with its pool light. Trails are not drawn yet.
  - `washoi demo <file.shagdemo>` plays the match on its map in the window
    with the free camera, or with `--screenshot` draws it `--time` seconds
    in and prints where each player is (Quake axes, for `--at`).
- **Proof.** The dm2 deathmatch demo (3 players, 15,885 frames, 254 s)
  plays through; at 20 s a bot is drawn mid-jump with its outline and tint
  where the recording puts it, a plasma bolt in flight beside it. A test
  plays the recorded bot demos end to end and checks the players move and
  projectiles fly.
- **Not yet.** Projectile trails, beams, hitscan, explosions and pickups
  from the demo's events, following a player's view, the HUD, and watching a live
  match in the window (`washoi lobby … spectate` still runs headless).
- **Next.** Demo events (effects) and live spectating.

### Milestone 4: prediction and reconciliation (November 2, 2026)

- **shagger `331ec69`.** `network::client::snapshot_buffer` ports
  `SnapshotBuffer.ts` whole (stale-tick rejection, the jitter-adaptive
  delay, server-time bracketing shifted by the measured transit, teleport
  snap, shortest-path yaw, extrapolation capped at 60 ms, frame sampling);
  the caller passes the clock and the two cvars.
  `shared::bsp::spawn_point_table` ports the spawn points and facings.
- **wash-fps-rust.**
  - `simulation::movement_profile` gains `saveState` / `loadState`.
  - `client::input_history` ports `InputHistory.ts`;
    `client::v2_network_client` ports `V2NetworkClient.ts`: sanitize, lifts,
    knockback drag and movement per command; confirm a snapshot against the
    prediction stored for the command it acknowledges, else adopt authority,
    resume the acknowledged movement state and replay; the ledge-grab and
    wall-jump suppression with its time, divergence and knockback releases;
    the two-second replay budget and resync; the decaying visual offset; the
    spawn facing. `NetGraph` is left out; its numbers are in `DebugInfo`.
  - `client::prediction` installs the map (collision, lifts, triggers,
    spawn facings, from the packages in `--data`) and builds each
    `ServerSnapshot` as `GameEvents.ts` does (`lift_state`,
    `knockback_state`). The weapon sync sets the speed bonus
    (`weapon_speed_bonus`).
  - `washoi connect … --play` predicts its commands with the f32 values the
    wire carries (19.6) and reports confirmed and corrected snapshots.
    `WASHOI_PREDICT_DEBUG=1` prints each miss.
- **Proof, live against a local `net:server` on `washoi_dm2`.** Running and
  turning for 14 s: 735 snapshots confirm the prediction, 2 are corrected
  (the first snapshot and the one before the weapon sync arrived), no
  resyncs. Before the speed bonus the server ran 10 u/s faster with the
  blade and nearly every snapshot was corrected, so the check bites.
- **Not yet.** Combat (knockback, death and respawn) against bots, a real
  network's latency, and the production servers, whose game ports the build
  environment cannot reach.
- **Next.** Milestone 7: watch a live match or a demo in the window.

### Milestone 6, twentieth slice: pipeline precompile (November 2, 2026)

- **shagger `ff6abf8`.** `ModelRenderer::precompile` builds every pipeline
  the uploaded models can take (mirrored or not, outlined or not) from the
  same `pipeline_keys` that `prepare` uses. The world, sky, effect and
  explosion renderers already built theirs when created.
- **wash-fps-rust.** `washoi view` precompiles after loading the map's
  models and the view model: 32 model pipelines in about 0.2 s on dm2 with
  a player and the plasma gun.
- **Not yet.** A `wgpu::PipelineCache` saved between runs (Vulkan only).
- **Next.** Prediction (milestone 4).

### Milestone 6, nineteenth slice: the weapon in hand (November 2, 2026)

- **shagger `34e1f90`.**
  - `weapon::weapon_bob` and `weapon::weapon_recoil` port `WeaponBob` and
    `WeaponRecoil` whole (the `cg_gun_bob_*` defaults, the ADS recoil
    multiplier).
  - `weapon::weapon_view_manager` ports the placement half of
    `WeaponViewManager`: the hip offset at `cg_handedness`, the ADS lerp
    (`ads.time` or `cl_ads_time`, linear when `adsRequireReady`), the GLB
    root (scale (1, 1, -1), rotation (0, -π/2, 0), the blade's own), the
    weapon camera (`cg_weaponfov` 60°, clip 0.1 to 1000), `weaponLight`
    and `bladeLight`, the material modes (`_glow` as added light, `_emit`
    as an unlit two-sided StandardMaterial, `_glass` at alpha 0.35) and
    the outline width.
  - `model.wgsl` adds Babylon PBR's GGX specular (Schlick fresnel from
    `mix(0.04, albedo, metallic)`, Trowbridge-Reitz, height-correlated
    Smith) for hemispheric, directional and point lights.
  - `rendering::offscreen` draws an overlay pass that clears depth, as
    rendering group 1 does.
- **wash-fps-rust.** The weapons table carries each model's view placement.
  `client::weapon_view` loads the gun and draws it over the world in the
  window (right mouse aims) and in `washoi view --weapon ID [--aim]`.
  `tools/parity` takes `WEAPON=`.
- **Parity** (`washoi_dm2`): the plasma gun stands, turns and is lit as in
  the browser; the view is 9.8, 6.3 and 5.5 levels off, mostly the
  browser's hidden health pickup. The specular also brings the player views
  from 4 to 8 levels down to about 2.
- **Not yet.** Muzzle flash and fire animations, the switch animation, the
  metallic-roughness and normal maps on models (the sight glass is darker),
  and the blade's environment reflection (it draws black).
- **Next.** Pipeline precompile at load, then prediction (milestone 4).

### Milestone 6, eighteenth slice: glow maps (November 2, 2026)

- **shagger.** `world_renderer` loads a surface's glow image as
  `loadTextures` does (`loadOptionalTexture` with no suffixes; the shader
  registry already resolves `_glow` images and glow stages):
  - on a lit StandardMaterial it adds as the emissive in display space, and
    on the generated PBR material as linear emissive (`GAMMAEMISSIVE`);
  - on a fullbright surface it replaces the base image as the emissive,
    since lighting is disabled there;
  - with an animated glow stage (`glowStage`) it draws instead as
    `registerGlowAnimation`'s shine: a second pass, unlit, `ALPHA_ONEONE`,
    no depth write, polygon offset, stepped by that stage's `tcMod`,
    `rgbGen` and `animMap`.
- **Parity** (`washoi_dm2`; glow images on the lamp panels, light strips,
  teleporter and jump pad): the teleporter alcove at 1.6 levels, the mega
  health floor at 1.6, the jump pad view at 2.2. What is left there is
  the browser's teleporter sparkle and its hidden health pickup.
- **Next.** The remaining 21.10 items.

### Milestone 6, seventeenth slice: companion surfaces and world lights (November 2, 2026)

- **shagger.**
  - `loader::bsp::mesh::tangents` ports `buildBspTangents` (Qfusion's
    image-space frame), proved against V8 with a quad, a mirrored-UV and a
    degenerate-UV triangle. Every world vertex carries its tangent.
  - `world_materials` ports `loadOptionalTexture`'s candidates (an explicit
    image, a blank, or the found base image with `_norm`/`_normal`/`_bump`
    and `_gloss`/`_spec`), `shouldUseGeneratedPBR`, and the gloss
    defaults (intensity 1.5, exponent 24, microsurface
    `1 - sqrt(2 / (e + 2))`).
  - `world.wgsl` draws a lightmapped pass's companions:
    - the normal map through Babylon's bump include (`perturbNormal` on
      the tangent frame);
    - BspBakedLightingPlugin's baked gloss (`bspSpec`, Blinn-Phong on the
      deluxe direction, gloss map times intensity);
    - StandardMaterial's dynamic specular, with the gloss map as the
      specular colour (`specularTexture` replaces `specularColor`, so the
      intensity drops out there, as in Babylon);
    - the generated specular-gloss PBR material: baked terms multiplied in
      display space, dynamic lights through Lambert diffuse (EON at zero
      diffuse roughness) and height-correlated Smith GGX, with the standard
      linear falloff. Babylon's multiple-scattering energy factor needs the
      environment BRDF lookup, and is left out.
- **wash-fps-rust.** The world takes the dynamic lights.
- **Parity** (`washoi_dm2`):
  - Static views: the far corridor goes from 4.5 to 0.8 levels (it was the
    "three levels darker" gap); the pickup view from about 11 to 8, the rest
    being the browser's hidden health pickup and its red debris.
  - `LIGHTS=1` held projectiles: from 4.3 to 0.2 levels.
  - `LIGHTS=1` explosions: from about 7 to 4 levels.
- **Next.** Glow maps (`glowTexture` emissive and the shine overlay), then
  the remaining 21.10 items.

### Milestone 6, sixteenth slice: dynamic lights, models first (November 2, 2026)

- **shagger.**
  - `weapon::point_light_pool` ports `PointLightPool`: eight effect lights,
    the oldest stolen when all are taken, a stolen holder's handle going
    stale.
  - `rendering::dynamic_lights` is what Babylon's `ClusteredLightContainer`
    hands materials: position and range (capped at 16383), diffuse and
    specular times intensity. Every surface loops over all of them. The
    browser culls by screen tile and depth slice, but each light already
    stops at its range.
  - `standard.wgsl` adds them through `computeLighting` (linear falloff to
    the range, Blinn-Phong specular). `model.wgsl` adds them to the PBR
    diffuse sum with an inverse-square falloff inside the range.
    `world.wgsl` can add them to `diffuseBase` on lightmapped surfaces, as
    BspBakedLightingPlugin's StandardMaterial path does.
  - `explosion_effect::explosion_flash` is the flash light's fade.
- **wash-fps-rust.** Held projectiles carry their definitions' lights and
  explosions their flashes, through the pool. `--no-lights` turns them off,
  and `tools/parity` keeps them in both only with `LIGHTS=1`.
- **Found: the world needs its companion surfaces first.** A probe of the
  browser's `washoi_dm2` shows 27 of its lightmapped BSP materials are
  PBRMaterials (`shouldUseGeneratedPBR`: surfaces with a normal map and a
  gloss map, specular-gloss workflow, reflectivity 1.5, `usePhysicalLightFalloff`
  off) and 20 are StandardMaterials. washoi view draws every world surface
  without its normal and gloss maps, and so without the baked gloss
  specular (`bspSpec`) either. A dynamic light on the PBR ones goes through
  PBR's BRDF on linear albedo and reads as a tight hot spot. The same light
  through StandardMaterial washes a whole wall. So the world's dynamic
  lights stay off until those surfaces are ported. The models take theirs
  now.
- **Next.** The companion surfaces: normal maps, gloss maps and the
  generated PBR world materials, with their baked gloss. Then the world's
  dynamic lights.

### Milestone 6, fifteenth slice: rail and lightning beams (November 2, 2026)

- **shagger.**
  - `weapon::rail` ports the Electrobolt rail (`RailEffectRenderer`,
    `RailBeamGeometry`, `RailShaderMaterial` as `rail_beam.wgsl` and
    `rail_impact.wgsl`): glow, core and a twin analytic helix, with the
    muzzle crack and terminal nucleus.
  - `weapon::voltaic` ports the Voltaic lightning beam: the local hero beam
    (two crossed sheets, drawn as a batch of one where the browser binds
    uniforms), remote players' beams with their muzzle resolver, and chain
    arcs keyed by their quantised endpoints (`js_round`). The bolt field,
    forks, endpoint crowns and contact sparks follow the GLSL line by line.
  - `make_voltaic_seed` (FNV-1a over UTF-16 units) is golden against V8.
  - `EffectQuadBatch::with_template` draws any template mesh, with a fifth
    vec4 per instance (`iNormal`).
- **wash-fps-rust.** `washoi view --rail` and `--lightning hero|remote|chain`
  hold each at an age; `tools/parity` spawns the same through SHAG's
  `HitscanEffects` renderers (`EFFECTS=rail:…;lightning:…`).
- **Checked against the browser.** Rails: the helix, core and endpoints
  land on the same pixels; the view differs by 6 levels, most of it the
  procedural pickup the browser view leaves out. Lightning: every bolt,
  fork, ghost and the chain arc's contact crown match in shape and colour
  in two views, 5 to 7 levels apart.
- **Not yet.** Atmospheric fog on effects, the beam lights, wiring the
  effects to snapshots and events.
- **Next.** Dynamic and projectile lights (section 21.10's forward light
  buffer).

### Milestone 6, fourteenth slice: the HDR target and image processing (November 2, 2026)

- **shagger.**
  - `babylon::post_processes::image_processing_post_process` ports
    `ImageProcessingConfiguration` (exposure, contrast and the vignette, with
    `bind`'s scale and stretch maths, unit tested) and the post-process's
    shader: from linear colour, exposure, vignette (multiply or opaque),
    gamma, saturate, contrast. Apache-2.0, with a `NOTICE` entry.
  - `rendering::hdr_target` is the Rgba16Float colour and depth the scene
    renders into, as SHAG's `DefaultRenderingPipeline` (`hdr: true`) sets
    up. `offscreen::render_hdr_to_rgba` renders a screenshot the same way.
  - Every material now hands the target what Babylon's do when image
    processing runs as a post-process: the world, sky and
    `StandardMaterial`s take their colour to linear space (pow 2.2), the
    PBR models clamp linear colour to 30, the explosion batch takes its
    `uLinearOutput` path, and the effect quads and outline stay raw.
    `ModelRenderer::set_image_processing` is gone; no shader does image
    processing itself any more, so blending happens in linear colour as in
    the browser.
- **wash-fps-rust.** Every renderer draws in `HDR_FORMAT`; the window and
  `--screenshot` resolve the target through SHAG's configuration
  (`PostProcessingManager`'s defaults: contrast 1.05, exposure 1, vignette
  weight 0.3, stretch 0.5, black, multiply).
- **Checked against the browser** with `tools/parity` on `washoi_dm2`. The
  vignette's falloff matches the browser's region for region (the corners
  and bottom edge now agree to within a level, where washoi view used to be
  2 to 4 levels brighter there). The streaks, flashes and sparks lost their
  greyer cores and now look like the browser's; the views differ by 6
  levels (the world alone by 4 to 7, much of it the procedural pickup the
  browser view leaves out without `ITEMS`). Explosions: 5 to 6 levels.
  Projectiles, trails and pickups (`ITEMS=1.7`): 4 levels in both views,
  down from 5.7 to 7. Rust is still 3 to 4 levels darker overall.
- **Not yet.** Bloom (off in the browser at SHAG's defaults), glow,
  chromatic aberration and colour correction (milestone 11), MSAA.
- **Next.** Rail and lightning beams, then dynamic and projectile lights.

### Milestone 6, thirteenth slice: bullet streaks, muzzle flashes and impact sparks (November 2, 2026)

- **shagger `44a1a82`.**
  - `weapon::tracer`, `weapon::muzzle` and `weapon::impact` port
    `StreakTracerRenderer`, `MuzzleFlashRenderer` and
    `ImpactSparkRenderer` with their materials' GLSL in WGSL: pooled
    instances of one quad that the vertex shader animates from the spawn
    time. The impact scatter draws from mulberry32 seed `0x5eed` as the
    browser's does.
  - `rendering::effect_quad_batch` is what Babylon does for those
    thin-instanced quads: the quad, four vec4s per instance, the
    `ShaderMaterial` uniforms (`effect_quad.wgsl`), premultiplied alpha, no
    depth writes. `EffectSlots` is the shared slot machinery.
  - `shared::spread_utils` ports `mulberry32`, golden against V8.
- **wash-fps-rust.** `washoi view --tracer`, `--muzzle` and `--impact` hold
  each effect at an age.
- **Checked against the browser** with `tools/parity` (`EFFECTS=`), which
  spawns the same effects through SHAG's `HitscanEffects` renderers on a
  held clock. Every streak, flash and ember lands where the browser puts
  it, and the view differs by 6 levels in 255. The browser's cores read
  whiter. The browser runs `DefaultRenderingPipeline` in HDR with image
  processing as a post-process (`applyByPostProcess`, contrast 1.05, no
  bloom or tone mapping), so these shaders write linear colour above 1
  that the post-process takes to display space. washoi view does image
  processing inside each material, in display space, as section 21.10's
  HDR target is not built yet. The explosions match because their shader
  converts to linear in that mode.
- **Not yet.** The flash anchor (flashes riding the barrel),
  `HitscanEffects` (which weapon fires what), atmospheric fog, rail and
  lightning beams.
- **Next.** The HDR target with image processing as a post-process (21.10,
  section 8's frame graph), so effects and materials blend in linear
  colour as the browser's do. Then rail and lightning beams, then dynamic
  lights.

### Milestone 6, twelfth slice: explosions (November 1, 2026)

- **shagger `fbe4e72`.**
  - `weapon::explosion` ports `ExplosionBatchRenderer` and
    `ExplosionBatchMaterial`: every live explosion's fireball, rays and
    sparks as three instanced meshes, animated in the vertex shader from
    their spawn time (`explosion_batch.wgsl`). The randomness comes from
    the caller in `Math.random`'s draw order. `weapon::explosion_effect`
    ports the four presets and which projectile takes which
    (`spawnImpact`).
  - `babylon::meshes::ico_sphere_builder` ports `CreateIcoSphereVertexData`
    (positions and indices).
  - Blended draws no longer write depth. Babylon's transparent pass sets
    each mesh's alpha mode, and `setAlphaMode` turns the depth mask off
    for anything but opaque, whatever the material says. The explosions
    showed it: with depth writes, a fireball hid its own sparks and its
    back faces. (The eleventh slice had turned depth writes on for blended
    `StandardMaterial`s; this takes that back.)
- **wash-fps-rust.** `washoi view --explosion ID,X,Y,Z,AGE` holds a
  projectile's explosion against a floor, `AGE` seconds in, its randomness
  from mulberry32 seed 1.
- **Checked against the browser** with `tools/parity` (`EXPLOSIONS=`),
  which swaps the same mulberry32 in for `Math.random` while SHAG's
  `ExplosionEffect` spawns each explosion, holds its clock, and gives back
  the flash light. Rocket and plasma explosions differ by 5 levels in 255,
  grenade and gunblade by 4. Each ray and spark is where the browser puts
  it. The trails view moved from 5 to 6 levels to 6 to 7 without depth
  writes.
- **Order.** Explosions draw after the world's blended surfaces and before
  the blended models. Their alpha indices (44 to 46) place them among the
  BSP's own blended surfaces in SHAG, which this does not do yet.
- **Not yet.** The flash light (dynamic lights), the camera shake,
  atmospheric fog, the impact sparks and models of projectiles that do not
  explode, spawning from impacts and server events.
- **Next.** Beams, tracers and muzzle flashes, then dynamic lights (21.10).

### Milestone 6, eleventh slice: projectile trails (November 1, 2026)

- **shagger `1bd99e2`.**
  - `weapon::projectile_manager` ports `updateTrail` (the 60 points, the
    pre-fill, the camera-facing ribbon and its crossing blade with the
    distance taper), `clampTrailLength` and `trailMaterialFor`, as
    `ProjectileTrail`. `weapon::effect_geometry` ports
    `updateUnlitTrailRibbon`.
  - `babylon::meshes::ribbon_builder` ports `CreateRibbonVertexData` for
    two open paths, double sided.
  - The model renderer rewrites a primitive's vertices in place each frame
    and recentres its bounds. (This commit also had blended
    `StandardMaterial`s write depth; the twelfth slice takes that back.)
- **wash-fps-rust.** `washoi view --projectile` draws each trail as a
  straight second of flight at 60 frames a second would leave it, rebuilt
  each frame for the camera.
- **Checked against the browser** with `tools/parity` (`PROJECTILES=`),
  which gives SHAG's held projectiles the same trail points and runs its
  `updateTrail` each frame. Rocket, grenade, plasma (clamped to 120 units)
  and gunblade trails match; views differ by 5 to 6 levels in 255.
- **Known difference.** With items posed at 0 seconds, a pickup's
  translucent shell behind the grenade draws stronger than in the browser.
  At 1.7 seconds it matches.
- **Not yet.** Flight, impacts and explosions, the carried light, the fade
  in, projectiles from snapshots.
- **Next.** Explosions, then beams, tracers and muzzle flashes, then
  dynamic lights (21.10).

### Milestone 6, tenth slice: projectile models (November 1, 2026)

- **shagger `435ce6c`.**
  - `loader::model::md3_loader` ports `MD3Loader.ts`: the parser, the
    Babylon-axis surface meshes, and `createMaterialFromShader` as
    `StandardMaterial` parameters with the stage animation
    `ShaderAnimationSystem` runs on them (`tcMod`s, `rgbGen` and `alphaGen`
    waves). Not yet: alpha test, glow textures, `animMap`, distance ramps,
    soft particles, autosprite.
  - `weapon::projectile_manager` ports the drawing of one projectile: the
    model and its materials (the gunblade blast's teal glow included) and
    `faceAlong` with the MD3 turn. `shared::entities::projectile_def`
    ports `ProjectileDef`.
  - `StandardMaterial` gains its alpha modes (combine, add, one-one),
    emissive as illumination, and a per-draw texture matrix, texture level
    and diffuse colour.
- **wash-fps-rust.**
  - `shared::entities::definitions::projectiles` ports the four
    projectile definitions.
  - `washoi view --projectile ID,X,Y,Z,YAW[,PITCH]` holds a rocket,
    grenade, plasma bolt or gunblade blast on the map, its shader stages
    `--time` seconds in.
- **Checked against the browser** with `tools/parity`
  (`PROJECTILES=`), which spawns the same projectiles through SHAG's
  `ProjectileManager` with their lights off and their stages posed at the
  same time. All four stand, face and glow as in the browser; views differ
  by 5 to 6 levels in 255.
- **Not yet.** Flight, trails, impacts and explosions, the carried light
  (with dynamic lights), the fade in, projectiles from snapshots.
- **Next.** Trails and the effects of 21.10 (beams, tracers, muzzle
  flashes, impacts and explosions), then dynamic lights.

### Milestone 6, ninth slice: procedural map items (November 1, 2026)

SHAG draws health, armour, ammo and the quad as generated sculptures
(wash-fps `world/pickups/procedural`), not MD3 models, so this slice ports
those; MD3 is left for projectiles.

- **shagger `156f988`.**
  - `babylon::meshes` ports `VertexData` (normals, transform, bake, merge,
    flat shading) and `MeshBuilder`'s box, sphere, torus, plane and
    cylinder.
  - `babylon::materials::standard_material` and `standard.wgsl` port the
    `StandardMaterial` terms the pickups use: hemispheric and directional
    diffuse and specular, emissive colour and texture, emissive and
    opacity Fresnel, diffuse alpha, RGB opacity, ambient and contrast.
  - `rendering::model_renderer` draws standard materials, with a per-draw
    emissive (the pulse) and texture offset (the ring scroll), per-material
    depth write, and the separate culling pass (back faces, then front).
    Scene lights carry their specular colours.
- **wash-fps-rust.**
  - `world::pickups::procedural` ports the recipes, geometry, materials,
    asset cache and visual: hover, bob, spin, breathing, pulse, the shell's
    colour flow, ring pivots and scroll, and the high tiers' billboarded
    cross. `world::pickups::ammo_icon_texture` loads the crate icons.
  - `washoi view` draws every map item, posed `--time` seconds in.
- **Checked against the browser** with `tools/parity` (`ITEMS=<seconds>`
  poses SHAG's visuals at the same time). Every dm2 prototype has the
  browser's vertex and index counts. Mega and 25 health, an armour and an
  ammo crate stand, turn, pulse and shade as in the browser; views differ
  by 5 to 6 levels in 255, as without items.
- **Not yet.** Collect and respawn transients, the respawn assembly and
  availability from snapshots.
- **Next.** MD3 projectiles (21.10).

### Milestone 6, eighth slice: skinned players (November 1, 2026)

- **shagger `73c5ea4`.**
  - `loader::model::glb` decodes `KHR_draco_mesh_compression` primitives
    with `draco-core` (20.7). All of Vanguard's 6,749 vertices decode to the
    accessors' bounds, in 8 ms.
  - `loader::model::gltf_model` gains `from_glb_rigged`: the bind pose,
    each primitive's joints and weights, and the node hierarchy and skins
    (`Skeleton`) for posing at run time. `loader::model::gltf_animation`
    reads clips as Babylon's glTF loader does: 60 frames a second, slerped
    rotations, ends held.
  - `animation::player_model_manager` ports `PlayerModelManager.ts`'s
    loading, retargeting by bone name with position tracks dropped,
    `createInstance`, `playAnimationOn`, the stepped clock (`wrapFrame`)
    and `resetRootMotion`. `animation::player_model_assets` holds the
    Vanguard recipe. All 17 clips load in 12 ms.
  - `rendering::model_renderer` skins on the GPU from a per-frame joint
    palette (a storage buffer), takes a per-draw albedo tint, and draws
    Babylon's `OutlineRenderer` outline: pushed out along the normals in
    bind space, before the mesh without depth write, then into depth only.
  - `network::client::snapshot_buffer` has `InterpolatedEntity`.
- **wash-fps-rust.**
  - `client::remote_player_logic` ports `deriveAnimation`.
    `client::remote_player_renderer` ports the body's placement, the clip's
    speed ratio and the style. `client::player_model_visual_settings` ports
    the team colours and the outline width at the cvar defaults.
  - `washoi view --player X,Y,Z,YAW[,CLIP[,FRAME]]` stands players on the
    map in the FFA style.
- **Checked against the browser** with `tools/parity` (`PLAYERS=`), which
  stands the same players through SHAG's `PlayerModelManager`.
  - On dm2, an idle player and a running one at frame 10 stand, turn and
    pose exactly as in the browser, outlines included.
  - Whole views differ by 4 to 8 levels in 255, as without players. On
    the near body green and blue are within 3; red is 9 lower, the
    browser's specular light and normal map, which models do not draw yet.
- **Not yet.** The weapon in hand, one-shot clips, powerup glow, normal
  maps, and players from a live match or a demo (the snapshot buffer).
- **Next.** MD3 items and projectiles (21.10).

### Milestone 6, seventh slice: glTF models and the map's weapon pickups (November 1, 2026)

- **shagger `9f1a9b4`.**
  - `loader::model::glb` reads `.glb`. `loader::model::gltf_model` poses a
    model at its first animation frame (`settleConstantSourceAnimations`),
    skins it once on the CPU, and keeps its `tag_*` attachment points
    (`collectModelAttachmentPoints`), as SHAG's pickup cache hands models
    out: flattened, with the loader's `__root__` gone.
  - `loader::model::tags_loader` ports `TagsLoader.ts`.
    `weapon::weapon_barrels` ports the world half of `WeaponBarrels.ts`
    (`findBarrelMount`, `attachWorldBarrels`, the material preparation).
    `world::pickup` ports how `Pickup.loadModel` places a cloned model.
  - `rendering::model_renderer` draws models as Babylon's `PBRMaterial`
    draws them with no environment texture. The lights are
    `SkyLightSystem`'s BSP-mode set (`lighting::sky_light_system`): the fill,
    the sun and the ambient light. Diffuse is Lambert (Babylon's EON model
    at diffuse roughness 0). Contrast is 1.05.
  - Blended draws write no depth, because Babylon's `setAlphaMode` turns the
    depth mask off. They sort back to front by bounding centre, and ties
    keep Babylon's mesh creation order. That order decides how the plasma
    gun's glass, halo and plasma layers stack.
  - The world pass now splits into opaque and blended halves, so models
    draw between them.
- **wash-fps-rust.**
  - `world::fps_pickup` draws every map weapon pickup at its
    `landMapPickups` spot, with `mapWeaponYaw` (`world::entity_manager`),
    its definition's scale and its barrels.
  - `shared::entities::definitions::weapons::weapons_table` holds the
    weapon models.
  - `washoi view` draws them. Health, armour, ammo and powerups are
    procedural visuals in wash-fps and are not drawn yet.
- **Checked against the browser** with `tools/parity`, which now shows the
  map's weapon pickups too.
  - Placement, barrels and draw order match on dm2.
  - In close-ups of the rocket launcher and the plasma gun, the colour
    means of the models are within 2 levels in 255.
  - Whole views differ by 6 to 8 on average. The floor's gloss maps
    account for most of that.
  - Normal, metallic-roughness and specular light for models are not drawn
    yet.
- **Next.** Skinned glTF players (21.10), then MD3 items and projectiles.

### Milestone 6, sixth slice: map pickups and the parity harness (November 1, 2026)

- **Map pickups.** `shared::bsp::pickup_table` ports wash-fps's
  `PickupTable.ts`. `shared::items::item_throw` ports `ItemThrow.ts`: the
  item throw, the spawn toss and `landMapPickups`. `tests/pickups_golden.rs`
  (shagger `tools/golden/pickups.mjs`) matches all 187 pickups of the seven
  maps bit for bit: classname, kind, origin and landing spot.
- **Parity harness (8.3).** `tools/parity` drives SHAG's fps game headless
  in Playwright's Chromium (SwiftShader WebGL2). It plays a demo to load a
  map, pauses it, holds the camera at Quake 3 views with only the world and
  sky drawn, and compares each view with `washoi view`.
  - On dm2 and dm14 the geometry, the camera and the far box line up
    exactly. The mean difference is 4 to 8 levels in 255.
  - What is left is the browser's normal and gloss maps and its image
    processing (exposure and contrast), both still to come in 21.10.
- **Next.** glTF models in shagger, then the weapon pickups drawn where
  `landMapPickups` puts them, checked against the browser with this harness.

### Milestone 6, fifth slice: PVS and frustum culling (November 1, 2026)

- **shagger `cb67eb9`.**
  - `loader::bsp::mesh::pvs` ports `buildFaceClusters`, and
    `loader::bsp::pvs_culler` ports `PVSCuller`, with batch ids in place of
    Babylon meshes. It fails open as SHAG's does.
  - World batches carry their faces' clusters (`bspPvsClusters`) and
    Babylon-space bounds. Autosprite bounds are grown by how far they turn.
  - `rendering::world_culling` leaves out the batches the PVS hides, as
    `PVSCuller.update` does, and those outside the view frustum, as Babylon
    does for each mesh. `WorldRenderer::draw_visible` draws the rest.
    `r_bsp_cellsize` is 0 by default, so batches stay whole per texture as
    in SHAG.
- **wash-fps-rust.** `washoi view` culls each frame and prints what it left
  out. `--no-cull` draws every batch, for comparing.
- **Proof.**
  - `pvs.mjs` runs SHAG's own `buildFaceClusters` and `PVSCuller` on every
    map, with every face as a mesh, over 400 camera points in the world
    bounds. The points include stationary frames and a missing camera.
    `tests/pvs_golden.rs` matches the face clusters and all 2,800 frames:
    the cluster, the stats and which faces show.
  - The data never reaches one branch: no shipped face sits outside every
    cluster, so dropping the `-1` rule passes.
  - On screen: 14 views, two spawns on each of the seven maps, are pixel
    for pixel identical with and without culling, while it leaves out up to
    140 batches by PVS and 67 by frustum.
- **Not yet.** `SkyVisibility` (skipping the sky when no sky face is in a
  visible cluster), spatial cells (`r_bsp_cellsize`), and culling for
  models and movers once they draw.

### Milestone 6, fourth slice: the far box sky (November 1, 2026)

- **shagger `ae964c5`.**
  - `loader::texture::skybox_loader` ports `SkyboxLoader.loadSkybox` up to
    the cube texture. Each Quake face is found as `loadSkyboxFace` finds it,
    falling back to the unsuffixed image for Warsow's `skyparmssides`. It is
    decoded as SHAG decodes it: its own TGA reader, and KTX rows as stored,
    with no flip. Stubbed faces are scaled to the largest face, then turned
    into Babylon's six cube slots (`Q3_TO_BABYLON_FACE_MAP`).
  - `rendering::sky_renderer` uploads slot `i` to cube layer `i`, as Babylon
    uploads it to `TEXTURE_CUBE_MAP_POSITIVE_X + i`. It samples the cube
    with the Babylon-space view direction, from one screen-covering
    triangle drawn first without depth. `apply_skybox` picks the far box as
    `BspMeshBuilder.applySkybox` does, and `WorldGeometry` collects the sky
    shaders.
  - The KTX skies of dm2 and dm5 (`env/36sky`) are one-face ETC1 images,
    not cube files, so the six-face path covers all seven maps.
- **wash-fps-rust.** `washoi view` draws the sky and takes `--pitch DEG`
  (negative looks up).
- **Proof.**
  - `skybox.mjs` runs SHAG's own loader on a Babylon `NullEngine`, which
    captures the six faces handed to `RawCubeTexture`. WebP, JPEG and PNG,
    which SHAG leaves to the browser, are decoded with sharp (libwebp).
  - `tests/skybox_golden.rs` matches all 40 face transforms and 6 stub
    scales. It also matches every source face (path, size, pixels) and all
    six cube slots of each of the seven maps, by SHA-256. The `image`
    crate's WebP decode matches libwebp on every face.
  - On screen: dm7's autumn canopy, dm11's overcast sky and dm14's blue sky
    show through the openings, with no seams.
- **Not yet.** Sky cloud layers, the sky sphere and `qer_editorimage`
  fallbacks, and portal skies. No shipped map reaches them.

### Milestone 6, third slice: shader animation and autosprites (November 1, 2026)

- **shagger `9dfa321`.**
  - `loader::shader::q3_stage_evaluation` ports `Q3StageEvaluation.ts`:
    the six waveforms and `tcMod` stepping (`scroll`, `scale`, `rotate`,
    `stretch`, `turb`, `transform`), with V8's `sin` and `cos`.
  - `babylon::materials::texture` ports `Texture.getTextureMatrix` for the
    offset, scale and `wAng` the animation sets (Apache-2.0, in `NOTICE`).
  - `loader::bsp::mesh::autosprite_records` and `autosprite2` port the
    billboard records and the vertex attributes `Q3AutospritePlugin` reads.
  - The world renderer steps each animated pass as `ShaderAnimationSystem`
    does: tcMods into the texture matrix, `rgbGen` and `alphaGen` waves
    scaling colour and alpha, `animMap` frames swapped by bind group. Every
    pass also takes `rgbGen const`, `alphaGen const` and the distance ramps
    (`Q3StageColorPlugin`, `Q3DistanceRampPlugin`). Autosprite faces get
    batches of their own after the rest, as `buildAll` makes their meshes,
    and the vertex shader turns them with the plugin's math.
- **wash-fps-rust.** `washoi view` takes `--time T` (seconds of animation
  before a screenshot) and `--at X,Y,Z,YAW` (a Q3 viewpoint instead of a
  spawn).
- **Proof.**
  - `stage_evaluation.mjs` steps every animated stage of the 342 shipped
    shaders (41 shaders) for 600 frames of uneven length with SHAG's code;
    `tests/stage_evaluation_golden.rs` matches bit for bit. Swapping V8's
    `sin` for the platform's fails it.
  - `autosprites.mjs` builds the record and attributes of all 419
    autosprite faces on the seven maps; `tests/autosprites_golden.rs`
    matches bit for bit.
  - On screen: washoi_dm14's billboard runs its `animMap` and sawtooth
    `rgbGen` (black at load, as in SHAG, lit by five seconds). A snd1 lamp
    corona covers the same screen area seen from three sides, so it turns.
- **Not yet.** Environment and `$cubemap` reflections and normal, gloss and
  glow maps (after milestone 7, 21.10); blended passes sorted back to
  front; then the farbox sky and PVS and frustum culling.

### Milestone 6, second slice: shader-defined surfaces (November 1, 2026)

- **shagger `e99ed34`.** `loader::shader::q3_shader_parser` ports
  `Q3ShaderParser.ts` with its JavaScript quirks kept: `parseFloat(x) || d`
  reads 0 as missing, `template` expansion follows `String.replace`
  replacement patterns, and templates carry from one script to the next.
- **shagger `f29da3a`.** `q3_shader_registry` ports `Q3ShaderRegistry.ts`:
  scripts load in owning-package order (`Pk3Loader::list_files` keeps the
  TypeScript's glob, where `.` matches any character, and its `Map` order),
  shaders are found with the underscore-to-slash fallbacks, and
  `resolveMaterialProps` and `resolveMaterialPasses` resolve materials,
  conditional branches and implicit `_norm`/`_gloss`/`_glow` materials.
  Stages carry an id, since the TypeScript compares stage objects.
- **shagger `da3a7ef`.** `loader::texture::ktx1_loader` ports `KTX1Loader.ts`
  and takes its software path always: ETC1 and DXT blocks decoded on the
  CPU, rows flipped, uncompressed alpha, luminance and RGB expanded. This
  reverses 20.6: 115 published textures (most of a map's concrete and metal)
  exist only as KTX.
- **shagger `1457782`.** The world draws with its materials.
  `build_world_geometry` asks for each texture's passes and leaves out
  faces whose shader is sky. `rendering::world_materials` ports
  `buildDiffuseCandidates`, the missing-image fallbacks (radial gradient for
  additive halos, `$whiteimage`, `$blackimage`) and `Q3Blend.ts` as wgpu
  blend, alpha-test and depth state. Opaque passes draw first, then blended
  ones; `cull none`, the decal polygon offset, full-bright stages without a
  lightmap, and vertex colour and alpha follow the props.
- **Proof.**
  - `shaders.mjs` parses every script in the packages (and four hand-written
    edge cases) with SHAG's parser; `tests/shaders_golden.rs` matches each
    as canonical JSON.
  - `shader_materials.mjs` mounts all 21 packages, loads the 342 shaders and
    resolves all 529 shader and map texture names under three option sets;
    `tests/shader_materials_golden.rs` matches every one. Mutating a
    cutoff or a pass rule fails it. The two branches no shipped data reaches
    (the inverse multiply blend, implicit `_add` glows) have unit tests.
  - `ktx.mjs` runs SHAG's loader over all 115 `.ktx` files against an engine
    with no compressed formats; `tests/ktx_golden.rs` matches the 112 it
    decodes by SHA-256 and the three six-face skies it refuses by message.
  - On the five maps rendered, one texture still has no image (a dm5 shader
    whose `material` names a file that is not shipped), against a third of
    each map before. Screenshots in the project files.
- **Audit.** `tools/shader_audit.mjs` counts what the world faces of the
  seven maps use (`docs/research/shader_audit.json`): 9,644 faces have a
  shader, 6,455 only an image. Blends are mostly alpha blend (2,282 faces)
  and filter (1,930); 1,288 faces use `rgbGen vertex` and 586
  `distanceRamp`; 196 are autosprites and 223 autosprite2; 70 use
  `tcGen environment`. `tcMod transform`, `tcGen vector` and
  `lightingDiffuse` are unused.
- **Not yet, in the order planned.**
  - Animation: `tcMod` (scroll, turb, rotate, stretch), `rgbGen wave` and
    `distanceRamp`, `animMap`.
  - Autosprites turned to the camera (drawn as their static faces now),
    environment and `$cubemap` reflections (a reflection pass is left out),
    normal, gloss and glow maps.
  - The sky, PVS and frustum culling, movers, and blended passes sorted back
    to front.
  - The visual parity harness (8.3).

### Milestone 6, first slice: the map world on screen (November 1, 2026)

- **shagger `acb039a`.** `loader::bsp::mesh` ports `coordinates.ts`,
  `vertices.ts`, `faceBuilders.ts` (polygons, meshes, and Bezier patches on
  one shared grid at level 8) and the byte side of `lightmaps.ts` (the IBSP
  transfer curve, raw FBSP pages, deluxe directions swizzled into Babylon
  space, `scaleBspVertexColors`).
- **shagger `c1eed0d`.**
  - `loader::pk3::pk3_loader` and `node_pk3` port `PK3Loader`'s file index
    and `NodePk3`'s disk ingest: lower-cased paths, packages in
    `localeCompare` order (ICU root order for ASCII names, checked against
    Node), later packages winning.
  - `loader::texture` ports `TextureExtensions.ts` and the candidate order of
    `resolveAndLoadTexture` (WebP, the name, TGA, PNG, JPG, then ETC1 KTX,
    which is not decoded natively).
  - `loader::bsp::bsp_lighting_profile` ports `BspLightingProfile.ts`.
  - `rendering` is the first wgpu code: a device with or without a window, a
    free camera in Babylon's conventions (left-handed, Y up, glam's
    `directx` projection for wgpu's 0-to-1 depth), the world grouped into
    batches the way `BspMeshBuilder.buildAll` groups them (texture,
    layered lightmap, patch), and one WGSL program with
    `BspBakedLightingPlugin`'s lightmap and deluxe terms. Textures are RGBA8
    without sRGB decode, as Babylon samples them. `offscreen` renders to an
    image for screenshots and tests.
- **wash-fps-rust.** `washoi view <map>` loads every package in a folder,
  builds the world from a spawn point, and either writes a PNG
  (`--screenshot`) or opens a winit window with a fly camera (mouse, W A S D,
  Space and C, Shift).
- **Proof.**
  - `render_maps.mjs` runs SHAG's own face builders over all 16,169 faces of
    the seven shipped maps, its lightmap encoding over every page through
    Babylon's NullEngine, and its lighting profile over each worldspawn.
    Rust matches all of it bit for bit (`tests/render_maps_golden.rs`); a
    one-ulp change to a patch weight fails it.
  - `tests/render_smoke.rs` renders washoi_dm2 from every spawn point and
    requires under 2 per cent of the frame to be empty. With the cull side
    flipped, 42 per cent is.
  - The build environment has no GPU, so it runs Mesa's lavapipe (Vulkan on
    the CPU). The window runs there under Xvfb at 31 to 36 fps for 59k to
    113k triangles at 1280x720.
- **Versions pinned (section 12).** wgpu 30 and winit 0.30, the pair egui
  0.37 builds on; glam 0.34; image 0.25; zip 9. winit is built without its
  drawn Wayland title bar, which pulls in the unmaintained `ttf-parser`.
- **Minimum Rust is now 1.88** in both repositories. zip 9 is the first
  release without RUSTSEC-2026-0367 to 0369 and needs 1.88. egui 0.37 will
  need 1.95 when the HUD starts.
- **Not yet, in the order planned.**
  - The Q3 shader parser and registry (21.7). Until then, about a third of
    each map's textures, the ones a shader script defines, draw white, and
    so do decals and glass. Sky faces are left out by the BSP flag only.
  - Sky farbox, PVS and frustum culling, alpha test, autosprites, movers.
  - The visual parity harness (8.3): the same view rendered by the browser
    client and by this renderer, compared.

### Milestone 2: against production (November 1, 2026)

- **shagger `bf7105a`.** The first `wss://` connection panicked: tokio-tungstenite
  enables no rustls crypto provider, and the local tests only used `ws://`.
  `network::client::tls` now builds one connector for the lobby and game
  connections: rustls with `ring`, the bundled Mozilla roots, and any PEM
  certificates in `SSL_CERT_FILE` (the counterpart of Node's
  `NODE_EXTRA_CA_CERTS`, for networks that re-sign TLS).
- **wash-fps-rust.** `washoi lobby <lobby> bot [gametype] [difficulty]`
  creates a private room, invites a lobby bot (`lobby:invite_bot`), readies
  once `lobby:bot_invited` arrives and plays the match `lobby:connect` names.
- **Proof, live on `wss://eu.washoi.net/lobby`.** One new account logs in, the
  key is kept in a key file, the welcome lists the four gametypes, a private
  duel is created, the bot Tamago (Hard) joins, the lobby starts the match on
  `washoi_dm5` and sends `lobby:connect` for `wss://eu.washoi.net:7777` with a
  ticket.
- **Left.** The game server leg on production: the build environment's
  egress relay carries port 443 but not the game ports 7777 to 7850, and
  SHAG's own Node client times out there the same way, so it waits for a run
  from a normal network. It works against local servers.

### Milestone 2: the headless client, against local servers (November 1, 2026)

- **shagger `91ed8d9`.** `network::client` ports `ClockSync.ts`,
  `ConnectionWatchdog.ts`, `SnapshotReceiver.ts` and the connection logic of
  `hosts/NetworkGameHost.ts`: the handshake (Join, Welcome with the 125 Hz
  check, JoinAccept, ResumeToken, the wait for the token on a ticketed join),
  snapshots dropped before the join and acked the moment they decode,
  baselines kept per connection (128), clock-sync bursts and resyncs, the
  10-second handshake timeout, the watchdog with its skipped gaps, commands
  and the reliable requests. It is written without I/O: the owner feeds it
  frames and the time. `network_session` is that owner, a tokio current-thread
  runtime on a network thread with `tokio-tungstenite` (rustls, `ring`,
  webpki roots, Nagle off), talking to the game through channels; on a drop
  or a stall it reconnects and sends `Resume` every 2 s until the server's
  grace has passed. Unit tests cover the handshake cases.
- **shagger `dc5f226`.** `lobby` ports `LobbyClient.ts` and the message types
  the client acts on (welcome, auth:error, matches:update, lobby:connect,
  chat, room state, lobby:error); every other message arrives as JSON
  (`serde_json`). Outgoing messages leave absent fields out, as
  `JSON.stringify` does.
- **wash-fps-rust.** `washoi connect <game server>` and
  `washoi lobby <lobby> list | spectate | queue <gametype>`, with
  `--key-file` to keep the account key and `--play` to join and run.
- **Proof, live against local servers.** wash-fps's `net:server`,
  `bot-server` and `lobby:server` run in the build environment from the SHAG
  checkout. The Rust client joins a deathmatch with three bots, acks at
  62.5 Hz with a ping of 0 to 1 ms (17 ms before Nagle was turned off), reads
  every Washoi event, keeps the roster the server leaves out, gets fragged,
  and resumes the same player after its connection is cut through a proxy.
  Through the lobby, two Rust clients log in (new account, then by key),
  queue for duel, get readied into one room, receive `lobby:connect` with
  their tickets and play the match the lobby spawned.
- **Licences.** `deny.toml` in both repositories now allows
  `CDLA-Permissive-2.0`, the licence of `webpki-roots` (the Mozilla CA list).
- **Left.** The live servers (waiting on the owner's go-ahead, since logging
  in creates an account on production), the stall-resume path against a
  real stall, and `match:seat_reissue` when a ticket expires.

### Milestone 1, done: demo files (November 1, 2026)

- **shagger `5cf322f`.** `shagger_shared::demo` ports `DemoReader.ts`,
  `DemoInputs.ts` (the reading side) and `DemoMapName.ts`. Gzip goes through
  `flate2` with its pure Rust backend, inflating concatenated members the way
  Node's `gunzipSync` does. Reads keep the TypeScript's tolerance: strings
  past the end are cut short, frames that fail to decode are skipped,
  inputs that fail leave the frame with none, and a frame's header is read
  before the demo header, so an out-of-bounds frame is `None` even when the
  header is broken.
- **Proof, synthetic.** `protocol.mjs` writes 400 demos of versions 0 to 7,
  raw and gzip (truncated, bad CRC, extra members, trailing bytes), with bad
  magic, visibility, account ids, index offsets and frame lengths, read by
  SHAG's own `DemoReader`. Rust matches header, index, all 829 frames by
  index, `readAllFrames` and `findFrameByTick`, including which reads fail.
  Mutations (BOM, visibility, safe-integer bound, gzip members) each fail.
- **Proof, real matches (wash-fps-rust).** `tests/golden/demos/` holds three
  12-second demos recorded with wash-fps's `demo:record` (bots on dm14 FFA,
  snd1 Search and Destroy, amphi1 Amphi; format version 6). Their 2,246
  frames, with about 27,000 Washoi events (match, weapon, combat and
  knockback state, fire, damage, beams, projectiles, pickups, drops, deaths,
  awards), decode exactly as SHAG reads them. The expected output is
  `demo_frames.json.gz` (14 MB of JSON).
- **Running the servers locally.** wash-fps's `demo:record` and `net:server`
  run from the SHAG checkout once the map packages are in
  `<shag>/public/data` (the files `tools/fetch_maps.sh` downloads, plus the
  manifest and index). The headless client is tested against that first.

### Milestone 3, done: movement on the shipped maps (November 1, 2026)

- **shagger.** `tools/golden/movement_run.mjs` is the stepping loop that
  `movement.mjs` used inline (sanitize, lifts, knockback drag, simulate, the
  canonical line); `movement.mjs` still writes the same file byte for byte.
  `movement_maps.mjs` runs it on the shipped BSPs: per map, 14 runs from
  spawn points facing their authored yaw, 6 dropped onto teleporters and
  jumppads, and 4 from random points in the world bounds, 600 steps each.
  It keeps only the per-step hash and names each map by file name and
  SHA-256. `generate.mjs` runs it when `WASHOI_MAPS` is set.
- **wash-fps-rust.** `tests/movement_harness` holds the replay shared by
  `movement_golden.rs` and the new `movement_maps_golden.rs`.
  `tools/fetch_maps.sh` follows 20.2 (manifest, index, package SHA-256) to
  download the map packages and extract the BSPs; the test skips without
  `WASHOI_MAPS` and fails on a map whose digest differs from the vectors. CI
  runs it in its own `shipped-maps` job, allowed to fail, since a map update
  on the servers breaks it until the vectors are regenerated.
- **Proof.** All 7 maps on eu.washoi.net (amphi1, dm2, dm5, dm7, dm11, dm14,
  snd1; 300 MB of BSP), 162 runs, 97,200 steps: Rust matches every step on
  the first run. The runs cover grounded movement, slides, wall charge and
  wall jump, ledge grabs, wading, knockback, 15 teleports and 631 jumppad
  steps. The shipped maps have no ladders and no water deep enough to swim,
  so those stay covered by the synthetic maps only. A mutation in view
  smoothing fails both tests.
- **This is 19.7's `sim:dump`** in the form the golden tools already had,
  without a new SHAG CLI. Inputs captured from real play are still open; they
  come with `InputHistory` in milestone 4.
- **Network access.** The build environment now reaches washoi.net and
  crates.io. The live manifest matches 20.2: 21 packages, index version 2,
  maps owned by `map_washoi_<map>.pk3`.
- **Next.** Milestone 2, the headless client (lobby auth, `lobby:connect`,
  the game handshake, acks, clock sync), then `V2NetworkClient` and
  reconciliation (milestone 4). Demo files (`flate2`) can now be done too.

### Milestone 3, second part: the movement profile (November 1, 2026)

- **shagger `9b95e87`.** `shagger_shared::fdlibm` ports V8's `sin` and `cos`
  (fdlibm, Sun's notice kept), exposed as `js::js_sin` and `js::js_cos`.
  `f64::sin` differs from V8 for about 3% of inputs. Proof: 18,000 vectors
  from V8, including huge arguments that take `kernel_rem_pio2`.
- **shagger `de53d94`.** `bsp::trigger_table`, `server::simulation::
  input_sanitizer`, the network `input_component` buttons and the
  `movement_profile` contract, plus `js_parse_int10` and
  `js_number_to_string`. `tools/golden/maps.mjs` is the map builder shared by
  the collision and movement generators.
- **wash-fps-rust.** `liquid` and `simulation` port wash-fps's
  `FpsMovementProfile` with acceleration, crouch and prone, slide, wall
  charge and wall jump, ledge grab, swim and water jump, ladders, knockback,
  teleporters and jumppads, on top of shagger's collision world.
- **Proof.** `tools/golden/movement.mjs` runs wash-fps's own profile (SHAG's
  pinned wash-fps commit, at `shag/src/games/fps`) on 40 synthetic maps with
  walls, ledges, pools, ladders, teleporters and jumppads, 4 runs of 180 steps
  each, stepping lifts and knockback drag the way `V2NetworkClient` does.
  Each step records position and velocity bits plus a hash over every result
  field and the full saved state. Rust matches all 28,800 steps. Mutation
  checks (swim with `f64::sin`, a rounded wall-jump rise, crouch timing,
  prone, water jump, ledge abort, jumppad) each fail the test.
- **Next.** The `sim:dump` harness against real maps, then
  `V2NetworkClient`, `InputHistory` and reconciliation (milestone 4).

### Milestone 3, first part: map loading and collision (November 1, 2026)

- **shagger `a33f0a8`.** `loader::bsp::bsp_loader` ports `BspLoader.ts`
  (`load`, `loadCollision`, entity parsing, leaf lookup, cluster visibility,
  used textures) for IBSP and FBSP. `shagger_shared::js` gained `js_trim`,
  `js_parse_float` and `js_string_to_number`. Proof: 250 synthetic maps with
  real-file damage (lumps outside the file, partial records, bad patch
  ranges) from `tools/golden/bsp.mjs` load exactly as SHAG loads them.
- **shagger `42a23dc`.** Map collision. `shagger_shared` gained
  `bsp::{map_entities, brush_model_state, lift_system}` and
  `collision::{collision_world, shared_collision_world}`; `shagger::physics`
  ports `SweepCollisionBackend.ts` (BSP walk, brushes, patch facets, clip
  brush fallback, containment pruning, lifts, `func_explosive` models, slide
  and step-slide moves, point contents, ladders, ground) and
  `SweepBackendAdapter.ts`. The Babylon `Vector3` subset it needs is in
  `babylon/maths/math_vector.rs` (Apache-2.0).
- **Proof.** `tools/golden/collision.mjs` builds 120 IBSP maps (floor, stairs,
  ramps, nested and slanted brushes, every collision texture kind, curved
  patches, random BSP trees, lifts with triggers, explosives) and runs about
  8,400 mixed queries through SHAG's own backend, adapter and
  `SharedCollisionWorld`, in order, so lift motion and model toggles feed later
  traces. Rust replays them and matches every bit. Mutation checks (overclip,
  leaf lookup, start-solid visit reset, side surface flags) each fail the test.
  The generator now needs SHAG's `node_modules` for `@babylonjs/core`.
- **Next.** The movement profile in `washoi::simulation` (acceleration,
  slide, wall jump, ledge grab, swim, ladder, knockback), then the `sim:dump`
  harness against the TypeScript simulation.

### Milestone 1: the wire protocol (October 31, 2026)

- **shagger `7d19ed0`, `ee860b4`.** `shagger_shared::net` ports
  `BinaryProtocol.ts` (encoder, decoder, commands, snapshot decode with
  baselines and the pickup mask) and `MessageCodec.ts` (snapshot envelope and
  header checks, every client and server message, `peekCommandSeq`). The
  snapshot decoder takes an `EventCodec`; `NoEvents` is the engine's own.
  `shagger_shared::json` parses with `JSON.parse` rules (duplicate keys, key
  order, lone surrogates), and `shagger_shared::js` gained the `DataView`
  integer conversions (`to_uint32` and friends).
- **Proof.** `tools/golden/protocol.mjs` runs SHAG's own TypeScript and writes
  byte and decoded-state vectors: 309 hand-written raw snapshots covering every
  entity and player flag, repeated ids, rotation without position, truncation
  and trailing bytes; 130 server-encoded delta chains through the envelope,
  including every header SHAG refuses; 300 commands with odd JavaScript numbers;
  all messages; 425 `TextDecoder` cases (BOM, invalid UTF-8). Rust matches all
  of them bit for bit on the first run after the port.
- **wash-fps-rust.** `washoi::shared::net::snapshot_events` is Washoi's
  `EventCodec`: all 25 codes, with `match_state`, `weapon_state` and
  `agent_directives` payloads kept as `json::Value`. 1,500 golden snapshots
  of random events (odd JSON, malformed JSON, truncation) decode exactly as SHAG
  decodes them.
- **Left in milestone 1.** Demo files: `DemoReader.ts` needs zlib, and
  crates.io is not reachable from the build environment yet, so this waits for
  `flate2`. The server-side snapshot encoder is not ported; the client never
  encodes a snapshot.
