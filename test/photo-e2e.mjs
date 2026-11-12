// The profile photo follows the user: browser A signs in and uploads a photo; browser B (another
// profile folder, as on another PC) signs in and shows it. node photo-e2e.mjs <chrome> <server> <user> <pwfile>
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [chromeBin, server, user, pwFile] = process.argv.slice(2);
const password = readFileSync(pwFile, 'utf8').trim();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);
let failed = 0;
const check = (ok, what) => { log(ok ? 'PASS' : 'FAIL', what); if (!ok) failed++; };

async function browser(port) {
  const dir = mkdtempSync(join(tmpdir(), 'bc-photo-'));
  const proc = spawn(chromeBin, [`--user-data-dir=${dir}`, `--remote-debugging-port=${port}`, '--no-first-run',
    '--no-default-browser-check', '--password-store=basic', '--disable-gpu', '--no-sandbox', 'about:blank'], { stdio: 'ignore' });
  const exited = new Promise((r) => proc.on('exit', r));
  let c;
  for (let i = 0; i < 60 && !c; i++) {
    try {
      const v = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
      const ws = new WebSocket(v.webSocketDebuggerUrl);
      await new Promise((ok, bad) => { ws.onopen = ok; ws.onerror = bad; });
      let id = 0; const waits = new Map();
      ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && waits.has(d.id)) { waits.get(d.id)(d); waits.delete(d.id); } };
      c = { send: (method, params = {}, sessionId) => new Promise((ok) => { const i2 = ++id; waits.set(i2, ok); ws.send(JSON.stringify({ id: i2, method, params, sessionId })); }) };
    } catch { await sleep(500); }
  }
  const open = async (url) => {
    const t = await c.send('Target.createTarget', { url });
    const s = (await c.send('Target.attachToTarget', { targetId: t.result.targetId, flatten: true })).result.sessionId;
    await sleep(3000);
    return async (e) => { const r = await c.send('Runtime.evaluate', { expression: e, awaitPromise: true, returnByValue: true }, s); return r.result?.result?.value ?? JSON.stringify(r).slice(0, 300); };
  };
  const signIn = async () => {
    const page = await open('chrome://browsecraft/');
    return page(`new Promise((ok) => {
      const orig = window.browsecraft.onSignIn;
      window.browsecraft.onSignIn = (r) => { orig(r); ok(r.ok ? 'ok' : r.error); };
      document.getElementById('server').value = ${JSON.stringify(server)};
      document.getElementById('user').value = ${JSON.stringify(user)};
      document.getElementById('password').value = ${JSON.stringify(password)};
      document.getElementById('form').requestSubmit();
    })`);
  };
  const close = async () => { await c.send('Browser.close'); await exited; return JSON.parse(readFileSync(join(dir, 'Local State'), 'utf8')).profile?.info_cache?.Default || {}; };
  return { dir, open, signIn, close };
}

// A: sign in, upload a photo.
const a = await browser(9341);
check((await a.signIn()) === 'ok', 'browser A signs in');
const settings = await a.open('chrome://settings/manageProfile');
await settings(`(() => { const c = document.createElement('canvas'); c.width = 300; c.height = 300;
  const g = c.getContext('2d'); g.fillStyle = '#8e44ad'; g.fillRect(0, 0, 300, 300); g.fillStyle = '#f1c40f'; g.fillRect(100, 100, 100, 100);
  const u = c.toDataURL('image/png'); chrome.send('setProfilePhoto', [u.substring(u.indexOf(',') + 1)]); return 'sent'; })()`);
await sleep(6000);
const ea = await a.close();
check(existsSync(join(a.dir, 'Default', 'Google Profile Picture.png')), 'browser A shows its photo');

// B: a fresh profile folder signs in as the same user: the photo arrives by itself.
const b = await browser(9342);
check((await b.signIn()) === 'ok', 'browser B signs in');
await sleep(10000);
const eb = await b.close();
const pic = join(b.dir, 'Default', 'Google Profile Picture.png');
check(existsSync(pic) && eb.use_gaia_picture === true, `browser B got the photo from the server (${eb.gaia_picture_file_name}, use ${eb.use_gaia_picture})`);
if (existsSync(pic)) {
  const png = readFileSync(pic);
  log('B photo:', png.length, 'bytes,', png.readUInt32BE(16), 'x', png.readUInt32BE(20));
}
log(ea.name, '/', eb.name);
log(failed ? `${failed} FAILED` : 'ALL PASSED');
process.exit(failed ? 1 : 0);
