// BrowseCraft end-to-end: two profiles sign in to the same sync server; a bookmark made in one
// appears in the other. Run inside the builder image with Xvfb running:
//   node e2e.mjs <chrome binary> <server url> <user> <password>
import { spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [chromeBin, server, user, password] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);

function launch(dir, port) {
  const p = spawn(chromeBin, [
    `--user-data-dir=${dir}`, `--remote-debugging-port=${port}`, '--enable-local-sync-backend',
    '--no-first-run', '--no-default-browser-check', '--password-store=basic', '--disable-gpu',
    '--no-sandbox', 'about:blank',
  ], { stdio: ['ignore', 'ignore', 'pipe'] });
  p.stderr.on('data', (d) => { const s = d.toString(); if (/sync|browsecraft/i.test(s)) process.stderr.write(`[${port}] ${s}`); });
  return p;
}

async function cdp(port) {
  for (let i = 0; i < 60; i++) {
    try {
      const v = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
      const ws = new WebSocket(v.webSocketDebuggerUrl);
      await new Promise((ok, bad) => { ws.onopen = ok; ws.onerror = bad; });
      let id = 0; const waits = new Map();
      ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && waits.has(d.id)) { waits.get(d.id)(d); waits.delete(d.id); } };
      const send = (method, params = {}, sessionId) => new Promise((ok) => { const i2 = ++id; waits.set(i2, ok); ws.send(JSON.stringify({ id: i2, method, params, sessionId })); });
      return { ws, send };
    } catch { await sleep(500); }
  }
  throw new Error('no devtools on ' + port);
}

async function page(c, url) {
  const t = await c.send('Target.createTarget', { url });
  const a = await c.send('Target.attachToTarget', { targetId: t.result.targetId, flatten: true });
  const s = a.result.sessionId;
  await sleep(1500);
  const ev = async (expr) => {
    const r = await c.send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }, s);
    if (r.result?.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails).slice(0, 300));
    return r.result?.result?.value;
  };
  return { ev, close: () => c.send('Target.closeTarget', { targetId: t.result.targetId }) };
}

async function signIn(dir, port, name) {
  let p = launch(dir, port);
  let c = await cdp(port);
  const pg = await page(c, 'chrome://browsecraft');
  await pg.ev(`document.getElementById('server').value=${JSON.stringify(server)};document.getElementById('user').value=${JSON.stringify(user)};document.getElementById('password').value=${JSON.stringify(password)};document.getElementById('form').requestSubmit();true`);
  let state = '';
  for (let i = 0; i < 30; i++) {
    await sleep(500);
    state = await pg.ev(`document.getElementById('signed-in').hidden ? ('form; error=' + document.getElementById('error').textContent) : ('signed in as ' + document.getElementById('who-user').textContent)`);
    if (state.startsWith('signed in') || state.includes('error=') && !state.endsWith('error=')) break;
  }
  log(name, 'chrome://browsecraft:', state);
  if (!state.startsWith('signed in')) throw new Error('sign-in failed');
  c.ws.close(); p.kill(); await sleep(3000);
  p = launch(dir, port);
  c = await cdp(port);
  return { p, c };
}

async function syncState(c) {
  const pg = await page(c, 'chrome://sync-internals');
  await sleep(2000);
  const t = await pg.ev(`document.body.innerText.slice(0, 4000)`);
  await pg.close();
  return t;
}

const a = await signIn(mkdtempSync(join(tmpdir(), 'bc-a-')), 9301, 'A');
await sleep(8000);
const bmA = await page(a.c, 'chrome://bookmarks');
const made = await bmA.ev(`new Promise(r => chrome.bookmarks.create({parentId:'1', title:'BrowseCraft sync test', url:'https://example.org/browsecraft'}, b => r(b && b.id)))`);
log('A made bookmark', made);
await sleep(15000);
const b = await signIn(mkdtempSync(join(tmpdir(), 'bc-b-')), 9302, 'B');
let found = 0;
const bmB = await page(b.c, 'chrome://bookmarks');
for (let i = 0; i < 24 && !found; i++) {
  await sleep(5000);
  found = await bmB.ev(`new Promise(r => chrome.bookmarks.search('BrowseCraft sync test', x => r(x.length)))`);
}
log('B sees the bookmark:', found ? 'YES' : 'no');
if (!found) {
  log('A sync-internals:\n' + (await syncState(a.c)).slice(0, 2500));
  log('B sync-internals:\n' + (await syncState(b.c)).slice(0, 2500));
}
a.p.kill(); b.p.kill();
process.exit(found ? 0 : 1);
