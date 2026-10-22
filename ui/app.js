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
  os: /Mac/i.test(navigator.platform || '') ? 'MacOS' : 'Windows',
  settings: { theme: 'light', autoUpdate: true, prereleases: false, lastCheck: null },
  dataDir: 'data',
  justUpdated: false,
  preview: true,
};

const CURRENT_OS = BOOT.os || (/Mac/i.test(navigator.platform || '') ? 'MacOS' : 'Windows');

function isSupported(a) {
  if (typeof a.supported === 'boolean') return a.supported;
  if (Array.isArray(a.platforms) && a.platforms.length) return a.platforms.includes(CURRENT_OS);
  return true;
}

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

/* The colour schemes app.css defines, the original look first. The preview
   colours mirror each scheme's paper, card, ink and accents. */
const SCHEMES = {
  light: [
    { id: 'walnut', name: 'Walnut & Gilt', paper: '#F4EEE2', card: '#FCF8F0', ink: '#2E2620', accent: '#6B4F35', gilt: '#A8843F' },
    { id: 'sage',   name: 'Sage',          paper: '#EEF1E8', card: '#F8FAF4', ink: '#232A22', accent: '#4F6B45', gilt: '#8C9A3E' },
    { id: 'slate',  name: 'Slate',         paper: '#EEF1F5', card: '#F8FAFC', ink: '#1F2630', accent: '#3C5A80', gilt: '#4F86C6' },
    { id: 'rose',   name: 'Rose',          paper: '#F6EEEE', card: '#FCF7F7', ink: '#2E2224', accent: '#8A4B57', gilt: '#B5707C' },
  ],
  dark: [
    { id: 'gold',   name: 'Charcoal & Gold', paper: '#121212', card: '#1B1B1B', ink: '#ECECEC', accent: '#D2AA5E', gilt: '#E9CB8E' },
    { id: 'ocean',  name: 'Ocean',           paper: '#121212', card: '#1B1B1B', ink: '#ECECEC', accent: '#5AA9F0', gilt: '#A9D3FF' },
    { id: 'forest', name: 'Forest',          paper: '#121212', card: '#1B1B1B', ink: '#ECECEC', accent: '#6DBE7A', gilt: '#B6E3BC' },
    { id: 'plum',   name: 'Plum',            paper: '#121212', card: '#1B1B1B', ink: '#ECECEC', accent: '#B48CE6', gilt: '#DCC6F7' },
  ],
};

const schemeFor = (mode) => {
  const saved = mode === 'dark' ? state.settings.darkScheme : state.settings.lightScheme;
  return SCHEMES[mode].some((s) => s.id === saved) ? saved : SCHEMES[mode][0].id;
};

/* The page shows the scheme for whichever mode is on. */
function applyLook(theme) {
  el.root.dataset.os = CURRENT_OS;
  el.root.dataset.theme = theme;
  el.root.dataset.palette = schemeFor(theme);
}

function renderSchemes() {
  for (const mode of ['light', 'dark']) {
    const current = schemeFor(mode);
    $(`schemes-${mode}`).replaceChildren(...SCHEMES[mode].map((s, i) => {
      const b = document.createElement('button');
      b.className = `scheme${s.id === current ? ' on' : ''}`;
      b.title = `${s.name}${i === 0 ? ' (default)' : ''}`;
      // A tiny window in the scheme's colours: paper, a card, a heading, a button.
      b.innerHTML = `<span class="sw" style="background:${s.paper}">
          <span class="sw-card" style="background:${s.card}">
            <span class="sw-line" style="background:${s.ink}"></span>
            <span class="sw-btn" style="background:${s.accent}"></span>
            <span class="sw-dot" style="background:${s.gilt}"></span>
          </span>
        </span>`;
      const label = document.createElement('span');
      label.className = 'sw-name';
      label.textContent = s.name;
      if (i === 0) {
        const tag = document.createElement('span');
        tag.className = 'sw-default';
        tag.textContent = 'Default';
        label.append(tag);
      }
      b.append(label);
      b.addEventListener('click', () => {
        state.settings[mode === 'dark' ? 'darkScheme' : 'lightScheme'] = s.id;
        applyLook(el.setDark.checked ? 'dark' : 'light');   // immediately, not after the round trip
        renderSchemes();
        post({ cmd: 'settings', [mode === 'dark' ? 'darkScheme' : 'lightScheme']: s.id });
      });
      return b;
    }));
  }
}

