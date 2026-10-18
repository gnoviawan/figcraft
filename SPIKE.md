# SPIKE: web host API (E-0) on the `termul` fork branch

Termul embeds `figcraft-web` in an iframe and drives it over `postMessage`. This spike lands the host
API (patch area 1) and measures the E-0 go/no-go thresholds from the Termul canvas spine.

- **Base.** Branch `termul` sits on upstream `storytold/figcraft` main at `bb8b809` (2026-10-17, 763
  commits past `v0.6.0`). The brief named tag `v0.6.0`. A later directive moved the base to `bb8b809`.
  Nothing had been committed on `v0.6.0`, so no rebase was needed.
- **Measured on.** Windows 10 Pro 19045, Microsoft Edge 155.0.4283.45. That is the Chromium engine
  WebView2 uses, but the runs used the Edge browser, not a WebView2 host. Measured 2026-10-17.

## What changed in upstream main that affected the plan

- `b8f97a5` "M8: the control channel answers while the window is minimized" is desktop only.
  `drain_hidden` runs from `FigApp::logic` when a native window is hidden. The web host API does not
  queue `engine.*` / `document.*` / `node.*` at all: it answers them synchronously in the message
  handler. The main idea carried over is `NEEDS_FRAME`. A frame-only method fails at once instead of
  waiting, and here `ui.*` returns `CANVAS_HIDDEN`.
- Main's `ui.screenshot` without `path` returns base64 `data` (`shot_reply`). That makes it usable
  from the browser once a frame runs.
- Main added `apps/figcraft-web/src/query.rs` (`#join=`), browser downloads and clipboard. The host
  API sits next to them and does not touch them.

## Build

```sh
# from apps/figcraft-web; trunk 0.21.14, wasm-bindgen 0.2.129 and wasm-opt (binaryen 123) on PATH
trunk build --release        # output: dist/web (index.html, figcraft-web-<hash>.js, figcraft-web-<hash>_bg.wasm)
packaging/web/package.sh --skip-build   # 24 MiB wasm gate, then the zip
```

This machine has no MSVC C++ build tools, so native builds used the `x86_64-pc-windows-gnu` 1.95.0
toolchain (`RUSTUP_TOOLCHAIN=1.95.0-x86_64-pc-windows-gnu`) with a winlibs mingw-w64 GCC for `ring`.
The E-0 measurements below used a bundle built without `craft-fonts`, and that bundle has no fonts at
all (see "Fonts (F7)"). Release bundles need `CRAFT_FONTS_DIR` and `CRAFT_FONTS_REQUIRED=1`.
`package.sh --skip-build` passed its size gate on that bundle and then stopped, because this machine
has no `zip`.

The harness under `apps/figcraft-web/spike/` is handwritten HTML and JavaScript, which the repo's
"Rust only" rule does not cover. It is spike tooling, not shipped code.

## Host API (what landed)

Files: `apps/figcraft-web/src/host.rs` (plain Rust, unit-tested on the host), `bridge.rs` (the
`postMessage` transport) and `web.rs` (wiring).

- **Opt-in.** The API exists only when the page is framed and the URL carries `?ho=<host origin>`.
  It also reads `&ms=<mountSeq>`. Messages are accepted only when `event.origin == ho` and
  `event.source == window.parent`. Replies and events are posted to `ho` only. `*` and `null` are
  refused.
- **Envelope.** `{id, method, params, mountSeq}` → `{id, ok, result | error: {code, message},
  mountSeq}`. `params` and `result` are JSON text. `id` and `mountSeq` are echoed untouched. Events
  are `{method: "event.*", params, mountSeq}`, and their `mountSeq` comes from `?ms=`.
- **Synchronous.** `engine.execute {command, params}`, `engine.commands`, `document.inspect` and
  `node.get` go through upstream's `control::handle`. They run in the message handler, then request a
  repaint.
