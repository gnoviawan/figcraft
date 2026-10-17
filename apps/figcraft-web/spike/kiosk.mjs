// Runs the built bundle twice in Edge, as the full editor and under ?embed=kiosk, checks what the
// host sees over postMessage and saves a screenshot of each next to this file.
//   node apps/figcraft-web/spike/serve.mjs &   (serves the bundle and this harness)
//   PW_DIR=<dir with node_modules/playwright-core> node apps/figcraft-web/spike/kiosk.mjs
// Env: CHANNEL (msedge), HEADED=1, WEBGL=1 (force the WebGL2 backend), OUT (screenshot directory).
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(join(process.env.PW_DIR || '.', 'x.js'));
const { chromium } = require('playwright-core');
const here = fileURLToPath(new URL('.', import.meta.url));
const outDir = process.env.OUT || here;

const browser = await chromium.launch({ channel: process.env.CHANNEL || 'msedge', headless: !process.env.HEADED, args: ['--enable-unsafe-webgpu'] });
const results = {};
let failed = false;

for (const mode of ['full', 'kiosk']) {
  const page = await (await browser.newContext({ viewport: { width: 1600, height: 900 } })).newPage();
  await page.goto(`http://localhost:8768/host.html?${mode === 'kiosk' ? 'kiosk' : 'full'}${process.env.WEBGL ? '&webgl' : ''}`);
  await page.evaluate(() => bridge.ready());
  const call = (method, params = {}) => page.evaluate(([m, p]) => bridge.call(m, p, 5000), [method, JSON.stringify(params)]);
  const code = async (method, params) => {
    const r = await call(method, params);
    return r.ok ? 'ok' : r.error.code;
  };
  await call('engine.execute', { command: 'shape.rectangle', params: { x: 40, y: 40, width: 200, height: 120 } });
  await page.waitForTimeout(800);
  const ui = JSON.parse((await call('ui.inspect')).result);
  await page.screenshot({ path: join(outDir, `kiosk-${mode}.png`) });
  const probes = {
    'app.quit': await code('app.quit'),
    'window.new': await code('window.new'),
    'ui.set home': await code('ui.set', { home: true }),
    'ui.set mainMenu': await code('ui.set', { mainMenu: true }),
    'ui.menu.invoke file.open': await code('ui.menu.invoke', { id: 'file.open' }),
  };
  await page.waitForTimeout(500);
  const after = JSON.parse((await call('ui.inspect')).result);
  if (mode === 'kiosk') await page.screenshot({ path: join(outDir, 'kiosk-kiosk-after-probes.png') });
  results[mode] = { canvasTop: ui.canvas.rect[1], probes, homeAfter: after.home, mainMenuAfter: after.mainMenu };
  await page.close();
}

const { full, kiosk } = results;
const expect = (ok, what) => { if (!ok) { failed = true; console.error(`FAIL ${what}`); } };
expect(full.canvasTop > 0, 'the full editor has a tab bar above the canvas');
expect(kiosk.canvasTop === 0, 'the kiosk canvas starts at the top');
for (const [name, c] of Object.entries(kiosk.probes)) expect(c === 'UNKNOWN_METHOD', `kiosk ${name} answers UNKNOWN_METHOD, got ${c}`);
expect(!kiosk.homeAfter && !kiosk.mainMenuAfter, 'the kiosk never shows home or the main menu');
console.log(JSON.stringify(results, null, 2));
await browser.close();
process.exit(failed ? 1 : 0);
