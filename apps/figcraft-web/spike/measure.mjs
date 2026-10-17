// Drives host.html in Chromium (Edge by default) and prints the E-0 numbers as JSON.
//   node apps/figcraft-web/spike/serve.mjs &   (serves the bundle and this harness)
//   PW_DIR=<dir with node_modules/playwright-core> node apps/figcraft-web/spike/measure.mjs
// Env: CHANNEL (msedge), HEADED=1, WEBGL=1 (force the WebGL2 backend), NODES (500).
import { createRequire } from 'node:module';
import { readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(join(process.env.PW_DIR || '.', 'x.js'));
const { chromium } = require('playwright-core');

const here = fileURLToPath(new URL('.', import.meta.url));
const NODES = Number(process.env.NODES || 500);
const stats = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  const q = (p) => s[Math.min(s.length - 1, Math.floor(p * s.length))];
  return { n: s.length, p50: +q(0.5).toFixed(2), p95: +q(0.95).toFixed(2), max: +s[s.length - 1].toFixed(2) };
};

const browser = await chromium.launch({ channel: process.env.CHANNEL || 'msedge', headless: !process.env.HEADED,
  args: ['--enable-unsafe-webgpu'],
  // Playwright keeps background pages running at full speed; a real WebView does not.
  ignoreDefaultArgs: ['--disable-backgrounding-occluded-windows', '--disable-renderer-backgrounding', '--disable-background-timer-throttling'],
});
const context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
const page = await context.newPage();
const consoleLines = [];
page.on('console', (m) => consoleLines.push(m.text()));
const out = { browser: `${browser.browserType().name()} ${browser.version()}`, headless: !process.env.HEADED };

const t0 = Date.now();
await page.goto(`http://localhost:8768/host.html${process.env.WEBGL ? '?webgl' : ''}`);
await page.evaluate(() => bridge.ready());
out.readyMs = Date.now() - t0;
out.backend = consoleLines.find((l) => l.includes('wgpu backend')) || 'unknown';

const call = (method, params = {}, timeoutMs = 5000) =>
  page.evaluate(([m, p, t]) => bridge.call(m, p, t), [method, JSON.stringify(params), timeoutMs]);
const frames = async () => JSON.parse((await call('ui.inspect')).result).framesDrawn;

out.describe = JSON.parse((await call('host.describe')).result);

// Repaint: the frame right after an external execute is the one that answers the next ui.* call
// (control requests drain at the start of a frame, before it draws the document).
const repaint = { execMs: [], toNextFrameMs: [], framesBetween: [] };
for (let i = 0; i < 20; i++) {
  const before = await frames();
  const r = await page.evaluate(async (i) => {
    const t = performance.now();
    const ex = await bridge.call('engine.execute', JSON.stringify({ command: 'shape.rectangle', params: { x: 30 * i, y: 0, width: 20, height: 20 } }));
    const ui = await bridge.call('ui.inspect');
    return { exec: ex.ms, total: performance.now() - t, after: JSON.parse(ui.result).framesDrawn };
  }, i);
  repaint.execMs.push(r.exec);
  repaint.toNextFrameMs.push(r.total);
  repaint.framesBetween.push(r.after - before);
}
await page.screenshot({ path: join(process.env.TMP || here, 'spike-repaint.png') });
out.repaint = { execMs: stats(repaint.execMs), sendToNextFrameMs: stats(repaint.toNextFrameMs), framesBetween: repaint.framesBetween };

async function latency(label) {
  const sync = [], exec = [], ui = [], uiErrors = {};
  for (let i = 0; i < 30; i++) {
    const a = await call('document.inspect', { depth: 1 });
    if (a.ok) sync.push(a.ms);
    const b = await call('engine.execute', { command: 'select.none' });
    if (b.ok) exec.push(b.ms);
  }
  for (let i = 0; i < 5; i++) {
    const c = await call('ui.inspect', {}, 3000);
    if (c.ok) ui.push(c.ms);
    else uiErrors[c.error.code] = (uiErrors[c.error.code] || 0) + 1;
  }
  const visibility = await page.evaluate(() => document.visibilityState);
  return { label, visibility, documentInspect: stats(sync), execute: stats(exec), uiInspect: ui.length ? stats(ui) : null, uiErrors };
}

const setMode = (mode) => page.evaluate((m) => { bridge.frame.className = m; }, mode);
out.latency = [];
for (const [mode, label] of [['', 'visible'], ['offscreen', 'offscreen 1x1'], ['tiny', 'in-view 1x1 opacity 0'], ['none', 'display:none']]) {
  await setMode(mode);
  await page.waitForTimeout(1500);
  out.latency.push(await latency(label));
}
await setMode('');