function renderSettings() {
  const s = state.settings;
  el.setAuto.checked = !!s.autoUpdate;
  el.setPre.checked = !!s.prereleases;
  el.setDark.checked = s.theme === 'dark';
  applyLook(s.theme === 'dark' ? 'dark' : 'light');
  renderSchemes();
  renderLastCheck();
}

function saveSettings() {
  const theme = el.setDark.checked ? 'dark' : 'light';
  applyLook(theme);   // immediately, not after the round trip
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
      return { pill: 'busy', short: 'Checking for updates…',
               title: 'Checking…', detail: `Asking GitHub for the latest release of ${BOOT.repo}.` };
    case 'upToDate':
      return { pill: 'ok', short: 'Up to date',
               title: 'Up to date', detail: 'This is the newest release on GitHub.' };
    case 'available':
      return { pill: 'gilt', short: `${v} is available`,
               title: `${v} available`, detail: `A newer release is out (${fmtMB(u.release.size)}).` };
    case 'downloading':
      return { pill: 'busy', short: `Downloading ${v} — ${pct(u)}%`,
               title: `Downloading ${v}`,
               detail: u.total ? `${fmtMB(u.done)} of ${fmtMB(u.total)}` : 'Starting the download…' };
    case 'ready':
      return { pill: 'gilt',
               short: state.deferred ? `${v} installs when you close the launcher` : `${v} is ready to install`,
               title: `${v} ready`,
               detail: 'Downloaded and verified. Restart to switch over, or it installs itself when you close the launcher.' };
    case 'error':
      return { pill: 'bad', short: u.message, title: 'Update failed', detail: u.message };
    default:
      return { pill: '', short: `Version ${BOOT.version}`,
               title: 'Not checked', detail: 'The launcher can check GitHub for a newer release.' };
  }
}

/* The status bar's left end only speaks while something is moving: the
   launcher downloading its own update, or an app downloading, installing,
   verifying or repairing. Otherwise the lamp and text stay hidden. */
function activityText(a) {
  const t = a.task;
  const p = t.total ? Math.floor((t.done / t.total) * 100) : 0;
  switch (t.state) {
    case 'downloading':
      return `${a.installed ? 'Updating' : 'Downloading'} ${a.name}` + (t.total ? ` — ${p}%` : '…');
    case 'installing': return `Installing ${a.name}…`;
    case 'verifying':  return `Verifying ${a.name}` + (t.total ? ` — ${t.done} of ${t.total} files` : '…');
    case 'repairing':  return `Repairing ${a.name}` + (t.total ? ` — ${p}%` : '…');
    default:           return null;
  }
}

function renderActivity() {
  const jobs = [];
  const u = state.update;
  if (u.state === 'downloading') jobs.push(`Updating Launcher${u.release ? ` to v${u.release.version}` : ''} — ${pct(u)}%`);
  for (const a of library.apps) {
    const text = activityText(a);
    if (text) jobs.push(text);
  }
  const on = jobs.length > 0;
  el.lamp.className = on ? 'busy' : '';
  el.statusText.textContent = on ? jobs[0] + (jobs.length > 1 ? ` · +${jobs.length - 1} more` : '') : '';
  el.statusText.title = jobs.join('\n');
}