- **Host-only.**
  - `host.openDocument {bytes}` → `{contentVersion}`. It replaces the open document.
  - `host.snapshot` → `{contentVersion, bytes}`. The bytes are the `.figcraft` file with versions and
    branches.
  - `host.markSaved {contentVersion}` clears dirty only when the version is still current. It returns
    `{dirty, contentVersion}`.
  - `host.renderPng {ids?, scale?}` and `host.exportBytes {format, ids?, scale?}` return a base64
    string. Without `ids` they export the current page's top-level layers.
  - `host.theme {tokens}` and `host.locale {bcp47}` restyle and relabel the editor at once. The
    payloads are in "Theme, locale and brand (E1-3)" below.
  - `host.describe` → `{tag, engineVersion}`. `tag` comes from the build-time `FIGCRAFT_HOST_TAG`
    and defaults to `dev`.
- **`ui.*`.** These are queued on upstream's control queue, and the next frame's drain answers them.
  They return `CANVAS_HIDDEN` while `document.hidden` is true or the canvas is off screen
  (IntersectionObserver).
- **Errors.** `window.*`, `app.quit`, other methods and missing methods return `UNKNOWN_METHOD`.
  Engine failures and bad params return `ENGINE_ERROR`.
- **Events.**
  - `event.ready` fires after the second frame. The first frame only installs fonts.
  - `event.dirtyChanged {dirty, contentVersion}` fires when that pair changes, and never before
    ready. It is checked at the end of every bridge call, which is the export path for agent edits.
    It is also checked after each frame, because user edits only happen inside frames.
  - `event.crashed {reason}` comes from a panic hook chained after eframe's.
- **`contentVersion`.** It counts changes of the active document's content. It is the identity of
  the document `Arc` (`DocState::document_itself`) plus the document's uid. The tracker holds a
  strong reference, so copy-on-write always gives an edited document a new pointer. Selection,
  viewport, and version or text previews do not bump it. A gesture in progress counts once, when
  committed.
- **No engine change was needed.** No change was made in `crates/`.

Tests: `cargo test -p figcraft-web` has 10 host-API tests, covering ids above 2^53 surviving as
text, contentVersion semantics, stale `markSaved`, a snapshot round trip, render and export, typed
errors and ready gating.

## Theme, locale and brand (E1-3)

Patch areas 3 and 4. The host sends both calls after every mount, and again whenever its own theme
or language changes. The editor keeps them for the life of the mount and applies them on every
frame. A remount starts with figcraft's own theme and the browser's language until the host sends
them again.

### `host.theme`

```json
{"tokens": {
  "mode": "dark",
  "background": "#08090a",
  "card": "#0f1011",
  "foreground": "#e5e5e6",
  "mutedForeground": "#8a8f98",
  "border": "#23252a",
  "primary": "#3b82f6",
  "primaryFill": "#3b82f6",
  "destructive": "#ef4444"
}}
```

- Every key is required. Colours are `#rrggbb` strings, which is what Termul's
  `readCssTokenHex('--<token>')` returns. `mode` is `"light"` or `"dark"`.
- The keys are Termul's semantic tokens in camelCase: `--background`, `--card`, `--foreground`,
  `--muted-foreground`, `--border`, `--primary` (accent text), `--primary-fill` (solid accent),
  `--destructive`.
- `{"tokens": null}` hands the colours back to the editor's own theme preference.
- The reply is `{dark}`. A missing key, a bad colour or a bad `mode` is `ENGINE_ERROR`, and the
  colours already applied stay.
- The mapping is `Tokens::from_host` in `crates/ui-egui/src/theme.rs`:
  - `background` paints the canvas and the tab strip.
  - `card` paints the panels, dialogs and menus.
  - `foreground` is used for text, icons and the focus ring. `mutedForeground` is used for secondary text.
  - `primaryFill` is the accent, used for selection and handles. `primary` is used for accent text.
  - `destructive` is used for danger.
  - Hover, pressed, field and selected-row shades are mixed from those colours.
  - Component purple, success, warning, guides and shadows come from figcraft's own light or dark
    theme, picked by `mode`.
