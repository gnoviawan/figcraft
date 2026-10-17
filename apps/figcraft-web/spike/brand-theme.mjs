// Screenshots of a bundle built with FIGCRAFT_BRAND_NAME, themed and localised by the host:
// Termul dark, Termul light, About, and German. Writes brand-*.png next to this file.
//   node apps/figcraft-web/spike/serve.mjs &
//   PW_DIR=<dir with node_modules/playwright-core> node apps/figcraft-web/spike/brand-theme.mjs
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(join(process.env.PW_DIR || '.', 'x.js'));
const { chromium } = require('playwright-core');
const here = fileURLToPath(new URL('.', import.meta.url));

const termulDark = { mode: 'dark', background: '#08090a', card: '#0f1011', foreground: '#e5e5e6', mutedForeground: '#8a8f98',
  border: '#23252a', primary: '#3b82f6', primaryFill: '#3b82f6', destructive: '#ef4444' };
const termulLight = { mode: 'light', background: '#ffffff', card: '#ffffff', foreground: '#0d0d0d', mutedForeground: '#6b6b6b',
  border: '#e6e6e6', primary: '#2563eb', primaryFill: '#3b82f6', destructive: '#dc2626' };

const browser = await chromium.launch({ channel: process.env.CHANNEL || 'msedge', headless: !process.env.HEADED, args: ['--enable-unsafe-webgpu'] });
const page = await (await browser.newContext({ viewport: { width: 1600, height: 900 } })).newPage();
await page.goto(`http://localhost:8768/host.html${process.env.WEBGL ? '?webgl' : ''}`);
await page.evaluate(() => bridge.ready());

const call = async (method, params = {}) => {
  const r = await page.evaluate(([m, p]) => bridge.call(m, p), [method, JSON.stringify(params)]);
  if (!r.ok) throw new Error(`${method}: ${JSON.stringify(r.error)}`);
  return JSON.parse(r.result);
};
const settle = async () => {
  await call('ui.inspect');
  await call('ui.inspect');
};
const shot = async (name) => {
  await settle();
  await page.locator('#frame').screenshot({ path: join(here, `brand-${name}.png`) });
  console.log(`brand-${name}.png`);
};

for (let i = 0; i < 3; i++) {
  await call('engine.execute', { command: 'shape.rectangle', params: { x: 80 + 160 * i, y: 80, width: 120, height: 90 } });
}
console.log('describe', await call('host.describe'));
console.log('theme dark', await call('host.theme', { tokens: termulDark }));
await shot('dark');
console.log('theme light', await call('host.theme', { tokens: termulLight }));
await shot('light');
await call('ui.menu.invoke', { id: 'help.about' });
await shot('about');
await call('ui.key', { key: 'Escape' });
console.log('locale', await call('host.locale', { bcp47: 'de-DE' }));
await call('ui.menu.invoke', { id: 'help.about' });
await shot('about-de');
await browser.close();