function renderUpdate() {
  const u = state.update;
  const d = describe(u);

  renderActivity();
  el.root.classList.toggle('fuelling', u.state === 'downloading');

  // A shortcut from the status bar to whatever the update wants next.
  const nudge = { available: 'Install', ready: 'Restart', error: 'Details' }[u.state];
  el.statusUpd.classList.toggle('on', !!nudge);
  el.statusUpd.title = nudge ? d.short : 'Update settings';
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

/* ------------------------------------------------------------- library */

/* The host sends the whole library on every change; the cards are rebuilt from
   it each time. Nothing installs on its own: an update only shows a button. */
const library = {
  apps: [],
  armed: null,        // id of the app whose uninstall button is waiting for a second click
  armedTimer: null,
  logOpen: {},        // id -> whether its log drawer is expanded
  logStart: {},       // id -> time of its log's first line, to spot a fresh install
  wraps: {},          // id -> { wrap, cardSig, fullSig } so progress ticks never destroy other cards' buttons
};

const APP_ICONS = BOOT.icons || {};

const ICONS = {
  folder: '<path d="M1.8 4.2c0-.8.6-1.4 1.4-1.4h3l1.5 1.6h5.1c.8 0 1.4.6 1.4 1.4v6.1c0 .8-.6 1.4-1.4 1.4H3.2c-.8 0-1.4-.6-1.4-1.4z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/>',
  notes:  '<path d="M4 1.8h5.6L12.5 4.7v9.5H4z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/><path d="M6 8h4.5M6 10.7h4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>',
  trash:  '<path d="M2.5 4.2h11M6.3 4.2V2.6h3.4v1.6M4 4.2l.7 9.4h6.6l.7-9.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>',
  verify: '<path d="M8 1.6 13.2 3.5v4c0 3.2-2.2 5.6-5.2 6.9-3-1.3-5.2-3.7-5.2-6.9v-4z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/><path d="M5.6 8.1 7.3 9.8 10.5 6.3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>',
};

function h(tag, cls, text) {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
}

function iconButton(icon, title, onClick, extra = '') {
  const b = h('button', `iconbtn ${extra}`);
  b.title = title;
  b.innerHTML = `<svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">${ICONS[icon]}</svg>`;
  b.addEventListener('click', onClick);
  return b;
}

function textButton(label, cls, onClick) {
  const b = h('button', `btn small ${cls}`, label);
  b.addEventListener('click', onClick);
  return b;
}

function summarizeApps(list, noun = 'app') {
  const updates = list.filter((a) => a.updateAvailable).length;
  const installed = list.filter((a) => a.installed).length;
  return `${list.length} ${noun}${list.length === 1 ? '' : 's'} · ${installed} installed`
    + (updates ? ` · ${updates} update${updates > 1 ? 's' : ''}` : '');
}

/* Signature of everything that affects a card's header and buttons (excluding
   progress numbers and log lines), so progress ticks on one app never rebuild
   another card's DOM or interrupt a click in progress. */
function appCardSig(a) {
  const latest = a.latest ? `${a.latest.version}:${a.latest.size}:${a.latest.page}` : '';
  return [
    a.installed || '',
    latest,
    a.checked ? 1 : 0,
    a.updateAvailable ? 1 : 0,
    isSupported(a) ? 1 : 0,
    a.task.state,
    a.task.message || '',
    a.damaged.length,
    a.running ? 1 : 0,
    a.starting ? 1 : 0,
    library.armed === a.id ? 1 : 0,
  ].join('|');
}

function appFullSig(a, cardSig) {
  const first = a.log.length ? a.log[0].t : 0;
  const last = a.log.length ? `${a.log[a.log.length - 1].t}:${a.log[a.log.length - 1].text}` : '';
  const open = first && library.logStart[a.id] !== first ? true : !!library.logOpen[a.id];
  return `${cardSig}|${a.task.done || 0}:${a.task.total || 0}|${a.log.length}:${first}:${last}:${open ? 1 : 0}`;
}

function renderAppList(container, list) {
  const sorted = [...list].sort((a, b) => {
    const instA = a.installed ? 1 : 0;
    const instB = b.installed ? 1 : 0;
    if (instA !== instB) return instB - instA;
    return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
  });

  for (const a of sorted) {
    const cardSig = appCardSig(a);
    const fullSig = appFullSig(a, cardSig);
    const prev = library.wraps[a.id];

    if (prev && prev.fullSig === fullSig) {
      continue;
    }
    if (prev && prev.cardSig === cardSig) {
      // Only progress or log lines changed: update the status bar and log drawer
      // in place, keeping the card and its buttons untouched.
      const statusEl = prev.wrap.querySelector('.app-status');
      if (statusEl) fillAppStatus(statusEl, a);
      const oldLog = prev.wrap.querySelector('.app-log');
      if (a.log.length) {
        const nextLog = appLog(a);
        if (oldLog) oldLog.replaceWith(nextLog);
        else prev.wrap.append(nextLog);
      } else if (oldLog) {
        oldLog.remove();
      }
      prev.fullSig = fullSig;
      continue;
    }

    const wrap = prev ? prev.wrap : h('div', 'app-wrap');
    wrap.replaceChildren(appCard(a));
    if (a.log.length) wrap.append(appLog(a));
    library.wraps[a.id] = { wrap, cardSig, fullSig };
  }

  sorted.forEach((a, i) => {
    const wrap = library.wraps[a.id].wrap;
    if (container.children[i] !== wrap) {
      container.insertBefore(wrap, container.children[i] || null);
    }
  });
  while (container.children.length > sorted.length) {
    container.lastElementChild.remove();
  }
}

/* Each tab's count of available updates, on its chip. */
function setBadge(id, list) {
  const updates = list.filter((a) => a.updateAvailable).length;
  const badge = $(id);
  badge.textContent = updates;
  badge.classList.toggle('on', updates > 0);
}

function renderLibrary() {
  const apps = library.apps;
  const inTab = (cat) => apps.filter((a) => (a.category || 'apps') === cat);
  const mainApps = inTab('apps');
  const ownApps = mainApps.filter((a) => !a.thirdParty);
  const thirdPartyApps = mainApps.filter((a) => a.thirdParty);
  const tempApps = inTab('temporary');
  const games = inTab('games');

  setBadge('lib-badge', mainApps);
  setBadge('temp-badge', tempApps);
  setBadge('games-badge', games);
  $('lib-summary').textContent = summarizeApps(ownApps);
  $('third-party-summary').textContent = summarizeApps(thirdPartyApps);
  $('temp-summary').textContent = tempApps.length ? summarizeApps(tempApps) : '';
  $('games-summary').textContent = games.length ? summarizeApps(games, 'game') : '';
  $('temp-empty').classList.toggle('on', tempApps.length === 0);
  $('games-empty').classList.toggle('on', games.length === 0);
  const checking = apps.some((a) => a.task.state === 'checking');
  document.querySelectorAll('.app-check').forEach((b) => { b.disabled = checking; });

  // Rebuilding replaces the log bodies, so keep where each was scrolled to: a log
  // scrolled to the bottom follows new lines; one scrolled up stays put.
  const scroll = {};
  document.querySelectorAll('.log-body').forEach((b) => {
    scroll[b.dataset.id] = { top: b.scrollTop, end: b.scrollTop + b.clientHeight >= b.scrollHeight - 4 };
  });

  renderAppList($('apps'), ownApps);
  renderAppList($('third-party-apps'), thirdPartyApps);
  renderAppList($('temp-apps'), tempApps);
  renderAppList($('games'), games);

  document.querySelectorAll('.log-body').forEach((b) => {
    const s = scroll[b.dataset.id];
    b.scrollTop = !s || s.end ? b.scrollHeight : s.top;
  });
}

const timeOf = (ms) => new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });

/* The drawer under a card: everything the launcher has done to that app this
   session. A new install or update opens it; the arrow folds it away. */
function appLog(a) {
  const first = a.log[0].t;
  if (library.logStart[a.id] !== first) {
    library.logStart[a.id] = first;
    library.logOpen[a.id] = true;
  }
  const open = !!library.logOpen[a.id];
  const last = a.log[a.log.length - 1];

  const drawer = h('div', `app-log${open ? ' open' : ''}`);
  const toggle = h('button', 'log-toggle');
  toggle.title = open ? 'Hide the log' : 'Show the log';
  toggle.innerHTML = '<svg class="chev" width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">'
    + '<path d="M2 3.5 5 6.5 8 3.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  toggle.append(h('span', 'log-label', 'Log'));
  toggle.append(h('span', `log-last ${last.kind}`, open ? `${a.log.length} lines` : last.text));
  toggle.addEventListener('click', () => {
    library.logOpen[a.id] = !open;
    renderLibrary();
  });
  drawer.append(toggle);

  if (open) {
    const body = h('div', 'log-body');
    body.dataset.id = a.id;
    for (const line of a.log) {
      const row = h('div', `log-line ${line.kind}`);
      row.append(h('span', 'log-t', timeOf(line.t)), h('span', 'log-text', line.text));
      body.append(row);
    }
    drawer.append(body);
  }
  return drawer;
}