- The colours apply in the call itself, without a reload or a frame. While they are set, figcraft's
  own light/dark switches are hidden: the tab-bar toggle and the Preferences Theme row. The check
  is `FigApp::theme_toggle_shown()`. The kiosk branch can add `&& !kiosk` there.

### `host.locale`

- The request is `{"bcp47": "de-DE"}` and the reply is `{"language": "de"}`. The reply names the
  catalog the tag resolved to.
- The tag resolves the same way the language preference does: the exact tag, else its base
  language, else a regional catalog. A tag with no catalog shows English.
- The call sets the `pref.language` preference and switches the catalog at once.
- A missing or malformed tag is `ENGINE_ERROR`.

### Brand

- The build-time env is `FIGCRAFT_BRAND_NAME`. Leave it unset for upstream's "FigCraft".
- Termul builds the bundle with `FIGCRAFT_BRAND_NAME="Termul Canvas" trunk build --release`.
- `crates/ui-egui/build.rs` replaces "FigCraft" in every catalog's text when it bakes the catalogs.
  The keys, which are the English text in the code, stay as they are.
  `crates/ui-egui/src/brand.rs` holds the name for the strings outside the catalogs: the About
  title, the home sidebar, window titles, the engine's command labels and the web loading text.
- The static page, which shows before the wasm starts, takes the name too. A trunk post_build hook
  (`packaging/web/brand-index.sh`) writes it into `index.html`'s `<title>` and "Loading ..." text,
  so `trunk build` and `packaging/web/package.sh` both do it. The hook needs `sh` and `awk` on
  PATH. `.github/workflows/termul-release.yml` fails the build when the name is missing from the
  wasm or from the page.
- A rebranded About shows the name and version, then only the plain text "Based on FigCraft by the
  ArtCraft team.". It has no ArtCraft name, link, Discord link or logo tile.
- Left as is:
  - The `.figcraft` file extension.
  - Engine and format error messages, such as "not a valid FigCraft file".
  - The Help menu's "Help page" link to getartcraft.com. A kiosk or rebranded build drops that row and "Getting started" (`toolbar::help_rows`) and keeps "Keyboard shortcuts" and About.
  - FigCraft's own chameleon-tail glyph (`Icon::Logo`, original MIT artwork, not an ArtCraft mark)
    on the rail's main-menu button and the document tabs. The kiosk build hides both.
  - A kiosk also hides the Share button and the people bar (avatars, spotlight menu) at the top right of the right panel. Present stays, in the corner Share left.

### Verified

`spike/brand-theme.mjs` drives a bundle built with `FIGCRAFT_BRAND_NAME="Termul Canvas"` and saves
four screenshots: `brand-dark.png`, `brand-light.png`, `brand-about.png` and `brand-about-de.png`.
Termul dark and then Termul light are applied to the same mount, with no reload. About shows only
the name, the version and the attribution. After `host.locale {bcp47: "de-DE"}` the UI is German
and About reads "Über Termul Canvas".

Not themed by the tokens:

- The canvas page colour, which is document data.
- Canvas overlays, which use the `ACCENT` constant rather than `Tokens`.

In the built wasm, every "ArtCraft" is in one of these places:

- The two About strings a rebranded build does not show, in each language.
- The attribution.

Every "FigCraft" is in one of these places:

- Catalog keys, each followed by its renamed value.
- Engine, format and plugin error messages.
- The `brand::UPSTREAM` constant.

The test `brand::tests::a_rebranded_build_shows_no_upstream_name_or_artcraft_mark` checks every
catalog value in every language.

## Guest presence (patch area 5)

Termul shows each agent working on the canvas as a guest: a pointer with a name tag in the
agent's colour, and a ring on the nodes it writes or reads. The host decides where a guest goes.
The editor only draws it. Guests live in the window: they are never synced or saved, never touch
the document, `contentVersion`, the dirty flag or undo, and never move the view.

