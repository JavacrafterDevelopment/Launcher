/* ============================================================================
   Launcher front-end.

   The Rust host owns all state: settings and the self-update. The page sends
   commands through window.ipc and the host answers by calling window.onHost with
   whole snapshots, so there is never anything here to fall out of sync.
   ========================================================================== */

const BOOT = window.BOOT || {
  // Opened as a plain file (design work in a browser): stand in for the host.
  version: '0.1.0',
  repo: 'JavacrafterDevelopment/Launcher',
  settings: { theme: 'light', autoUpdate: true, prereleases: false, lastCheck: null },
  dataDir: 'data',
  justUpdated: false,
  preview: true,
};

const post = (msg) => {
  if (window.ipc) window.ipc.postMessage(JSON.stringify(msg));
  else console.log('[host]', msg);
};

const $ = (id) => document.getElementById(id);

const el = {
  root:        document.documentElement,
  tabs:        document.querySelectorAll('#tabs .chip'),
  pages:       document.querySelectorAll('.page'),
  lamp:        $('lamp'),
  statusText:  $('statusText'),
  statusUpd:   $('status-update'),
  statusUpdT:  $('status-update-text'),
  version:     $('version'),
  verCurrent:  $('ver-current'),
  repoLink:    $('repo-link'),
  pill:        $('upd-pill'),
  pillText:    $('upd-pill-text'),
  detail:      $('upd-detail'),
  progress:    $('upd-progress'),
  bar:         $('upd-bar'),
  notes:       $('upd-notes'),
  notesTitle:  $('upd-notes-title'),
  notesLink:   $('upd-notes-link'),
  notesBody:   $('upd-notes-body'),
  btnCheck:    $('btn-check'),
  btnDownload: $('btn-download'),
  btnRestart:  $('btn-restart'),
  last:        $('upd-last'),
  setAuto:     $('set-auto'),
  setPre:      $('set-pre'),
  setDark:     $('set-dark'),
  btnData:     $('btn-data'),
  toast:       $('toast'),
  toastTitle:  $('toast-title'),
  toastDesc:   $('toast-desc'),
  toastProg:   $('toast-progress'),
  toastBar:    $('toast-bar'),
  toastActs:   $('toast-actions'),
  toastLater:  $('toast-later'),
  toastNow:    $('toast-now'),
};

const state = {
  settings: BOOT.settings,
  update:   { state: 'idle' },
  auto:     false,
  page:     'library',
  countdown: null,     // interval id while the auto-restart counts down
  deferred:  false,    // the user pressed "Later" on this run's update
};

/* -------------------------------------------------------------- pages */

function showPage(name) {
  state.page = name;
  el.tabs.forEach((t) => t.classList.toggle('on', t.dataset.page === name));
  el.pages.forEach((p) => p.classList.toggle('on', p.id === `page-${name}`));
}

el.tabs.forEach((t) => t.addEventListener('click', () => showPage(t.dataset.page)));
document.querySelectorAll('[data-goto]').forEach((b) =>
  b.addEventListener('click', () => showPage(b.dataset.goto)));

/* ----------------------------------------------------------- settings */

function renderSettings() {
  const s = state.settings;
  el.setAuto.checked = !!s.autoUpdate;
  el.setPre.checked = !!s.prereleases;
  el.setDark.checked = s.theme === 'dark';
  el.root.dataset.theme = s.theme === 'dark' ? 'dark' : 'light';
  renderLastCheck();
}

function saveSettings() {
  const theme = el.setDark.checked ? 'dark' : 'light';
  el.root.dataset.theme = theme;   // immediately, not after the round trip
  post({
    cmd: 'settings',
    theme,
    autoUpdate: el.setAuto.checked,
    prereleases: el.setPre.checked,
  });
}

[el.setAuto, el.setPre, el.setDark].forEach((c) => c.addEventListener('change', saveSettings));

el.btnData.addEventListener('click', () => post({ cmd: 'openDataDir' }));

function renderLastCheck() {
  const t = state.settings.lastCheck;
  el.last.textContent = t ? `Checked ${ago(t)}` : '';
}

function ago(secs) {
  const d = Math.max(0, Date.now() / 1000 - secs);
  if (d < 45) return 'just now';
  if (d < 3600) return `${Math.round(d / 60)} min ago`;
  if (d < 86400) return `${Math.round(d / 3600)} h ago`;
  const days = Math.round(d / 86400);
  return days === 1 ? 'yesterday' : `${days} days ago`;
}

