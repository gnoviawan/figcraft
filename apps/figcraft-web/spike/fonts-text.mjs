// A bundle built with craft-fonts embeds fonts, so text has real width and draws. Creates a text
// node, checks text.fonts, the node's width and a PNG export, and saves fonts-text.png.
//   node apps/figcraft-web/spike/serve.mjs &
//   PW_DIR=<dir with node_modules/playwright-core> node apps/figcraft-web/spike/fonts-text.mjs
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(join(process.env.PW_DIR || '.', 'x.js'));
const { chromium } = require('playwright-core');
const here = fileURLToPath(new URL('.', import.meta.url));

const browser = await chromium.launch({ channel: process.env.CHANNEL || 'msedge', headless: !process.env.HEADED, args: ['--enable-unsafe-webgpu'] });
let failed = false;
const expect = (ok, what) => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`); if (!ok) failed = true; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const page = await (await browser.newContext({ viewport: { width: 1600, height: 900 } })).newPage();
await page.goto(`http://localhost:8768/host.html?kiosk${process.env.WEBGL ? '&webgl' : ''}`);
await page.evaluate(() => bridge.ready());
const raw = async (method, params = {}) => {
  const r = await page.evaluate(([m, p]) => bridge.call(m, p), [method, JSON.stringify(params)]);
  if (!r.ok) throw new Error(`${method}: ${JSON.stringify(r.error)}`);
  return r.result;
};
const call = async (method, params = {}) => JSON.parse(await raw(method, params));
const exec = (command, params = {}) => raw('engine.execute', { command, params });

const fonts = JSON.parse(await exec('text.fonts'));
const families = Array.isArray(fonts) ? fonts.map((f) => (typeof f === 'string' ? f : f.family)) : [];
console.log('text.fonts:', JSON.stringify(fonts).slice(0, 300));
expect(families.includes('Inter'), `text.fonts lists Inter (${families.length} entries)`);
const styles = JSON.parse(await exec('text.fonts', { family: 'Inter' }));
console.log('Inter styles:', JSON.stringify(styles));

const made = await exec('text.create', { text: 'Termul Canvas text', x: 40, y: 60, props: { fontSize: 64, fontFamily: 'Inter' } });
const id = made.match(/"id"\s*:\s*"?(\d+)/)[1];
await exec('select.none');
const node = await call('node.get', { id });
console.log('node.get:', JSON.stringify(node).slice(0, 400));
const width = Number(node.width ?? node.size?.[0] ?? node.bounds?.width ?? 0);
expect(width > 100, `text layer has real width: ${width}`);

const png = await raw('host.renderPng', { ids: [id], scale: 1 });
const bytes = Buffer.from(png, 'base64');
expect(bytes.length > 1500 && bytes.subarray(1, 4).toString() === 'PNG', `PNG export is not empty: ${bytes.length} bytes`);

await sleep(600);
await page.locator('#frame').screenshot({ path: join(here, 'fonts-text.png') });
await browser.close();
process.exit(failed ? 1 : 0);