### Methods

All three are host-only, return `{}`, and work in a kiosk and while the canvas is hidden. A hidden
canvas keeps the state and draws it once frames run again.

- `host.guestCursor {id, name, color, x, y, page?}`.
  - `id` and `name` are text. The name tag shows at most 64 characters.
  - `color` is `#rrggbb`.
  - `x` and `y` are page coordinates, the same space as `absoluteBounds`.
  - `page` is an optional page node id (text, like every node id). With it, the guest shows only
    while that page is in front. Without it, the guest shows on any page.
  - The first call creates the guest at its target with a 200 ms fade-in. Later calls glide it
    there (exponential approach at 2.5/s). Reduced motion jumps instead.
- `host.guestFlash {id, ids, kind}`.
  - `ids` is an array of node ids. `kind` is `"write"` or `"read"`.
  - A write ring grows in over 200 ms, holds 400 ms and releases over 300 ms. A read ring holds
    1.5 s with a scan band sweeping down the node. Reduced motion keeps only the fades.
  - Rings show only for nodes on the page in front. An unknown node is skipped.
  - A flash also counts as an update for the idle timer.
- `host.guestHide {id}` fades the guest out over 1.5 s and drops it.

An unknown guest `id` on `guestFlash` or `guestHide` is a no-op that still returns `{}`. A missing or
malformed parameter is `ENGINE_ERROR`.

### Lifecycle

- A guest with no update for 5 s fades out over 1.5 s on its own, so a host that dies leaves no
  pointer. The clock keeps running while no frames are drawn.
- An update during a fade brings the guest back from the opacity it had, without a pop.
- A pointer at rest drifts about 4 points. A gliding one does not.
- A pointer outside the view shows as a small arrow on the canvas edge pointing toward it, with
  the name.
- Guests are keyed by `id` and independent. Each keeps at most 100 rings.
- Frames are asked for while a guest drifts, glides, fades or rings. Under reduced motion a still
  guest asks for one frame when its fade is due.

### Where it lives

- `crates/ui-egui/src/guests.rs` holds the model and the drawing. Tests are in `guests_tests.rs`.
- The drawing reuses `live::pointer`, `live::name_tag` and `canvas::node_quad`. It runs right after
  `live::draw`, outside its "no peers" early return.
- `UiState::guests` holds the state. `ui.inspect` reports it as `guests`.
- `apps/figcraft-web/src/host.rs` parses the three methods. Tests are in `host_tests.rs`.
- `spike/guests.mjs` drives the methods in a kiosk mount and saves `guests-*.png`.

### Verified

`spike/guests.mjs` ran against a `FIGCRAFT_BRAND_NAME="Termul Canvas"` release bundle in headless
Edge, with all 12 checks passing. One agent visits three rectangles, rings one as a write and one
as a read, and a second agent joins. One target is off screen, then the first agent hides and the
second fades out on its own. `contentVersion` stayed the same and no `event.dirtyChanged` fired.
The screenshots are `guests-1-write.png` to `guests-7-hidden.png`, and `guests.webm` records the
whole run. Playwright's video needs its ffmpeg; `PLAYWRIGHT_BROWSERS_PATH=D:/devtools/ms-playwright`
has it on this machine. Without it the script runs and skips the video.

## Fonts (F7)

The first release bundle had no fonts: `text.fonts` returned `[]`, text layers were 1 px wide and
text exports were empty. The wasm build cannot use system fonts, and `crates/text/build.rs` embeds
fonts only from the optional `CRAFT_FONTS_DIR` build input (storytold/craft-fonts).