function appCard(a) {
  const t = a.task;
  const busy = ['checking', 'downloading', 'installing', 'verifying', 'repairing', 'stopping'].includes(t.state);
  const damaged = a.damaged.length;
  const latest = a.latest;
  const supported = isSupported(a);
  const platformsLabel = Array.isArray(a.platforms) && a.platforms.length ? a.platforms.join(' / ') : '';
  const icon = a.icon || APP_ICONS[a.id];
  const card = h('div', `app${a.updateAvailable ? ' has-update' : ''}${supported ? '' : ' unsupported'}`);

  const tile = h('div', `app-tile${a.installed ? '' : ' dim'}${icon ? ' has-icon' : ''}`);
  if (icon) {
    const img = h('img', 'app-icon');
    img.src = icon;
    img.alt = '';
    img.draggable = false;
    tile.append(img);
  } else {
    tile.textContent = a.name[0];
  }
  card.append(tile);

  // Name, version line, tagline.
  const main = h('div', 'app-main');
  const title = h('div', 'app-title');
  let live = null;
  title.append(h('span', 'app-name', a.name));
  const ver = h('span', 'app-ver');
  if (!supported && !a.installed) {
    ver.append(platformsLabel ? `${platformsLabel} only` : `Not available on ${CURRENT_OS}`);
  } else if (a.installed) {
    ver.append(`v${a.installed}`);
    if (a.updateAvailable) {
      ver.append(' → ');
      ver.append(h('span', 'new', `v${latest.version}`));
    } else if (a.checked && latest) {
      ver.append(' · up to date');
    }
    if (a.running) {
      live = h('span', 'running');
      live.append(h('span', 'dot'), 'running');
    }
  } else if (latest) {
    ver.append(`v${latest.version} available` + (latest.size ? ` · ${fmtMB(latest.size)}` : ''));
  } else if (a.checked && t.state !== 'error') {
    ver.append('No release published yet');
  }
  title.append(ver);
  if (live) title.append(live);
  main.append(title, h('div', 'app-tagline', a.tagline));
  card.append(main);

  // Buttons.
  const actions = h('div', 'app-actions');
  if (a.installed) {
    if (damaged) {
      const fix = textButton(`Repair ${damaged} file${damaged > 1 ? 's' : ''}`, 'gilt', () => post({ cmd: 'appRepair', id: a.id }));
      fix.title = 'Try the repair again: download just the damaged files';
      fix.disabled = busy;
      actions.append(fix);
    }
    if (a.updateAvailable && supported) {
      const up = textButton(`Update to v${latest.version}`, 'gilt', () => post({ cmd: 'appInstall', id: a.id }));
      up.disabled = busy;
      actions.append(up);
    }
    // While it runs, Launch becomes a red Stop, so the launcher never starts a second copy.
    let launch;
    if (t.state === 'stopping') {
      launch = textButton('Stopping…', 'danger', () => {});
      launch.disabled = true;
    } else if (a.running) {
      launch = textButton('Stop', 'danger', () => post({ cmd: 'appStop', id: a.id }));
      launch.title = `Ask ${a.name} to quit`;
      launch.disabled = busy;
    } else if (a.starting) {
      launch = textButton('Starting…', 'primary', () => {});
      launch.disabled = true;
    } else {
      launch = textButton('Launch', 'primary', () => post({ cmd: 'appLaunch', id: a.id }));
      launch.disabled = busy || !supported;
    }
    actions.append(launch);
    actions.append(iconButton('folder', 'Open its folder', () => post({ cmd: 'appFolder', id: a.id })));
    const check = iconButton('verify', 'Verify files: check every file against its SHA-256 and re-download any that are damaged',
      () => post({ cmd: 'appVerify', id: a.id }));
    check.disabled = busy;
    actions.append(check);
    if (latest) {
      actions.append(iconButton('notes', 'Release notes on GitHub', () => post({ cmd: 'openUrl', url: latest.page })));
    }
    const armed = library.armed === a.id;
    const del = iconButton('trash', armed ? 'Click again to delete it and its settings' : 'Uninstall',
      () => uninstallClick(a.id), armed ? 'danger armed' : 'danger');
    del.disabled = busy;
    actions.append(del);
  } else if (!supported) {
    const label = platformsLabel ? `${platformsLabel} only` : `Not on ${CURRENT_OS}`;
    const install = textButton(label, 'primary', () => {});
    install.disabled = true;
    install.title = `${a.name} does not have a ${CURRENT_OS} tag and cannot be downloaded on ${CURRENT_OS}.`;
    actions.append(install);
  } else {
    const label = !latest && t.state === 'checking' ? 'Checking…' : 'Install';
    const install = textButton(label, 'primary', () => post({ cmd: 'appInstall', id: a.id }));
    install.disabled = busy || !latest;
    actions.append(install);
  }
  card.append(actions);

  if (a.platforms && a.platforms.length) {
    const osRow = h('div', 'app-platforms');
    for (const os of a.platforms) {
      osRow.append(h('span', `os-tag${os === CURRENT_OS ? ' match' : ''}`, os));
    }
    card.append(osRow);
  }

  // What it's doing right now, if anything.
  const status = h('div', 'app-status');
  fillAppStatus(status, a);
  card.append(status);
  return card;
}

