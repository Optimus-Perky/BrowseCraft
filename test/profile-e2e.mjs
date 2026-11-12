// BrowseCraft sign-in, sync without a restart, profile name and photo, end to end. Run as root
// inside the builder image with Xvfb running (profile-e2e.sh does both):
//   node profile-e2e.mjs <chrome binary> <server url> <user> <password file>
// Afterwards it prints the profile's entry from Local State (name, picture).
import { spawn } from 'node:child_process';
import { createWriteStream, existsSync, mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [chromeBin, server, user, passwordFile] = process.argv.slice(2);
const password = readFileSync(passwordFile, 'utf8').trim();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);
let failed = 0;
const check = (ok, what) => { log(ok ? 'PASS' : 'FAIL', what); if (!ok) failed++; };

const dir = mkdtempSync(join(tmpdir(), 'bc-profile-'));
const proc = spawn(chromeBin, [
  `--user-data-dir=${dir}`, '--remote-debugging-port=9334', '--no-first-run', '--no-default-browser-check',
  '--password-store=basic', '--disable-gpu', '--no-sandbox', '--enable-logging=stderr', '--v=0',
  '--vmodule=browsecraft*=1,sync_service_impl*=1,sync_manager_impl*=1', 'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe'] });
proc.stderr.pipe(createWriteStream(process.env.CHROME_LOG || '/tmp/chrome-profile.log'));
const exited = new Promise((r) => proc.on('exit', r));

async function cdp(port) {
  for (let i = 0; i < 60; i++) {
    try {
      const v = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
      const ws = new WebSocket(v.webSocketDebuggerUrl);
      await new Promise((ok, bad) => { ws.onopen = ok; ws.onerror = bad; });
      let id = 0; const waits = new Map();
      ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && waits.has(d.id)) { waits.get(d.id)(d); waits.delete(d.id); } };
      const send = (method, params = {}, sessionId) => new Promise((ok) => { const i2 = ++id; waits.set(i2, ok); ws.send(JSON.stringify({ id: i2, method, params, sessionId })); });
      return { send };
    } catch { await sleep(500); }
  }
  throw new Error('no devtools on ' + port);
}

async function open(c, url) {
  const t = await c.send('Target.createTarget', { url });
  const a = await c.send('Target.attachToTarget', { targetId: t.result.targetId, flatten: true });
  const s = a.result.sessionId;
  await sleep(3000);
  return async (expr) => {
    const r = await c.send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true }, s);
    return r.result?.result?.value ?? JSON.stringify(r.result?.exceptionDetails?.exception?.description ?? r);
  };
}

const c = await cdp(9334);
await sleep(2000);

// 1. The sign-in page: signed out, server suggested from policy, both ways to sign in offered.
const page = await open(c, 'chrome://browsecraft/');
const before = await page(`({out: !document.getElementById('signed-out').hidden,
  server: document.getElementById('server').value,
  windows: !!document.getElementById('sign-in-windows')})`);
check(before.out && before.server === server && before.windows,
  `chrome://browsecraft starts signed out, server ${before.server} from policy, Windows button there`);

// 2. The Windows sign-in says plainly it is Windows-only here (Linux build).
const win = await page(`new Promise((ok) => {
  const orig = window.browsecraft.onSignIn;
  window.browsecraft.onSignIn = (r) => { orig(r); ok(r); };
  document.getElementById('sign-in-windows').click();
})`);
check(!win.ok && /Windows only/.test(win.error), `Windows sign-in on Linux is refused clearly (${win.error})`);

// 2b. The user names the profile themselves first; signing in overwrites it.
const renamer = await open(c, 'chrome://settings/manageProfile');
await renamer(`chrome.send('setProfileName', ['My own name'])`);
await sleep(1000);
const renamed = await renamer(`new Promise((ok) => {
  const orig = window.cr.webUIResponse;
  window.cr.webUIResponse = (id, ok2, resp) => { if (id === 'bc-info') { ok(resp.name); } else { orig(id, ok2, resp); } };
  chrome.send('getProfileInfo', ['bc-info']);
})`);
check(renamed === 'My own name', `the profile was renamed by hand first (${renamed})`);