- **Which craft-fonts commit.** `4391305` (`43913056ce82a3fdad0ef69f6b88d1dc922a8104`), the head of
  craft-fonts' `pr/latin-presentation-fonts` branch. Upstream's own `release.yml` pins the same SHA.
  craft-fonts `main` (`a71c20e`) has no `fonts/latin-manifest.txt`, and the web build embeds only
  Inter (plus Noto Sans CJK with `FIGCRAFT_EMBED_CJK`), so a bundle built against `main` would again
  embed nothing. Re-pin to `main` once the Latin manifest lands there. If that branch is deleted
  before the SHA is reachable from `main`, `actions/checkout` can fail to fetch it. The workflow's
  fonts check and the `CRAFT_FONTS_REQUIRED=1` build error make that a loud failure, not a silent
  fontless release.
- **What the wasm embeds.** Inter Regular, Italic, SemiBold and Bold (`fonts/latin-manifest.txt`;
  1,669,200 B of font files in the checkout). Nothing else: the other
  families are skipped for wasm32, and CJK stays out unless `FIGCRAFT_EMBED_CJK` is set.
- **Local build.** `D:/git/craft-fonts` is a clone at that SHA (detached). Build with
  `CRAFT_FONTS_DIR=D:/git/craft-fonts CRAFT_FONTS_REQUIRED=1 FIGCRAFT_BRAND_NAME="Termul Canvas" FIGCRAFT_HOST_TAG=dev trunk build --release`.
  `.cargo/config.toml` already points at `../craft-fonts`, which is `D:/git/craft-fonts` for the
  checkout and for worktrees named `D:/git/figcraft-<unit>`. Without `CRAFT_FONTS_REQUIRED=1` a
  missing clone only warns and builds a fontless bundle, so always set it for a bundle you ship.
- **Release workflow.** `termul-release.yml` checks out craft-fonts at `CRAFT_FONTS_REF` into
  `craft-fonts/`, sets `CRAFT_FONTS_DIR` and `CRAFT_FONTS_REQUIRED=1`, and its zip check fails the
  job when the wasm lacks Inter's UTF-16 name-table bytes or the zip lacks `OFL-inter.txt`.
  `packaging/env.sh`'s `copy_font_licences` already writes `OFL-<family>.txt` for every family in
  the checkout into the zip, so the licence travels with the bundle.

### Size

| Bundle | wasm bytes | vs 25,165,824 B gate |
|---|---|---|
| No fonts (E-0, `bb8b809` plus host API) | 22,391,412 | 2,774,412 B under |
| Inter x4 embedded (this build) | 24,042,866 | 1,122,958 B (1.07 MiB) under |

The delta is 1,651,454 B for four Inter faces. The gate holds, so nothing was changed there. It
is a thin margin: each further Inter style costs about 0.4 MB, so the next two styles would use
more than half of it. Options if it tightens (not needed now): embed Regular and Bold only (about
0.8 MB less), subset the faces (no tool in craft-fonts for it), or fetch fonts as lazily loaded
assets (needs a fork change outside the four patch areas).

### Proof it renders

`spike/fonts-text.mjs` against that bundle in headless Edge (1600x900, WebGPU), all checks pass:
`text.fonts` is `["Inter"]`, `text.fonts {family: "Inter"}` lists Regular, Italic, SemiBold and
Bold, a `text.create` of "Termul Canvas text" at 64 px has width 583 and height 77 (G1 saw 1 px),
`host.renderPng` of it is an 8,117-byte PNG, and `spike/fonts-text.png` shows the text on the
canvas and in the Layers panel.

### Licences and attribution for Termul

- Inter is SIL OFL 1.1, "Copyright (c) 2016 The Inter Project Authors (https://github.com/rsms/inter)".
  The licence file declares no Reserved Font Name. The files are used unmodified (craft-fonts checks
  each SHA-256).
- The OFL allows embedding and redistributing the font inside software, including a commercial
  product, as long as the licence text and copyright notice travel with it and the font is not sold
  on its own. So redistribution inside Termul is allowed.