setInterval(renderLastCheck, 30000);

/* -------------------------------------------------------------- update */

const fmtMB = (b) => `${(b / 1048576).toFixed(b < 10485760 ? 1 : 0)} MB`;
const pct = (u) => (u.total > 0 ? Math.floor((u.done / u.total) * 100) : 0);

/* One description per state, shared by the status bar, the settings panel and
   the toast, so they can never disagree. */
function describe(u) {
  const v = u.release ? `v${u.release.version}` : '';
  switch (u.state) {
    case 'checking':
      return { lamp: 'busy', pill: 'busy', short: 'Checking for updates…',
               title: 'Checking…', detail: `Asking GitHub for the latest release of ${BOOT.repo}.` };
    case 'upToDate':
      return { lamp: 'ok', pill: 'ok', short: 'Up to date',
               title: 'Up to date', detail: 'This is the newest release on GitHub.' };
    case 'available':
      return { lamp: 'gilt', pill: 'gilt', short: `${v} is available`,
               title: `${v} available`, detail: `A newer release is out (${fmtMB(u.release.size)}).` };
    case 'downloading':
      return { lamp: 'busy', pill: 'busy', short: `Downloading ${v} — ${pct(u)}%`,
               title: `Downloading ${v}`,
               detail: u.total ? `${fmtMB(u.done)} of ${fmtMB(u.total)}` : 'Starting the download…' };
    case 'ready':
      return { lamp: 'gilt', pill: 'gilt',
               short: state.deferred ? `${v} installs when you close the launcher` : `${v} is ready to install`,
               title: `${v} ready`,
               detail: 'Downloaded and verified. Restart to switch over, or it installs itself when you close the launcher.' };
    case 'error':
      return { lamp: 'bad', pill: 'bad', short: u.message, title: 'Update failed', detail: u.message };
    default:
      return { lamp: '', pill: '', short: `Version ${BOOT.version}`,
               title: 'Not checked', detail: 'The launcher can check GitHub for a newer release.' };
  }
}

function renderUpdate() {
  const u = state.update;
  const d = describe(u);

  el.lamp.className = d.lamp;
  el.statusText.textContent = d.short;
  el.root.classList.toggle('fuelling', u.state === 'downloading');

  // A shortcut from the status bar to whatever the update wants next.
  const nudge = { available: 'Install', ready: 'Restart', error: 'Details' }[u.state];
  el.statusUpd.classList.toggle('on', !!nudge);
  if (nudge) el.statusUpdT.textContent = nudge;

  el.pill.className = `pill ${d.pill}`;
  el.pillText.textContent = d.title;
  el.detail.textContent = d.detail;

  const downloading = u.state === 'downloading';
  el.progress.classList.toggle('on', downloading);
  el.bar.style.width = downloading ? `${pct(u)}%` : '0';

  const busy = u.state === 'checking' || downloading;
  el.btnCheck.disabled = busy || u.state === 'ready';
  el.btnCheck.classList.toggle('spin', u.state === 'checking');
  el.btnDownload.classList.toggle('on', u.state === 'available');
  el.btnRestart.classList.toggle('on', u.state === 'ready');

  renderNotes(u.release);
  renderToast();
}

function renderNotes(rel) {
  el.notes.classList.toggle('on', !!rel);
  if (!rel) return;
  el.notesTitle.textContent = rel.name + (rel.prerelease ? ' · pre-release' : '');
  el.notesLink.dataset.url = rel.page;
  const body = (rel.notes || '').trim();
  el.notesBody.replaceChildren();
  if (body) {
    el.notesBody.textContent = body;
  } else {
    const m = document.createElement('span');
    m.className = 'muted';
    m.textContent = 'No release notes.';
    el.notesBody.append(m);
  }
}

el.btnCheck.addEventListener('click', () => post({ cmd: 'checkUpdate' }));
el.btnDownload.addEventListener('click', () => post({ cmd: 'downloadUpdate' }));
el.btnRestart.addEventListener('click', () => post({ cmd: 'restartNow' }));
el.statusUpd.addEventListener('click', () => {
  if (state.update.state === 'available') post({ cmd: 'downloadUpdate' });
  else if (state.update.state === 'ready') post({ cmd: 'restartNow' });
  showPage('settings');
});

function openRepo(e) {
  e.preventDefault();
  const url = e.currentTarget.dataset.url;
  if (url) post({ cmd: 'openUrl', url });
}