// Page hidden. Headless Chromium keeps every page "visible", so this needs HEADED=1: first another
// tab in front, then the window minimised.
async function hiddenRuns(label, hide, show) {
  await hide();
  await page.waitForTimeout(1500);
  out.latency.push(await latency(label));
  await setMode('offscreen');
  await page.waitForTimeout(500);
  out.latency.push(await latency(`${label} + offscreen 1x1`));
  await setMode('');
  await show();
  await page.waitForTimeout(1000);
}
if (process.env.HEADED) {
  const cover = await context.newPage();
  await hiddenRuns('page hidden (other tab in front)', () => cover.bringToFront(), () => page.bringToFront());
  await cover.close();
  const cdp = await context.newCDPSession(page);
  const { windowId } = await cdp.send('Browser.getWindowForTarget');
  const state = (windowState) => () => cdp.send('Browser.setWindowBounds', { windowId, bounds: { windowState } });
  await hiddenRuns('window minimised', state('minimized'), state('normal'));
}

// A document of NODES overlapping rectangles (no grid, so no spacing handles), all selected and
// dragged with the real mouse. Each frame's
// own cost is timed by wrapping the editor's requestAnimationFrame callbacks.
await call('engine.execute', { command: 'select.all' });
await call('engine.execute', { command: 'edit.delete' });
const tc = Date.now();
for (let i = 0; i < NODES; i++) {
  await call('engine.execute', { command: 'shape.rectangle', params: { x: (i * 37) % 600, y: (i * 53) % 400, width: 60, height: 60 } });
}
out.createNodesMs = Date.now() - tc;
await call('engine.execute', { command: 'view.zoomFit' });
await call('engine.execute', { command: 'select.all' });
const doc = JSON.parse((await call('document.inspect', { depth: 1 })).result);
const inspect = JSON.parse((await call('ui.inspect')).result);
const [x0, y0, x1, y1] = inspect.canvas.rect;
const box = await page.locator('#frame').boundingBox();
const cx = box.x + (x0 + x1) / 2, cy = box.y + (y0 + y1) / 2;
const editor = page.frames().find((f) => f.url().startsWith('http://127.0.0.1:8767'));
await editor.evaluate(() => {
  window.__work = [];
  window.__gaps = [];
  let last = 0;
  const raf = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (cb) => raf((t) => {
    const s = performance.now();
    cb(t);
    window.__work.push(performance.now() - s);
    if (last) window.__gaps.push(s - last);
    last = s;
  });
});
await page.mouse.move(cx, cy);
await page.waitForTimeout(500);
await editor.evaluate(() => { window.__work = []; window.__gaps = []; });
const framesBefore = await frames();
const td = Date.now();
await page.mouse.down();
for (let i = 1; i <= 120; i++) {
  await page.mouse.move(cx + i * 2, cy + Math.sin(i / 10) * 40);
}
await page.mouse.up();
const dragMs = Date.now() - td;
const timing = await editor.evaluate(() => ({ work: window.__work.slice(), gaps: window.__gaps.slice() }));
const framesAfter = await frames();
const after = JSON.parse((await call('document.inspect', { depth: 1 })).result);
const first = (d) => { const c = d.pages?.find((p) => p.id === d.currentPage)?.children?.[0]; return c ? { x: c.x, y: c.y } : null; };
out.drag = {
  nodes: NODES,
  pageChildren: doc.pages?.find((p) => p.id === doc.currentPage)?.children?.length,
  selected: doc.selection?.length,
  mouseMoves: 120,
  dragMs,
  framesDrawn: framesAfter - framesBefore,
  frameWorkMs: stats(timing.work),
  frameIntervalMs: stats(timing.gaps),
  lastUndo: after.history?.undo?.[0],
  firstNodeBefore: first(doc),
  firstNodeAfter: first(after),
};
out.dirtyEvents = await page.evaluate(() => bridge.events.filter((e) => e.method === 'event.dirtyChanged').length);
out.events = await page.evaluate(() => [...new Set(bridge.events.map((e) => e.method))]);

const dist = process.env.BUNDLE_DIR || join(here, '../../../dist/web');
out.bundle = readdirSync(dist).filter((f) => f.endsWith('.wasm') || f.endsWith('.js')).map((f) => ({ file: f, bytes: statSync(join(dist, f)).size }));

console.log(JSON.stringify(out, null, 2));
await browser.close();