// 3. Password sign-in: signed in at once, with the server's display name.
const signed = await page(`new Promise((ok) => {
  const orig = window.browsecraft.onSignIn;
  window.browsecraft.onSignIn = (r) => { orig(r); ok(r); };
  document.getElementById('user').value = ${JSON.stringify(user)};
  document.getElementById('password').value = ${JSON.stringify(password)};
  document.getElementById('form').requestSubmit();
})`);
check(signed.ok && signed.status.signedIn, `password sign-in works (${signed.ok ? signed.status.displayName : signed.error})`);
const shown = await page(`document.getElementById('who-name').textContent + ' @ ' + document.getElementById('who-server').textContent`);
log('page shows:', shown);

// 4. Sync starts without a restart: the engine reaches the server within a minute.
let syncing = false;
for (let i = 0; i < 30 && !syncing; i++) {
  await sleep(2000);
  syncing = /Running against the BrowseCraft server/.test(readFileSync(process.env.CHROME_LOG || '/tmp/chrome-profile.log', 'utf8'));
}
check(syncing, 'sync engine started against the server without a restart');
await sleep(15000); // let it upload, for the server-side count

// 5. Upload a photo through Settings > Customise profile.
const settings = await open(c, 'chrome://settings/manageProfile');
const button = await settings(`(() => {
  const find = (root) => { const b = root.querySelector('#uploadPhoto'); if (b) return b;
    for (const el of root.querySelectorAll('*')) if (el.shadowRoot) { const f = find(el.shadowRoot); if (f) return f; } return null; };
  return !!find(document);
})()`);
check(button, 'Customise profile has "Upload a photo"');
const icons = await settings(`new Promise(async (ok) => {
  const canvas = document.createElement('canvas'); canvas.width = 400; canvas.height = 300;
  const g = canvas.getContext('2d'); g.fillStyle = '#c0392b'; g.fillRect(0, 0, 400, 300);
  g.fillStyle = '#2f6f5e'; g.fillRect(150, 100, 100, 100);
  const url = canvas.toDataURL('image/png');
  chrome.send('setProfilePhoto', [url.substring(url.indexOf(',') + 1)]);
  await new Promise((r) => setTimeout(r, 4000));
  const orig = window.cr.webUIResponse;
  window.cr.webUIResponse = (id, ok2, resp) => { if (id === 'bc-icons') { ok(resp); } else { orig(id, ok2, resp); } };
  chrome.send('getAvailableIcons', ['bc-icons']);
})`);
const first = Array.isArray(icons) ? icons[0] : null;
check(first && first.label === 'Your photo' && first.selected && first.isGaiaAvatar,
  `the photo is the first avatar and selected (${first ? first.label + ', selected ' + first.selected : JSON.stringify(icons).slice(0, 200)})`);

// 6. Sign out: back to the form, sync stops.
const out = await page(`new Promise((ok) => {
  const orig = window.browsecraft.onStatus;
  window.browsecraft.onStatus = (s) => { orig(s); ok(s); };
  document.getElementById('sign-out').click();
})`);
check(!out.signedIn, 'sign-out returns to the sign-in form');

await c.send('Browser.close');
await exited;
const state = JSON.parse(readFileSync(join(dir, 'Local State'), 'utf8'));
const entry = state.profile?.info_cache?.Default || {};
log('profile entry:', JSON.stringify({ name: entry.name, is_using_default_name: entry.is_using_default_name,
  gaia_picture_file_name: entry.gaia_picture_file_name, use_gaia_picture: entry.use_gaia_picture,
  last_downloaded_gaia_picture_url_with_size: entry.last_downloaded_gaia_picture_url_with_size }));
check(entry.name === signed.status?.displayName, `the profile is named after the account (${entry.name})`);
check(existsSync(join(dir, 'Default', 'Google Profile Picture.png')), 'the photo is saved in the profile');

log(failed ? `${failed} FAILED` : 'ALL PASSED');
process.exit(failed ? 1 : 0);
