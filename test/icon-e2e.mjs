// Store install icon: does beginInstallWithManifest3 get past the icon with our iconUrl, and
// without one? node icon-e2e.mjs <chrome> <server> <user> <password file>
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, createWriteStream } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [chromeBin, server, user, pwFile] = process.argv.slice(2);
const password = readFileSync(pwFile, 'utf8').trim();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const dir = mkdtempSync(join(tmpdir(), 'bc-icon-'));
const proc = spawn(chromeBin, [`--user-data-dir=${dir}`, '--remote-debugging-port=9336', '--no-first-run',
  '--no-default-browser-check', '--password-store=basic', '--disable-gpu', '--no-sandbox', '--enable-logging=stderr',
  '--vmodule=*image_fetcher*=2,*image_data_fetcher*=2,*webstore*=2', 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
proc.stderr.pipe(createWriteStream('/repo/icon-e2e-chrome.log'));

let c;
for (let i = 0; i < 60 && !c; i++) {
  try {
    const v = await (await fetch('http://127.0.0.1:9336/json/version')).json();
    const ws = new WebSocket(v.webSocketDebuggerUrl);
    await new Promise((ok, bad) => { ws.onopen = ok; ws.onerror = bad; });
    let id = 0; const waits = new Map();
    ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && waits.has(d.id)) { waits.get(d.id)(d); waits.delete(d.id); } };
    c = { send: (method, params = {}, sessionId) => new Promise((ok) => { const i2 = ++id; waits.set(i2, ok); ws.send(JSON.stringify({ id: i2, method, params, sessionId })); }) };
  } catch { await sleep(500); }
}
const t = await c.send('Target.createTarget', { url: `${server}/webstore/` });
const s = (await c.send('Target.attachToTarget', { targetId: t.result.targetId, flatten: true })).result.sessionId;
const ev = async (e) => { const r = await c.send('Runtime.evaluate', { expression: e, awaitPromise: true, returnByValue: true }, s); return r.result?.result?.value ?? JSON.stringify(r).slice(0, 600); };
await sleep(3000);
console.log('sign-in:', await ev(`fetch('/webstore/sign-in', {method: 'POST', body: new URLSearchParams({user: ${JSON.stringify(user)}, password: ${JSON.stringify(password)}})}).then(r => r.status)`));
await c.send('Page.reload', {}, s); await sleep(3000);
const id = 'eimadpbcbfnmbkopoojfekhnkhdbieeh';
const manifest = `document.querySelector('[data-id="${id}"]').dataset.manifest`;
console.log('has button:', await ev(`!!document.querySelector('[data-id="${id}"]')`));
console.log('click:', await ev(`(() => { const b = document.querySelector('[data-id="${id}"]'); b.click(); return b.textContent.trim(); })()`));
for (let i = 0; i < 12; i++) { await sleep(1000); const r = await ev(`document.getElementById('s-${id}').textContent`); if (i === 11 || /Not installed|Installed/.test(r)) { console.log('status after', i + 1, 's:', r); break; } }
try { (await import('node:child_process')).execSync('xwd -root -display :96 -out /repo/icon-e2e.xwd'); console.log('screenshot taken'); } catch (e) { console.log('no screenshot:', String(e).slice(0, 120)); }
proc.kill('SIGKILL');
process.exit(0);
