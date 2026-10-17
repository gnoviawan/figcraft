// Kiosk, brand and the host's dark theme in one mount. Needs a bundle built with
// FIGCRAFT_BRAND_NAME="Termul Canvas". Saves integrated-kiosk-brand.png next to this file.
//   node apps/figcraft-web/spike/serve.mjs &
//   PW_DIR=<dir with node_modules/playwright-core> node apps/figcraft-web/spike/integrated.mjs
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(join(process.env.PW_DIR || '.', 'x.js'));
const { chromium } = require('playwright-core');
const here = fileURLToPath(new URL('.', import.meta.url));
const brand = process.env.BRAND || 'Termul Canvas';

const termulDark = { mode: 'dark', background: '#08090a', card: '#0f1011', foreground: '#e5e5e6', mutedForeground: '#8a8f98',
  border: '#23252a', primary: '#5b9bff', primaryFill: '#3b82f6', destructive: '#f0605d' };

const browser = await chromium.launch({ channel: process.env.CHANNEL || 'msedge', headless: !process.env.HEADED, args: ['--enable-unsafe-webgpu'] });
let failed = false;
const expect = (ok, what) => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`); if (!ok) failed = true; };

const bare = await (await browser.newContext()).newPage();
await bare.goto('http://127.0.0.1:8767/', { waitUntil: 'commit' });
expect((await bare.title()) === brand, `index.html title is "${brand}", got "${await bare.title()}"`);
expect((await bare.locator('#figcraft_loading').innerText().catch(() => '')).includes(brand) || (await bare.content()).includes(`Loading ${brand}`), 'loading text names the brand');
await bare.close();

const page = await (await browser.newContext({ viewport: { width: 1600, height: 900 } })).newPage();
await page.goto(`http://localhost:8768/host.html?kiosk${process.env.WEBGL ? '&webgl' : ''}`);
console.log('iframe src', await page.evaluate(() => document.getElementById('frame').src));
await page.evaluate(() => bridge.ready());
const call = async (method, params = {}) => {
  const r = await page.evaluate(([m, p]) => bridge.call(m, p), [method, JSON.stringify(params)]);
  if (!r.ok) throw new Error(`${method}: ${JSON.stringify(r.error)}`);
  return JSON.parse(r.result);
};

for (let i = 0; i < 3; i++) {
  await call('engine.execute', { command: 'shape.rectangle', params: { x: 80 + 160 * i, y: 80, width: 120, height: 90 } });
}
expect((await call('host.theme', { tokens: termulDark })).dark === true, 'host.theme dark applied in kiosk');
await call('ui.inspect');
await call('ui.inspect');
const ui = await call('ui.inspect');
expect(ui.canvas.rect[1] === 0, 'kiosk: no tab bar above the canvas');
expect(!ui.home && !ui.mainMenu, 'kiosk: no home, no main menu');
console.log('describe', await call('host.describe'));
await page.locator('#frame').screenshot({ path: join(here, 'integrated-kiosk-brand.png') });
await browser.close();
process.exit(failed ? 1 : 0);