- **Action for Termul (not done here).** Ship `OFL-inter.txt` (it is in the release zip, and in
  craft-fonts at `fonts/inter/OFL.txt`) with the app's third-party notices, and add one line to its
  NOTICE or About credits: "Inter, Copyright (c) 2016 The Inter Project Authors, SIL Open Font
  License 1.1". Termul's own bundle only needs Inter. The zip also carries `OFL-*.txt` for the 20
  families it does not embed, which can be dropped from Termul's copy.
- Upstream's `NOTICE` and `ATTRIBUTION.md` already describe craft-fonts' fonts. The fork's brand
  knob leaves them alone.

## Measurements

Harness: `apps/figcraft-web/spike/`.

- `serve.mjs` serves the bundle on `127.0.0.1:8767` and the harness on `localhost:8768`, so there
  are two origins.
- `host.html` loads the bundle in an iframe and has buttons for execute, inspect, snapshot and the
  iframe modes. The canvas updates live.
- `measure.mjs` uses Playwright, headless Edge, a 1600×900 viewport and WebGPU.
- `hidden.mjs` launches Edge directly and drives it over raw CDP. Playwright emulates a visible page
  even when the window is minimised, so real `document.hidden` needs this script.

Raw results from the final runs are summarised below. Latencies are host-page round trips over
`postMessage`.

### Repaint after an external execute

- `engine.execute` round trip: p50 0.8 ms, p95 7.2 ms (n=20).
- A `ui.inspect` sent right after it is answered by the next frame. That frame drains control
  requests before it draws, so it is the frame that shows the change.
- `framesDrawn` advanced by exactly 1 in 74 of 80 samples over four runs, and by 2 in the rest
  (another repaint was already queued).
- Send → next frame: p50 25–35 ms. rAF ran at about 30 Hz in these runs.

### Bridge call latency by iframe state (30 calls each of `document.inspect` and `engine.execute`)

| State | `document.inspect` / `engine.execute` | `ui.inspect` |
|---|---|---|
| visible | p50 1.0 / 0.6 ms, max 3.1 / 1.1 ms | p50 27 ms (one frame) |
| offscreen 1×1 (`left:-10000px`) | p50 1.0 / 0.7 ms, max 1.3 / 0.9 ms; 300 more calls, none over 50 ms | `CANVAS_HIDDEN` at once. Before the IntersectionObserver fix: no frame ever, host timeout |
| in viewport 1×1, `opacity:0` | p50 1.0 / 0.6 ms, max 1.4 / 0.8 ms | about 1010 ms (Chromium draws about 1 frame/s) |
| `display:none` | p50 1.4 / 0.8 ms, **but stalls of 0.87–4.8 s** at the switch and intermittently (5 calls over 50 ms in 600) | `CANVAS_HIDDEN` at once |
| page hidden (Edge minimised, `document.hidden` true) | p50 0.5 / 0.4 ms, max 2.2 / 5.6 ms | `CANVAS_HIDDEN` at once |
| page hidden + offscreen 1×1 | p50 0.5 / 0.5 ms, max 14.4 / 1.1 ms | `CANVAS_HIDDEN` |
| page hidden + `display:none` | p50 0.4 / 0.3 ms, max 28.9 / 1.0 ms | `CANVAS_HIDDEN` |

### 500-node drag

- 500 overlapping rectangles, all selected, dragged with the real mouse (120 moves). The result is
  one `Move` undo step, and the first node went from (0, 0) to (257, −24).
- Time inside each `requestAnimationFrame` callback, which covers egui logic, the UI and the paint
  submission: p50 8.0 ms, p95 72 ms, max 76 ms (n=314).
- Frame interval: p50 33 ms, p95 72.5 ms.
- Building the document through the bridge took 5.2 s for 500 `engine.execute` calls, about 10 ms
  each including Playwright overhead.

### Bundle

- `figcraft-web-<hash>_bg.wasm` is 22,360,389 bytes (21.3 MiB). The JS glue is 167,591 bytes.
- This was built without `craft-fonts`. With Inter embedded the wasm is 24,042,866 bytes (see "Fonts (F7)").