function fillAppStatus(status, a) {
  const t = a.task;
  const damaged = a.damaged.length;
  status.className = 'app-status';
  status.replaceChildren();
  if (t.state === 'downloading') {
    const pctDone = t.total ? Math.floor((t.done / t.total) * 100) : 0;
    const bar = h('div', 'progress');
    const fill = h('div', 'bar');
    fill.style.width = `${pctDone}%`;
    bar.append(fill);
    status.append(bar, t.total ? `Downloading ${pctDone}% of ${fmtMB(t.total)}` : 'Downloading…');
  } else if (t.state === 'installing') {
    status.append('Checking every file and installing…');
  } else if (t.state === 'verifying' || t.state === 'repairing') {
    const frac = t.total ? t.done / t.total : 0;
    const bar = h('div', 'progress');
    const fill = h('div', 'bar');
    fill.style.width = `${Math.floor(frac * 100)}%`;
    bar.append(fill);
    status.append(bar, t.state === 'verifying'
      ? (t.total ? `Verifying ${t.done} of ${t.total} files` : 'Verifying…')
      : (t.total ? `Repairing ${Math.floor(frac * 100)}% of ${fmtMB(t.total)}` : 'Reading the package index…'));
  } else if (damaged && t.state !== 'error') {
    status.classList.add('bad');
    status.append(`${damaged} file${damaged > 1 ? 's are' : ' is'} damaged or missing and ${damaged > 1 ? "weren't" : "wasn't"} repaired. Press Repair to try again.`);
  } else if (t.state === 'checking' && a.installed) {
    status.append('Checking for updates…');
  } else if (t.state === 'error') {
    status.classList.add('bad');
    status.append(t.message);
  } else if (library.armed === a.id) {
    status.classList.add('bad');
    status.append(`Click the bin again to delete ${a.name} and its settings.`);
  }
}

/* Uninstalling deletes the app's settings too, so it takes a second click. */
function uninstallClick(id) {
  clearTimeout(library.armedTimer);
  if (library.armed === id) {
    library.armed = null;
    post({ cmd: 'appUninstall', id });
  } else {
    library.armed = id;
    library.armedTimer = setTimeout(() => { library.armed = null; renderLibrary(); }, 4000);
  }
  renderLibrary();
}

document.querySelectorAll('.app-check').forEach((b) =>
  b.addEventListener('click', () => post({ cmd: 'appCheck' })));

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

/* A check picked from the menu bar says how it went once it's done, the way a
   Mac app's "Check for Updates…" does, rather than just flickering the page. */
let menuCheck = null;

function reportMenuCheck() {
  // Never cover a launch-time update toast that is still asking something.
  const busyToast = el.toast.classList.contains('on') && !el.toast.classList.contains('done');
  if (menuCheck === 'launcher') {
    if (!['upToDate', 'available', 'ready', 'error'].includes(state.update.state)) return;
    menuCheck = null;
    const d = describe(state.update);
    if (!busyToast) flashToast(d.title, d.detail);
  } else if (menuCheck === 'apps') {
    if (library.apps.some((a) => a.task.state === 'checking')) return;
    menuCheck = null;
    const n = library.apps.filter((a) => a.updateAvailable).length;
    if (busyToast) return;
    if (n) flashToast(`${n} app update${n > 1 ? 's' : ''} available`, 'Press Update on each one to install it.');
    else flashToast('Your apps are up to date', 'Everything installed is on its newest release.');
  }
}

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
      reportMenuCheck();
      break;
    case 'window':
      el.root.classList.toggle('maximized', !!msg.maximized);
      break;
    case 'menuCheck':
      showPage(msg.page);
      menuCheck = msg.check;
      break;
    case 'apps':
      library.apps = msg.apps;
      renderLibrary();
      renderActivity();
      reportMenuCheck();
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
  if ((e.ctrlKey || e.metaKey) && e.key === ',') { e.preventDefault(); showPage('settings'); }
  else if (e.key === 'Escape' && state.page !== 'library') showPage('library');
});

/* ---------------------------------------------------------------- boot */

el.version.textContent = `v${BOOT.version}`;
el.verCurrent.textContent = BOOT.version;
renderSettings();
renderUpdate();

if (BOOT.justUpdated) flashToast(`Updated to v${BOOT.version}`, 'You’re on the newest release.');

// Tells the host the page is painted: it shows the window only now, so there is
// no blank frame, and starts the launch-time update check. WebKit on macOS never
// runs animation frames for a window that isn't on screen yet, so a timer stands
// in there; whichever fires first wins.
let readySent = false;
const ready = () => {
  if (readySent) return;
  readySent = true;
  post({ cmd: 'ready' });
};
requestAnimationFrame(ready);
setTimeout(ready, 50);
