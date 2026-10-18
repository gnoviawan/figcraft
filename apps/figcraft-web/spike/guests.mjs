// Guest presence (host.guestCursor / guestFlash / guestHide) in a kiosk mount: one agent visits
// three rectangles, writes one and reads another, a second agent joins, one target is off screen,
// then the first agent hides. Saves guests-*.png (and guests.webm when Playwright can record)
// next to this file.
//   node apps/figcraft-web/spike/serve.mjs &
//   PW_DIR=<dir with node_modules/playwright-core> node apps/figcraft-web/spike/guests.mjs
import { createRequire } from 'node:module';
import { renameSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(join(process.env.PW_DIR || '.', 'x.js'));
const { chromium } = require('playwright-core');
const here = fileURLToPath(new URL('.', import.meta.url));
const videoDir = join(here, 'guests-video');

const termulDark = { mode: 'dark', background: '#08090a', card: '#0f1011', foreground: '#e5e5e6', mutedForeground: '#8a8f98',
  border: '#23252a', primary: '#5b9bff', primaryFill: '#3b82f6', destructive: '#f0605d' };

const browser = await chromium.launch({ channel: process.env.CHANNEL || 'msedge', headless: !process.env.HEADED, args: ['--enable-unsafe-webgpu'] });
let failed = false;
const expect = (ok, what) => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`); if (!ok) failed = true; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let context;
try {
  context = await browser.newContext({ viewport: { width: 1600, height: 900 }, recordVideo: { dir: videoDir, size: { width: 1600, height: 900 } } });
} catch (e) {
  console.log('no video:', e.message.split('\n')[0]);
  context = await browser.newContext({ viewport: { width: 1600, height: 900 } });
}
const page = await context.newPage();
await page.goto(`http://localhost:8768/host.html?kiosk${process.env.WEBGL ? '&webgl' : ''}`);
await page.evaluate(() => bridge.ready());
// Results stay JSON text here: node ids exceed 2^53.
const raw = async (method, params = {}) => {
  const r = await page.evaluate(([m, p]) => bridge.call(m, p), [method, JSON.stringify(params)]);
  if (!r.ok) throw new Error(`${method}: ${JSON.stringify(r.error)}`);
  return r.result;
};
const call = async (method, params = {}) => JSON.parse(await raw(method, params));
const inspect = async () => (await call('ui.inspect')).guests;
const shot = (name) => page.locator('#frame').screenshot({ path: join(here, `guests-${name}.png`) });

await call('host.theme', { tokens: termulDark });
const rects = [
  { x: 0, y: 40, width: 180, height: 120 },
  { x: 380, y: 260, width: 200, height: 140 },
  { x: 40, y: 480, width: 220, height: 150 },
];
const ids = [];
for (const r of rects) {
  const text = await raw('engine.execute', { command: 'shape.rectangle', params: r });
  ids.push(text.match(/"id"\s*:\s*"?(\d+)/)[1]);
}
await raw('engine.execute', { command: 'select.none' });
const centre = (i) => ({ x: rects[i].x + rects[i].width / 2, y: rects[i].y + rects[i].height / 2 });
const before = (await call('host.snapshot')).contentVersion;
await sleep(300);
const since = await page.evaluate(() => performance.now());

const claude = (at, extra = {}) => call('host.guestCursor', { id: 'sess-claude', name: 'Claude', color: '#7C9CFF', ...at, ...extra });

await claude(centre(0));
await sleep(600);
let g = await inspect();
expect(g.length === 1 && g[0].name === 'Claude' && g[0].onCanvas, `Claude shows on the canvas: ${JSON.stringify(g)}`);
expect(g[0].shown[0] === centre(0).x && g[0].shown[1] === centre(0).y, 'appears at its first target');
await call('host.guestFlash', { id: 'sess-claude', ids: [ids[0]], kind: 'write' });
await sleep(350);
g = await inspect();
expect(g[0].flashes.length === 1 && g[0].flashes[0].kind === 'write' && g[0].flashes[0].id === ids[0], `write ring on the first rectangle: ${JSON.stringify(g[0].flashes)}`);
await shot('1-write');

await claude(centre(1));
await sleep(250);
g = await inspect();
const t1 = centre(1);
expect(g[0].shown[0] > centre(0).x && g[0].shown[0] < t1.x, `glides, not jumps: x ${g[0].shown[0]}`);
await shot('2-glide');
await sleep(1500);
await call('host.guestFlash', { id: 'sess-claude', ids: [ids[1]], kind: 'read' });
await sleep(700);
await shot('3-read');
g = await inspect();
expect(Math.hypot(g[0].shown[0] - t1.x, g[0].shown[1] - t1.y) < 2, 'arrived at the second rectangle');
expect(g[0].flashes.some((f) => f.kind === 'read' && f.id === ids[1]), 'read ring on the second rectangle');

await call('host.guestCursor', { id: 'sess-opencode', name: 'opencode', color: '#F472B6', ...centre(2) });
await call('host.guestFlash', { id: 'sess-opencode', ids: [ids[2]], kind: 'write' });
await claude({ x: centre(2).x + 200, y: centre(2).y - 30 });
await sleep(450);
await shot('4-two-guests');
g = await inspect();
expect(g.length === 2 && g.map((x) => x.id).join() === 'sess-claude,sess-opencode', 'two guests at once');

await claude({ x: 6000, y: 300 });
await sleep(2500);
await shot('5-edge');
g = await inspect();
expect(g.find((x) => x.id === 'sess-claude').onCanvas === false, 'an off-screen target shows as an edge arrow');

await call('host.guestHide', { id: 'sess-claude' });
await call('host.guestFlash', { id: 'sess-opencode', ids: [ids[2]], kind: 'read' });
await sleep(400);
await shot('6-hiding');
await sleep(1400);
g = await inspect();
expect(g.length === 1 && g[0].id === 'sess-opencode', `Claude is gone after hide: ${JSON.stringify(g.map((x) => x.id))}`);
await shot('7-hidden');

const after = (await call('host.snapshot')).contentVersion;
expect(after === before, `contentVersion unchanged by guests (${before} -> ${after})`);
const late = await page.evaluate((t) => bridge.events.filter((e) => e.method === 'event.dirtyChanged' && e.at > t).length, since);
expect(late === 0, `no event.dirtyChanged while guests moved (${late})`);

await sleep(5500);
g = await inspect();
expect(g.length === 0, 'an idle guest fades out on its own');

const video = page.video();
await context.close();
if (video) {
  try {
    renameSync(await video.path(), join(here, 'guests.webm'));
    console.log('saved guests.webm');
  } catch (e) {
    console.log('no video:', e.message.split('\n')[0]);
  }
  rmSync(videoDir, { recursive: true, force: true });
}
await browser.close();
process.exit(failed ? 1 : 0);