### Out-of-tree `Backend` (A3)

`examples/backend-toy` is its own workspace, depending on `crates/mcp` by path. Its `Bridge`
implements `figcraft_mcp::Backend` by sending host-API envelopes, with `params`/`result` as JSON
text, through a transport closure. `cargo test --manifest-path examples/backend-toy/Cargo.toml`
passes 2 tests:

- Upstream's `call_tool` runs `create_node`, `inspect_document` and `export` through the bridge.
- A typed error reaches the `Err(String)` as the JSON text of `{code, message}`.

## Go / no-go

| Threshold (EPIC-SPLIT E-0) | Result | Verdict |
|---|---|---|
| External execute repaints within one frame | The next frame draws it (74/80 exactly one frame) | **GO** |
| Call < 500 ms while `display:none` (WebView2 engine) | p50 ~1 ms, but 0.87–4.8 s stalls | **NO-GO**. The fallback, an offscreen 1×1 mount, is already AD-19's rule and passes: max 1.3 ms, 0 of 360 calls over 50 ms |
| Call < 500 ms while the page is hidden (WebView2 engine) | Minimised Edge: max 28.9 ms | **GO** |
| Same two thresholds in WKWebView | Not measured on this machine | **OPEN (gating)** |
| Same in WebKitGTK | Not measured on this machine | Recorded, not gating |
| 500-node drag < 100 ms per frame | Frame work p95 72 ms, max 76 ms | **GO**, with a thin margin (headless, one machine) |
| Bundle < 24 MiB | 21.3 MiB without `craft-fonts`; 22.9 MiB (24,042,866 B) with Inter embedded | **GO**, 1.07 MiB headroom with fonts ("Fonts (F7)") |
| `Backend` implementable out of tree | Compiles, 2 tests pass | **GO** |

**Overall.** GO for E-1 on Chromium / WebView2, with the hidden mount as offscreen 1×1 (never
`display:none`). The E-0 gate is not fully closed: WKWebView was not measured, and the spine makes
it gating.

## Findings for the spine

1. **AD-19's premise is false in Chromium.** The premise is that an offscreen 1×1 visible iframe
   "keeps the egui frame loop running". A cross-origin iframe outside the viewport draws no frames.
   The host API does not need frames for `engine.*` / `document.*` / `node.*` / `host.*`, so agents
   still work. A hidden mount cannot answer `ui.*`, and it returns `CANVAS_HIDDEN` at once.
   Without the IntersectionObserver check, a `ui.*` call would time out. Three such timeouts would
   trip AD-31's crash detector and remount a healthy editor.
2. **Keep the hidden mount off `display:none`.** It stalls the iframe's message handling for
   seconds. AD-19 already forbids it.
3. **A mount started while the page is hidden never reaches `event.ready`.** It waits until the page
   is shown, because eframe's first frame waits for `requestAnimationFrame`. This was observed with
   an Edge window that started occluded. E-2 should mount visible, or treat "ready pending while
   hidden" as normal, not as a crash.
4. **The in-viewport 1×1 `opacity:0` iframe still draws about 1 frame/s.** It is an option if `ui.*`
   on hidden mounts ever matters, at about 1 s per call.
5. **The contract table's `engine.execute {id, params}` shape is wrong.** Upstream's control channel
   and `figcraft-mcp` send `{command, params}`. The fork keeps upstream's shape so `Bridge` forwards
   without translating. `integration-contracts.md` should say `{command, params}`.
6. **Choices the contract left open:**
   - Events carry `mountSeq` from `?ms=`.
   - `host.renderPng` / `host.exportBytes` return a bare base64 JSON string.
   - A stale `host.markSaved` is a no-op that returns `{dirty: true, contentVersion}`.
7. **`host.snapshot` during a version preview is unhandled.** It serializes `ds.doc`, which is the
   previewed version. Kiosk mode (E1-2) should end previews before a save, or the snapshot should
   use the document itself.