el.repoLink.textContent = `github.com/${BOOT.repo}`;
el.repoLink.dataset.url = `https://github.com/${BOOT.repo}/releases`;
el.repoLink.addEventListener('click', openRepo);
el.notesLink.addEventListener('click', openRepo);

/* --------------------------------------------------------------- toast */

/* Only updates that started on their own at launch get a toast — if you pressed
   the button in settings you are already looking at the progress. */
function renderToast() {
  const u = state.update;
  const show = state.auto && !state.deferred && (u.state === 'downloading' || u.state === 'ready');

  if (!show) {
    stopCountdown();
    if (!el.toast.classList.contains('done')) el.toast.classList.remove('on');
    return;
  }

  el.toast.classList.remove('done');
  el.toast.classList.add('on');
  const v = `v${u.release.version}`;

  if (u.state === 'downloading') {
    el.toastTitle.textContent = `Updating to ${v}`;
    el.toastDesc.textContent = u.total ? `Downloading — ${pct(u)}% of ${fmtMB(u.total)}` : 'Starting the download…';
    el.toastProg.classList.add('on');
    el.toastBar.style.width = `${pct(u)}%`;
    el.toastActs.classList.remove('on');
  } else {
    el.toastTitle.textContent = `${v} is ready`;
    el.toastProg.classList.remove('on');
    el.toastActs.classList.add('on');
    startCountdown();
  }
}

function startCountdown() {
  if (state.countdown) return;
  let left = 5;
  const tick = () => {
    el.toastDesc.textContent = `Restarting in ${left}…`;
    if (left-- <= 0) {
      stopCountdown();
      post({ cmd: 'restartNow' });
    }
  };
  tick();
  state.countdown = setInterval(tick, 1000);
}

function stopCountdown() {
  if (state.countdown) clearInterval(state.countdown);
  state.countdown = null;
}

el.toastNow.addEventListener('click', () => { stopCountdown(); post({ cmd: 'restartNow' }); });
el.toastLater.addEventListener('click', () => {
  state.deferred = true;
  renderUpdate();
});

function flashToast(title, desc) {
  el.toast.classList.add('on', 'done');
  el.toastTitle.textContent = title;
  el.toastDesc.textContent = desc;
  el.toastProg.classList.remove('on');
  el.toastActs.classList.remove('on');
  setTimeout(() => el.toast.classList.remove('on', 'done'), 4200);
}

/* ---------------------------------------------------------------- host */

window.onHost = (msg) => {
  switch (msg.type) {
    case 'settings':
      state.settings = msg.settings;
      renderSettings();
      break;
    case 'update':
      state.update = msg.update;
      state.auto = !!msg.auto;
      renderUpdate();
      break;
    case 'window':
      el.root.classList.toggle('maximized', !!msg.maximized);
      break;
  }
};

/* -------------------------------------------------------------- window */

/* The whole masthead is draggable except the controls sitting on it. */
document.getElementById('top').addEventListener('mousedown', (e) => {
  if (e.button !== 0 || e.detail > 1) return;
  if (e.target.closest('button, input, a, nav')) return;
  post({ cmd: 'window', action: 'drag' });
});

document.getElementById('top').addEventListener('dblclick', (e) => {
  if (e.target.closest('button, input, a, nav')) return;
  post({ cmd: 'window', action: 'maximize' });
});

document.querySelectorAll('.winbtn').forEach((b) =>
  b.addEventListener('click', () => post({ cmd: 'window', action: b.dataset.win })));

document.querySelectorAll('.grip').forEach((grip) => {
  grip.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    post({ cmd: 'resize', dir: grip.dataset.dir });
  });
});

/* The browser's own menu (Back, Reload, Inspect) has no place in an app window. */
document.addEventListener('contextmenu', (e) => {
  if (!e.target.closest('input[type=text], .notes-body')) e.preventDefault();
});

document.addEventListener('keydown', (e) => {
  if (e.ctrlKey && e.key === ',') { e.preventDefault(); showPage('settings'); }
  else if (e.key === 'Escape' && state.page !== 'library') showPage('library');
});

/* ---------------------------------------------------------------- boot */

el.version.textContent = `v${BOOT.version}`;
el.verCurrent.textContent = BOOT.version;
renderSettings();
renderUpdate();

if (BOOT.justUpdated) flashToast(`Updated to v${BOOT.version}`, 'You’re on the newest release.');

// Tells the host the page is painted: it shows the window only now, so there is
// no blank frame, and starts the launch-time update check.
requestAnimationFrame(() => post({ cmd: 'ready' }));
