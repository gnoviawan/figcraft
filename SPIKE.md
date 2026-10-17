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
The bundle was built without `craft-fonts` (`CRAFT_FONTS_DIR` is missing here). `package.sh --skip-build`
passed its size gate ("22360389 bytes (21 MiB; limit 25165824)") and then stopped, because this
machine has no `zip`.

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
  - The Help menu's link to getartcraft.com. The kiosk build hides that menu.
  - FigCraft's own chameleon-tail glyph (`Icon::Logo`, original MIT artwork, not an ArtCraft mark)
    on the rail's main-menu button and the document tabs. The kiosk build hides both.

### Verified

`spike/brand-theme.mjs` drives a bundle built with `FIGCRAFT_BRAND_NAME="Termul Canvas"` and saves
four screenshots: `brand-dark.png`, `brand-light.png`, `brand-about.png` and `brand-about-de.png`.
Termul dark and then Termul light are applied to the same mount, with no reload. About shows only
the name, the version and the attribution. After `host.locale {bcp47: "de-DE"}` the UI is German
and About reads "Über Termul Canvas".

Not themed by the tokens:

- The canvas page colour, which is document data.
- Canvas overlays and the Share button, which use the `ACCENT` constant rather than `Tokens`.

In the built wasm, every "ArtCraft" is in one of these places:

- The two About strings a rebranded build does not show, in each language.
- The attribution.

Every "FigCraft" is in one of these places:

- Catalog keys, each followed by its renamed value.
- Engine, format and plugin error messages.
- The `brand::UPSTREAM` constant.

The test `brand::tests::a_rebranded_build_shows_no_upstream_name_or_artcraft_mark` checks every
catalog value in every language.

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
- This was built without `craft-fonts`.

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
| Bundle < 24 MiB | 21.3 MiB without `craft-fonts` | **GO**, 2.7 MiB headroom. Embedding `craft-fonts` is unmeasured and could eat it |
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
