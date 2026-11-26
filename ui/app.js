/* ============================================================================
   Launcher front-end.

   The Rust host owns all state: settings, self-update, and the app library.
   The page sends commands through window.ipc and the host answers by calling
   window.onHost with whole snapshots.
   ========================================================================== */

const BOOT = window.BOOT || {
  // Opened as a plain file (browser preview): stand in for the host.
  version: '0.3.17',
  repo: 'JavacrafterDevelopment/Launcher',
  githubOrgUrl: 'https://github.com/JavacrafterDevelopment',
  os: /Mac/i.test(navigator.platform || '') ? 'MacOS' : 'Windows',
  settings: {
    theme: 'light',
    look: 'modern',
    checkEvery: 'auto',
    autoUpdate: true,
    simultaneousDownloads: 1,
    notifyAppUpdates: true,
    prereleases: false,
    installDir: null,
    lastCheck: Math.floor(Date.now() / 1000) - 180,
    lightScheme: 'walnut',
    darkScheme: 'gold',
    view: 'grid',
    sort: 'name-asc',
    pinned: [],
  },
  dataDir: 'data',
  defaultAppsDir: 'portableApplications',
  startupCheck: false,
  justUpdated: false,
  icons: {},
  preview: true,
};

const CURRENT_OS = BOOT.os || (/Mac/i.test(navigator.platform || '') ? 'MacOS' : 'Windows');
const GITHUB_ORG_URL = BOOT.githubOrgUrl || 'https://github.com/JavacrafterDevelopment';

const post = (msg) => {
  if (window.ipc) window.ipc.postMessage(JSON.stringify(msg));
  else console.log('[host]', msg);
};

const $ = (id) => document.getElementById(id);

function isSupported(a) {
  if (typeof a.supported === 'boolean') return a.supported;
  if (Array.isArray(a.platforms) && a.platforms.length) return a.platforms.includes(CURRENT_OS);
  return true;
}

const el = {
  root:            document.documentElement,
  splash:          $('splash'),
  splashBar:       $('splash-bar'),
  splashStatus:    $('splash-status'),
  splashSkip:      $('splash-skip'),
  tabs:            document.querySelectorAll('#tabs .cell'),
  legacyTabs:      document.querySelectorAll('#legacy-tabs .chip'),
  btnBell:         $('btn-bell'),
  bellCount:       $('bell-count'),
  bellPopover:     $('bell-popover'),
  bellBody:        $('bell-body'),
  bellClose:       $('bell-close'),
  btnSettings:     $('btn-settings'),
  pageCatalog:     $('page-catalog'),
  pageSettings:    $('page-settings'),
  settingsBack:    $('settings-backdrop'),
  btnSettingsDone: $('btn-settings-done'),
  btnQuit:         $('btn-quit'),
  catalogContent:  $('catalog-content'),
  catalogSummary:  $('catalog-summary'),
  btnRefresh:      $('btn-refresh'),
  btnUpdateAll:    $('btn-update-all'),
  sortWrap:        $('sort-wrap'),
  sortSelect:      $('sort-select'),
  searchInput:     $('search-input'),
  searchClear:     $('search-clear'),
  vtGrid:          $('vt-grid'),
  vtTable:         $('vt-table'),
  banner:          $('launcher-banner'),
  bannerText:      $('banner-text'),
  bannerProgress:  $('banner-progress'),
  bannerProgLabel: $('banner-prog-label'),
  bannerBar:       $('banner-bar'),
  bannerAction:    $('banner-action'),
  bannerDismiss:   $('banner-dismiss'),
  lamp:            $('lamp'),
  statusText:      $('statusText'),
  statusUpd:       $('status-update'),
  statusUpdT:      $('status-update-text'),
  btnGithubOrg:    $('btn-github-org'),
  repoLink:        $('repo-link'),
  pill:            $('upd-pill'),
  pillText:        $('upd-pill-text'),
  detail:          $('upd-detail'),
  progress:        $('upd-progress'),
  bar:             $('upd-bar'),
  notes:           $('upd-notes'),
  notesTitle:      $('upd-notes-title'),
  notesLink:       $('upd-notes-link'),
  notesBody:       $('upd-notes-body'),
  btnCheck:        $('btn-check'),
  btnDownload:     $('btn-download'),
  btnRestart:      $('btn-restart'),
  last:            $('upd-last'),
  setCheckEvery:   $('set-check-every'),
  setAuto:         $('set-auto'),
  setSimDl:        $('set-sim-dl'),
  setNotify:       $('set-notify'),
  setPre:          $('set-pre'),
  installDisplay:  $('install-folder-display'),
  btnBrowseInst:   $('btn-browse-install'),
  btnDefaultInst:  $('btn-default-install'),
  btnData:         $('btn-data'),
  lookButtons:     document.querySelectorAll('#look-group .cell'),
  themeButtons:    document.querySelectorAll('#theme-group .cell'),
  cardMenu:        $('card-menu'),
  toast:           $('toast'),
  toastTitle:      $('toast-title'),
  toastDesc:       $('toast-desc'),
  toastProg:       $('toast-progress'),
  toastBar:        $('toast-bar'),
  toastActs:       $('toast-actions'),
  toastLater:      $('toast-later'),
  toastNow:        $('toast-now'),
};

const state = {
  settings:         Object.assign({
    theme: 'light',
    look: 'modern',
    checkEvery: 'auto',
    autoUpdate: true,
    simultaneousDownloads: 1,
    notifyAppUpdates: true,
    prereleases: false,
    installDir: null,
    lastCheck: null,
    lightScheme: 'walnut',
    darkScheme: 'gold',
    view: 'grid',
    sort: 'name-asc',
    pinned: [],
  }, BOOT.settings || {}),
  update:           { state: 'idle' },
  updateMode:       'manual',
  tab:              'library',      // 'library' | 'temporary' | 'games'
  settingsOpen:     false,
  search:           '',
  bannerDismissed:  false,
  deferred:         false,          // user deferred a launcher update to install on exit
  bgToastDismissed: null,           // launcher version dismissed in bottom-right popup
  // Discord-style startup splash state
  splashActive:     !!BOOT.startupCheck,
  splashStart:      Date.now(),
  splashTimeout:    null,
  splashRestarting: false,
  // Background 5-minute app update notification tracking
  initialAppsSettled: false,
  bgCheckPending:   false,
  notifiedAppVers:  new Set(),
  // Active toast context
  toastMode:        null,           // 'launcher-bg' | 'launcher-dl' | 'launcher-ready' | 'app-bg' | 'flash'
  toastTimer:       null,
  toastTargetTab:   null,
  // Card overflow menu
  openMenuId:       null,
};

/* ----------------------------------------------------------- helpers */

const fmtMB = (b) => `${(b / 1048576).toFixed(b < 10485760 ? 1 : 0)} MB`;
const pct = (u) => (u.total > 0 ? Math.floor((u.done / u.total) * 100) : 0);

function h(tag, cls, text) {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
}

function ago(secs) {
  const d = Math.max(0, Date.now() / 1000 - secs);
  if (d < 45) return 'just now';
  if (d < 3600) return `${Math.round(d / 60)} min ago`;
  if (d < 86400) return `${Math.round(d / 3600)} h ago`;
  const days = Math.round(d / 86400);
  return days === 1 ? 'yesterday' : `${days} days ago`;
}

function fmtDate(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
}

const timeOf = (ms) => new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });

/* --------------------------------------------------- startup splash */

function startSplash() {
  if (!state.splashActive) return;
  el.splash.classList.add('on');
  el.splashStatus.textContent = 'Checking for updates…';
  el.splashBar.classList.add('indeterminate');
  el.splashBar.style.width = '';
  el.splashSkip.classList.remove('on');

  // Keep startup snappy: if the check takes > 2.5s (slow/offline network), let the user in
  // and turn any late discovery into a bottom-right popup instead of rebooting mid-use.
  clearTimeout(state.splashTimeout);
  state.splashTimeout = setTimeout(() => {
    if (state.splashActive && state.update.state !== 'downloading' && state.update.state !== 'ready') {
      closeSplash(true);
    }
  }, 2500);
}

function closeSplash(notifyHost = false) {
  if (!state.splashActive) return;
  state.splashActive = false;
  clearTimeout(state.splashTimeout);
  if (notifyHost) post({ cmd: 'dismissSplash' });
  el.splash.classList.add('fading');
  setTimeout(() => {
    el.splash.classList.remove('on', 'fading');
  }, 260);
}

function updateSplash() {
  if (!state.splashActive) return;
  const u = state.update;
  if (state.updateMode !== 'startup' && u.state !== 'idle' && u.state !== 'checking') {
    closeSplash(false);
    return;
  }
  if (u.state === 'checking' || u.state === 'idle') {
    el.splashStatus.textContent = 'Checking for updates…';
    el.splashBar.classList.add('indeterminate');
    el.splashSkip.classList.remove('on');
  } else if (u.state === 'available') {
    clearTimeout(state.splashTimeout);
    el.splashStatus.textContent = 'Update found — starting download…';
    el.splashBar.classList.add('indeterminate');
    el.splashSkip.classList.add('on');
  } else if (u.state === 'downloading') {
    clearTimeout(state.splashTimeout);
    const p = pct(u);
    el.splashStatus.textContent = u.total
      ? `Downloading update — ${p}% (${fmtMB(u.done)} of ${fmtMB(u.total)})`
      : 'Downloading update…';
    el.splashBar.classList.remove('indeterminate');
    el.splashBar.style.width = `${p}%`;
    el.splashSkip.classList.add('on');
  } else if (u.state === 'ready') {
    clearTimeout(state.splashTimeout);
    el.splashStatus.textContent = 'Installing update & restarting…';
    el.splashBar.classList.remove('indeterminate');
    el.splashBar.style.width = '100%';
    el.splashSkip.classList.remove('on');
    if (!state.splashRestarting && !state.deferred) {
      state.splashRestarting = true;
      setTimeout(() => post({ cmd: 'restartNow' }), 350);
    }
  } else if (u.state === 'upToDate' || u.state === 'error') {
    clearTimeout(state.splashTimeout);
    const elapsed = Date.now() - state.splashStart;
    const wait = Math.max(0, 450 - elapsed);
    setTimeout(() => closeSplash(false), wait);
  }
}

el.splashSkip.addEventListener('click', () => {
  state.deferred = true;
  closeSplash(true);
  renderUpdate();
});

/* ----------------------------------------------------- tabs & pages */

function setTab(name) {
  if (name === 'settings') {
    openSettings();
    return;
  }
  state.tab = name;
  if (state.settings.look === 'legacy') {
    state.settingsOpen = false;
  }
  closeCardMenu();
  syncNav();
  renderLibrary();
}

function openSettings() {
  state.settingsOpen = true;
  closeCardMenu();
  closeBell();
  syncNav();
}

function closeSettings() {
  state.settingsOpen = false;
  syncNav();
}

function syncNav() {
  const isLegacy = state.settings.look === 'legacy';
  el.tabs.forEach((b) => b.classList.toggle('on', b.dataset.tab === state.tab));
  el.legacyTabs.forEach((b) => {
    const active = state.settingsOpen ? b.dataset.tab === 'settings' : b.dataset.tab === state.tab;
    b.classList.toggle('on', active);
  });
  el.btnSettings.classList.toggle('on', state.settingsOpen);

  if (isLegacy) {
    el.pageCatalog.classList.toggle('on', !state.settingsOpen);
    el.pageSettings.classList.toggle('on', state.settingsOpen);
  } else {
    el.pageCatalog.classList.add('on');
    el.pageSettings.classList.toggle('on', state.settingsOpen);
  }
}

el.tabs.forEach((b) => b.addEventListener('click', () => setTab(b.dataset.tab)));
el.legacyTabs.forEach((b) => b.addEventListener('click', () => setTab(b.dataset.tab)));
el.btnSettings.addEventListener('click', () => (state.settingsOpen ? closeSettings() : openSettings()));
el.btnSettingsDone.addEventListener('click', closeSettings);
el.settingsBack.addEventListener('click', closeSettings);
el.btnQuit.addEventListener('click', () => post({ cmd: 'window', action: 'close' }));

/* --------------------------------------------------------- settings */

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

const darkMedia = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)');

function effectiveTheme(pref) {
  if (pref === 'dark') return 'dark';
  if (pref === 'light') return 'light';
  return darkMedia && darkMedia.matches ? 'dark' : 'light';
}

const schemeFor = (mode) => {
  const saved = mode === 'dark' ? state.settings.darkScheme : state.settings.lightScheme;
  return SCHEMES[mode].some((s) => s.id === saved) ? saved : SCHEMES[mode][0].id;
};

function applyLook() {
  const look = state.settings.look === 'legacy' ? 'legacy' : 'modern';
  const resolved = effectiveTheme(state.settings.theme);
  el.root.dataset.os = CURRENT_OS;
  el.root.dataset.look = look;
  el.root.dataset.theme = resolved;
  el.root.dataset.palette = schemeFor(resolved);
  syncNav();
}

if (darkMedia && darkMedia.addEventListener) {
  darkMedia.addEventListener('change', () => {
    if (state.settings.theme === 'system') {
      applyLook();
      renderLibrary();
    }
  });
}

function renderSchemes() {
  for (const mode of ['light', 'dark']) {
    const current = schemeFor(mode);
    $(`schemes-${mode}`).replaceChildren(...SCHEMES[mode].map((s, i) => {
      const b = document.createElement('button');
      b.type = 'button';
      b.className = `scheme${s.id === current ? ' on' : ''}`;
      b.title = `${s.name}${i === 0 ? ' (default)' : ''}`;
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
        const key = mode === 'dark' ? 'darkScheme' : 'lightScheme';
        state.settings[key] = s.id;
        applyLook();
        renderSchemes();
        post({ cmd: 'settings', [key]: s.id });
      });
      return b;
    }));
  }
}

function renderSettings() {
  const s = state.settings;
  el.setCheckEvery.value = s.checkEvery === 'manual' ? 'manual' : 'auto';
  el.setAuto.checked = !!s.autoUpdate;
  el.setSimDl.value = String(s.simultaneousDownloads ?? 1);
  el.setNotify.checked = s.notifyAppUpdates !== false;
  el.setPre.checked = !!s.prereleases;

  const look = s.look === 'legacy' ? 'legacy' : 'modern';
  el.lookButtons.forEach((b) => b.classList.toggle('on', b.dataset.lookVal === look));
  const themePref = ['system', 'light', 'dark'].includes(s.theme) ? s.theme : 'light';
  el.themeButtons.forEach((b) => b.classList.toggle('on', b.dataset.themeVal === themePref));

  const folder = s.installDir && s.installDir.trim() ? s.installDir : BOOT.defaultAppsDir;
  el.installDisplay.value = folder;
  el.btnDefaultInst.disabled = !s.installDir;

  // Sync toolbar controls with settings
  el.sortSelect.value = s.sort || 'name-asc';
  const view = s.view === 'table' ? 'table' : 'grid';
  el.vtGrid.classList.toggle('on', view === 'grid');
  el.vtTable.classList.toggle('on', view === 'table');
  el.sortWrap.style.display = view === 'grid' ? 'inline-flex' : 'none';

  applyLook();
  renderSchemes();
  renderLastCheck();
  renderActivity();
}

function patchSettings(patch) {
  Object.assign(state.settings, patch);
  renderSettings();
  post({ cmd: 'settings', ...patch });
}

el.setCheckEvery.addEventListener('change', () => patchSettings({ checkEvery: el.setCheckEvery.value }));
el.setAuto.addEventListener('change', () => patchSettings({ autoUpdate: el.setAuto.checked }));
el.setSimDl.addEventListener('change', () => patchSettings({ simultaneousDownloads: Number(el.setSimDl.value) }));
el.setNotify.addEventListener('change', () => patchSettings({ notifyAppUpdates: el.setNotify.checked }));
el.setPre.addEventListener('change', () => patchSettings({ prereleases: el.setPre.checked }));

el.lookButtons.forEach((b) => b.addEventListener('click', () => {
  patchSettings({ look: b.dataset.lookVal });
  renderLibrary();
}));

el.themeButtons.forEach((b) => b.addEventListener('click', () => {
  patchSettings({ theme: b.dataset.themeVal });
  renderLibrary();
}));

el.btnBrowseInst.addEventListener('click', () => post({ cmd: 'browseInstallDir' }));
el.btnDefaultInst.addEventListener('click', () => post({ cmd: 'resetInstallDir' }));
el.btnData.addEventListener('click', () => post({ cmd: 'openDataDir' }));

function renderLastCheck() {
  const t = state.settings.lastCheck;
  el.last.textContent = t ? `Checked ${ago(t)}` : '';
}

setInterval(() => {
  renderLastCheck();
  renderActivity();
}, 30000);

/* ----------------------------------------------------------- updates */

function describe(u) {
  switch (u.state) {
    case 'checking':
      return { pill: 'busy', short: 'Checking for updates…',
               title: 'Checking…', detail: `Asking GitHub for the latest release of ${BOOT.repo}.` };
    case 'upToDate':
      return { pill: 'ok', short: 'Up to date',
               title: 'Up to date', detail: 'You are running the newest release on GitHub.' };
    case 'available':
      return { pill: 'gilt', short: 'Launcher update available',
               title: 'Update available', detail: `A newer Launcher release is out (${fmtMB(u.release.size)}).` };
    case 'downloading':
      return { pill: 'busy', short: `Downloading update — ${pct(u)}%`,
               title: 'Downloading…',
               detail: u.total ? `${fmtMB(u.done)} of ${fmtMB(u.total)}` : 'Starting the download…' };
    case 'ready':
      return { pill: 'gilt',
               short: state.deferred ? 'Update installs when you close the launcher' : 'Update is ready to install',
               title: 'Ready to restart',
               detail: 'Downloaded and verified. Restart to switch over, or it installs itself when you close the launcher.' };
    case 'error':
      return { pill: 'bad', short: u.message, title: 'Update failed', detail: u.message };
    default:
      return { pill: '', short: 'Not checked',
               title: 'Not checked', detail: 'The launcher can check GitHub for a newer release.' };
  }
}

function activityText(a) {
  const t = a.task;
  const p = t.total ? Math.floor((t.done / t.total) * 100) : 0;
  switch (t.state) {
    case 'queued':      return `Queued: ${a.name}`;
    case 'downloading': return `${a.installed ? 'Updating' : 'Downloading'} ${a.name}` + (t.total ? ` — ${p}%` : '…');
    case 'installing':  return `Installing ${a.name}…`;
    case 'verifying':   return `Verifying ${a.name}` + (t.total ? ` — ${t.done} of ${t.total} files` : '…');
    case 'repairing':   return `Repairing ${a.name}` + (t.total ? ` — ${p}%` : '…');
    default:            return null;
  }
}

function renderActivity() {
  const jobs = [];
  const u = state.update;
  if (u.state === 'downloading') jobs.push(`Downloading Launcher update — ${pct(u)}%`);
  for (const a of library.apps) {
    const text = activityText(a);
    if (text) jobs.push(text);
  }
  const active = jobs.length > 0;
  const checkingAny = u.state === 'checking' || library.apps.some((a) => a.task.state === 'checking');

  el.lamp.className = active || checkingAny ? 'busy' : '';
  if (active) {
    el.statusText.textContent = jobs[0] + (jobs.length > 1 ? ` · +${jobs.length - 1} more` : '');
    el.statusText.title = jobs.join('\n');
  } else if (checkingAny) {
    el.statusText.textContent = 'Checking for updates…';
    el.statusText.title = '';
  } else if (state.settings.lastCheck) {
    el.statusText.textContent = `Checked ${ago(state.settings.lastCheck)} · using GitHub`;
    el.statusText.title = '';
  } else {
    el.statusText.textContent = 'Using GitHub';
    el.statusText.title = '';
  }
}

function renderUpdate() {
  const u = state.update;
  const d = describe(u);

  updateSplash();
  renderActivity();
  renderBell();
  el.root.classList.toggle('fuelling', u.state === 'downloading');

  // Footer update shortcut button
  const nudge = { available: 'Update', ready: 'Restart', error: 'Details' }[u.state];
  el.statusUpd.classList.toggle('on', !!nudge);
  el.statusUpd.title = nudge ? d.short : 'Update settings';
  if (nudge) el.statusUpdT.textContent = nudge;

  // Settings panel manual update section
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
  renderBanner();
  renderUpdateToast();
}

function renderBanner() {
  const u = state.update;
  const show = !state.splashActive && !state.bannerDismissed
    && (u.state === 'available' || u.state === 'downloading' || u.state === 'ready')
    && state.updateMode === 'background';
  // Only show the top banner if the user dismissed the bottom-right popup for this update
  const dismissedPopup = u.release && state.bgToastDismissed === u.release.version;
  el.banner.classList.toggle('on', !!show && !!dismissedPopup);
  if (!show || !dismissedPopup) return;

  if (u.state === 'downloading') {
    el.bannerText.textContent = 'Downloading Launcher update…';
    el.bannerProgress.classList.add('on');
    el.bannerProgLabel.textContent = u.total ? `${fmtMB(u.done)} of ${fmtMB(u.total)} · ${pct(u)}%` : 'Downloading…';
    el.bannerBar.style.width = `${pct(u)}%`;
    el.bannerAction.style.display = 'none';
  } else if (u.state === 'ready') {
    el.bannerText.textContent = 'Launcher update is ready to install.';
    el.bannerProgress.classList.remove('on');
    el.bannerAction.style.display = 'inline-flex';
    el.bannerAction.textContent = 'Restart now';
  } else {
    el.bannerText.textContent = 'A newer Launcher update is available.';
    el.bannerProgress.classList.remove('on');
    el.bannerAction.style.display = 'inline-flex';
    el.bannerAction.textContent = 'Update & restart';
  }
}

el.bannerAction.addEventListener('click', () => {
  if (state.update.state === 'available') post({ cmd: 'downloadUpdate' });
  else if (state.update.state === 'ready') post({ cmd: 'restartNow' });
});
el.bannerDismiss.addEventListener('click', () => {
  state.bannerDismissed = true;
  renderBanner();
});

function renderNotes(rel) {
  el.notes.classList.toggle('on', !!rel);
  if (!rel) return;
  el.notesTitle.textContent = (rel.prerelease ? 'Pre-release notes' : 'Release notes');
  el.notesLink.dataset.url = rel.page;
  const body = (rel.notes || '').trim();
  el.notesBody.replaceChildren();
  if (body) {
    el.notesBody.textContent = body;
  } else {
    const m = h('span', 'muted', 'No release notes.');
    el.notesBody.append(m);
  }
}

el.btnCheck.addEventListener('click', () => post({ cmd: 'checkUpdate' }));
el.btnDownload.addEventListener('click', () => post({ cmd: 'downloadUpdate' }));
el.btnRestart.addEventListener('click', () => post({ cmd: 'restartNow' }));
el.statusUpd.addEventListener('click', () => {
  if (state.update.state === 'available') post({ cmd: 'downloadUpdate' });
  else if (state.update.state === 'ready') post({ cmd: 'restartNow' });
  else openSettings();
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
el.btnGithubOrg.addEventListener('click', () => post({ cmd: 'openUrl', url: GITHUB_ORG_URL }));

/* ----------------------------------------------------- updates bell */

function renderBell() {
  const appUpdates = library.apps.filter((a) => a.updateAvailable && isSupported(a));
  const launcherHas = state.update.state === 'available' || state.update.state === 'ready';
  const total = appUpdates.length + (launcherHas ? 1 : 0);

  el.bellCount.textContent = total;
  el.bellCount.classList.toggle('on', total > 0);

  el.bellBody.replaceChildren();
  if (total === 0) {
    el.bellBody.append(h('div', 'popover-empty', 'Everything is up to date.'));
    return;
  }

  if (launcherHas) {
    const row = h('div', 'popover-item');
    const info = h('div', 'popover-item-info');
    info.append(
      h('div', 'popover-item-name', 'Launcher'),
      h('div', 'popover-item-meta', state.update.state === 'ready' ? 'Ready to restart' : 'Update available'),
    );
    const act = h('button', 'btn primary compact', state.update.state === 'ready' ? 'Restart' : 'Update');
    act.type = 'button';
    act.addEventListener('click', () => {
      closeBell();
      post({ cmd: state.update.state === 'ready' ? 'restartNow' : 'downloadUpdate' });
    });
    row.append(info, act);
    el.bellBody.append(row);
  }

  for (const a of appUpdates) {
    const row = h('div', 'popover-item');
    const info = h('div', 'popover-item-info');
    info.append(
      h('div', 'popover-item-name', a.name),
      h('div', 'popover-item-meta', `v${a.installed} → v${a.latest.version}`),
    );
    const act = h('button', 'btn primary compact', 'Update');
    act.type = 'button';
    act.addEventListener('click', () => {
      closeBell();
      post({ cmd: 'appInstall', id: a.id });
    });
    row.append(info, act);
    el.bellBody.append(row);
  }
}

function closeBell() {
  el.bellPopover.classList.remove('on');
  el.btnBell.classList.remove('on');
}

el.btnBell.addEventListener('click', (e) => {
  e.stopPropagation();
  const open = el.bellPopover.classList.toggle('on');
  el.btnBell.classList.toggle('on', open);
  if (open) closeCardMenu();
});
el.bellClose.addEventListener('click', closeBell);

/* ----------------------------------------------------------- library */

const library = {
  apps: [],
  armed: null,
  armedTimer: null,
  logOpen: {},
  logStart: {},
  tableAsc: true,
};

const APP_ICONS = BOOT.icons || {};

function tabOf(a) {
  const cat = a.category || 'apps';
  return cat === 'apps' ? 'library' : cat;
}

function summarizeApps(list, noun = 'app') {
  const updates = list.filter((a) => a.updateAvailable).length;
  const installed = list.filter((a) => a.installed).length;
  return `${list.length} ${noun}${list.length === 1 ? '' : 's'} · ${installed} installed`
    + (updates ? ` · ${updates} update${updates > 1 ? 's' : ''}` : '');
}

function setBadge(id, list) {
  const updates = list.filter((a) => a.updateAvailable).length;
  const badge = $(id);
  if (!badge) return;
  badge.textContent = updates;
  badge.classList.toggle('on', updates > 0);
}

function isPinned(id) {
  return Array.isArray(state.settings.pinned) && state.settings.pinned.includes(id);
}

function togglePin(id) {
  const cur = Array.isArray(state.settings.pinned) ? [...state.settings.pinned] : [];
  const idx = cur.indexOf(id);
  if (idx >= 0) cur.splice(idx, 1);
  else cur.push(id);
  patchSettings({ pinned: cur });
  renderLibrary();
}

function statusRank(a) {
  if (a.updateAvailable) return 0;
  if (a.running) return 1;
  if (a.installed) return 2;
  if (isSupported(a)) return 3;
  return 4;
}

function sortApps(list) {
  const key = state.settings.sort || 'name-asc';
  const pinnedOrder = state.settings.pinned || [];
  return [...list].sort((a, b) => {
    const pinA = pinnedOrder.indexOf(a.id);
    const pinB = pinnedOrder.indexOf(b.id);
    if (pinA !== -1 || pinB !== -1) {
      if (pinA === -1) return 1;
      if (pinB === -1) return -1;
      return pinA - pinB;
    }
    if (key === 'name-desc') {
      return b.name.localeCompare(a.name, undefined, { sensitivity: 'base' });
    }
    if (key === 'status') {
      const d = statusRank(a) - statusRank(b);
      if (d !== 0) return d;
    }
    if (key === 'updated') {
      const ta = a.latest && a.latest.published ? Date.parse(a.latest.published) || 0 : 0;
      const tb = b.latest && b.latest.published ? Date.parse(b.latest.published) || 0 : 0;
      if (ta !== tb) return tb - ta;
    }
    return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
  });
}

function matchesSearch(a, q) {
  if (!q) return true;
  const hay = `${a.name} ${a.tagline || ''} ${a.repo || ''} ${a.publisher || ''}`.toLowerCase();
  return hay.includes(q);
}

function renderWordmark(name, parent) {
  if (name.endsWith('Craft') && name.length > 5) {
    parent.append(name.slice(0, -5), h('span', 'name-accent', 'Craft'));
  } else {
    parent.textContent = name;
  }
}

function pinSvg(pinned) {
  return pinned
    ? '<svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M3.5 2h9v12.5L8 11.2l-4.5 3.3V2z"/></svg>'
    : '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="miter" aria-hidden="true"><path d="M3.5 2.5h9v11.5L8 10.8 3.5 14V2.5z"/></svg>';
}

function dotsSvg() {
  return '<svg width="15" height="15" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><circle cx="3.2" cy="8" r="1.3"/><circle cx="8" cy="8" r="1.3"/><circle cx="12.8" cy="8" r="1.3"/></svg>';
}

function appStyleVars(a) {
  const dark = effectiveTheme(state.settings.theme) === 'dark';
  const accent = a.accent || '#2D81FF';
  const ink = dark ? (a.inkDark || '#74AAFF') : (a.inkLight || '#1659C4');
  return `--app-accent:${accent};--app-ink:${ink};`;
}

function buildPrimaryActions(a, compact = false) {
  const t = a.task;
  const busy = ['checking', 'queued', 'downloading', 'installing', 'verifying', 'repairing', 'stopping'].includes(t.state);
  const supported = isSupported(a);
  const latest = a.latest;
  const damaged = (a.damaged || []).length;
  const szCls = compact ? 'compact' : 'small';

  if (t.state === 'queued') {
    const b = h('button', `btn ${szCls} main-btn`, 'Queued · Cancel');
    b.type = 'button';
    b.addEventListener('click', () => post({ cmd: 'appCancelQueue', id: a.id }));
    return b;
  }

  if (a.installed) {
    if (damaged) {
      const fix = h('button', `btn ${szCls} gilt main-btn`, `Repair ${damaged} file${damaged > 1 ? 's' : ''}`);
      fix.type = 'button';
      fix.disabled = busy;
      fix.addEventListener('click', () => post({ cmd: 'appRepair', id: a.id }));
      return fix;
    }

    if (t.state === 'stopping') {
      const b = h('button', `btn ${szCls} danger main-btn`, 'Stopping…');
      b.type = 'button';
      b.disabled = true;
      return b;
    }

    if (a.running) {
      const stop = h('button', `btn ${szCls} danger main-btn`, 'Stop');
      stop.type = 'button';
      stop.disabled = busy;
      stop.title = `Ask ${a.name} to quit`;
      stop.addEventListener('click', () => post({ cmd: 'appStop', id: a.id }));
      return stop;
    }

    if (a.updateAvailable && supported) {
      const split = h('div', 'split-btns');
      const open = h('button', `btn ${szCls}`, 'Open');
      open.type = 'button';
      open.disabled = busy;
      open.addEventListener('click', () => post({ cmd: 'appLaunch', id: a.id }));

      const updLabel = compact ? 'Update' : `Update`;
      const up = h('button', `btn ${szCls} primary`, updLabel);
      up.type = 'button';
      up.title = `Update ${a.name} to v${latest.version}`;
      up.disabled = busy;
      up.addEventListener('click', () => post({ cmd: 'appInstall', id: a.id }));
      split.append(open, up);
      return split;
    }

    if (a.starting) {
      const b = h('button', `btn ${szCls} main-btn`, 'Starting…');
      b.type = 'button';
      b.disabled = true;
      return b;
    }

    const open = h('button', `btn ${szCls} main-btn`, 'Open');
    open.type = 'button';
    open.disabled = busy || !supported;
    open.addEventListener('click', () => post({ cmd: 'appLaunch', id: a.id }));
    return open;
  }

  if (!supported) {
    const plat = Array.isArray(a.platforms) && a.platforms.length ? `${a.platforms.join(' / ')} only` : `Not on ${CURRENT_OS}`;
    const b = h('button', `btn ${szCls} main-btn`, plat);
    b.type = 'button';
    b.disabled = true;
    return b;
  }

  const label = !latest && t.state === 'checking' ? 'Checking…' : 'Install';
  const inst = h('button', `btn ${szCls} primary main-btn`, label);
  inst.type = 'button';
  inst.disabled = busy || !latest;
  inst.addEventListener('click', () => post({ cmd: 'appInstall', id: a.id }));
  return inst;
}

function statusBadgeInfo(a) {
  const t = a.task;
  const supported = isSupported(a);
  if (t.state === 'queued') return { text: 'Queued', tone: 'info' };
  if (t.state === 'downloading') return { text: 'Downloading…', tone: 'info' };
  if (t.state === 'installing') return { text: 'Installing…', tone: 'info' };
  if (t.state === 'verifying') return { text: 'Verifying…', tone: 'info' };
  if (t.state === 'repairing') return { text: 'Repairing…', tone: 'warn' };
  if (t.state === 'error') return { text: 'Error', tone: 'bad' };
  if (a.running) return { text: 'Running', tone: 'good', dot: true };
  if (a.installed) {
    if (a.updateAvailable) return { text: 'Update available', tone: 'warn' };
    return null; // Installed & up to date: keep clean just like ArtCraft Launcher!
  }
  if (!supported) {
    const p = Array.isArray(a.platforms) && a.platforms.length ? `${a.platforms.join(' / ')} only` : `Not on ${CURRENT_OS}`;
    return { text: p, tone: '' };
  }
  if (a.latest) return { text: 'Available to install', tone: 'info' };
  if (a.checked) return { text: 'No release yet', tone: '' };
  return { text: 'Checking…', tone: '' };
}

function fillAppStatus(status, a) {
  const t = a.task;
  const damaged = (a.damaged || []).length;
  status.className = 'app-status';
  status.replaceChildren();

  if (t.state === 'downloading') {
    const p = t.total ? Math.floor((t.done / t.total) * 100) : 0;
    const bar = h('div', 'progress on');
    const fill = h('div', 'bar');
    fill.style.width = `${p}%`;
    bar.append(fill);
    status.append(bar, t.total ? `Downloading ${fmtMB(t.done)} of ${fmtMB(t.total)} · ${p}%` : 'Downloading…');
  } else if (t.state === 'installing') {
    status.append('Verifying files and installing…');
  } else if (t.state === 'queued') {
    status.append('Waiting for another download to finish…');
  } else if (t.state === 'verifying' || t.state === 'repairing') {
    const frac = t.total ? t.done / t.total : 0;
    const bar = h('div', 'progress on');
    const fill = h('div', 'bar');
    fill.style.width = `${Math.floor(frac * 100)}%`;
    bar.append(fill);
    status.append(bar, t.state === 'verifying'
      ? (t.total ? `Verifying ${t.done} of ${t.total} files` : 'Verifying…')
      : (t.total ? `Repairing ${Math.floor(frac * 100)}% of ${fmtMB(t.total)}` : 'Reading package index…'));
  } else if (damaged && t.state !== 'error') {
    status.classList.add('bad');
    status.append(`${damaged} file${damaged > 1 ? 's' : ''} damaged or missing. Press Repair to fix.`);
  } else if (t.state === 'error') {
    status.classList.add('bad');
    status.append(t.message);
  }
}

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
  toggle.type = 'button';
  toggle.title = open ? 'Hide log' : 'Show log';
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

function buildAppCard(a) {
  const supported = isSupported(a);
  const icon = a.icon || APP_ICONS[a.id];
  const latest = a.latest;
  const pinned = isPinned(a.id);

  const wrap = h('div', 'app-wrap');
  wrap.dataset.appId = a.id;

  const card = h('div', `app${a.installed ? ' installed' : ' not-installed'}${a.updateAvailable ? ' has-update' : ''}${supported ? '' : ' unsupported'}`);
  card.setAttribute('style', appStyleVars(a));

  // Top strip: version + GitHub link (matching ArtCraft Launcher)
  const strip = h('div', 'app-strip');
  const ver = h('span', 'strip-ver');
  if (a.installed && a.updateAvailable && latest) {
    ver.append(h('span', 'dim-part', `v${a.installed} → `), h('span', 'new-ver', `v${latest.version}`));
  } else if (a.installed) {
    ver.textContent = `v${a.installed}`;
  } else if (latest) {
    ver.textContent = `v${latest.version}`;
  } else if (!supported) {
    ver.textContent = Array.isArray(a.platforms) ? `${a.platforms.join(' / ')} only` : 'Unavailable';
  } else {
    ver.textContent = a.checked ? 'No release' : 'Checking…';
  }

  const ghLink = h('a', 'strip-link', 'GitHub ↗');
  ghLink.href = '#';
  ghLink.title = `https://github.com/${a.repo}`;
  ghLink.addEventListener('click', (e) => {
    e.preventDefault();
    post({ cmd: 'openUrl', url: `https://github.com/${a.repo}` });
  });
  strip.append(ver, ghLink);
  card.append(strip);

  // Body
  const body = h('div', 'app-body');

  // Icon + Name + Publisher badge + platform subtitle
  const head = h('div', 'app-head');
  const tile = h('div', `app-tile${icon ? ' has-icon' : ''}`);
  if (icon) {
    const img = h('img', 'app-icon');
    img.src = icon;
    img.alt = '';
    img.draggable = false;
    tile.append(img);
  } else {
    tile.textContent = a.name[0];
  }

  const ident = h('div', 'app-identity');
  const nameRow = h('div', 'app-namerow');
  const nameEl = h('span', 'app-name');
  renderWordmark(a.name, nameEl);
  nameRow.append(nameEl);

  const pub = a.publisher || (a.thirdParty ? 'ArtCraft' : 'Javacrafter');
  const pubBadge = h('span', `hud-badge${pub === 'Javacrafter' || pub === 'ArtCraft' ? ' info' : ''}`, `by ${pub}`);
  nameRow.append(pubBadge);

  const subText = Array.isArray(a.platforms) && a.platforms.length ? a.platforms.join(' · ') : '';
  ident.append(nameRow);
  if (subText) ident.append(h('div', 'app-sub', subText));
  head.append(tile, ident);
  body.append(head);

  // Status badges row
  const badges = h('div', 'app-badges-row');
  const badgeInfo = statusBadgeInfo(a);
  if (badgeInfo) {
    const b = h('span', `hud-badge ${badgeInfo.tone || ''}`);
    if (badgeInfo.dot) b.append(h('span', 'running-dot'));
    b.append(badgeInfo.text);
    badges.append(b);
  }
  if (a.portable != null) {
    const pTag = h('span', `hud-badge${a.portable ? ' good' : ''}`, a.portable ? 'Portable' : 'Not portable');
    pTag.title = a.portable
      ? `Keeps its settings and data in ${a.folder}Data, inside its own folder.`
      : `Keeps its settings in ~/Library/Application Support/${a.name}, outside its own folder.`;
    badges.append(pTag);
  }
  body.append(badges);

  // Copy block: Download size (when install/update), tagline, last updated date
  const copy = h('div', 'app-copy');
  if ((!a.installed || a.updateAvailable) && supported && latest && latest.size > 0 && a.task.state === 'idle') {
    copy.append(h('div', 'app-size', `Download size: ${fmtMB(latest.size)}`));
  }
  if (a.tagline) {
    const tag = h('div', 'app-tagline', a.tagline);
    tag.title = a.tagline;
    copy.append(tag);
  }
  const pubDate = latest && latest.published ? fmtDate(latest.published) : '';
  if (pubDate) {
    copy.append(h('div', 'app-updated', `Last updated: ${pubDate}`));
  }
  body.append(copy);

  // Live task status / progress bar / error
  const status = h('div', 'app-status');
  fillAppStatus(status, a);
  body.append(status);

  // Bottom action row: Main button + Pin + Overflow menu
  const actions = h('div', 'app-actions');
  actions.append(buildPrimaryActions(a, false));

  const pinBtn = h('button', `sq-btn pin-btn${pinned ? ' pinned' : ''}`);
  pinBtn.type = 'button';
  pinBtn.title = pinned ? `Unpin ${a.name}` : `Pin ${a.name} to top`;
  pinBtn.innerHTML = pinSvg(pinned);
  pinBtn.addEventListener('click', () => togglePin(a.id));
  actions.append(pinBtn);

  const menuBtn = h('button', 'sq-btn menu-btn');
  menuBtn.type = 'button';
  menuBtn.title = `More actions for ${a.name}`;
  menuBtn.innerHTML = dotsSvg();
  menuBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    openCardMenu(a, menuBtn);
  });
  actions.append(menuBtn);

  body.append(actions);
  card.append(body);
  wrap.append(card);

  if (a.log && a.log.length) {
    wrap.append(appLog(a));
  }
  return wrap;
}

function buildTableView(list) {
  const wrap = h('div', 'apps-table-wrap');
  const table = h('table', 'apps-table');
  const thead = h('thead');
  const hrow = h('tr');

  const curSort = state.settings.sort || 'name-asc';
  const makeTh = (label, sortKey) => {
    const th = h('th', sortKey ? 'sortable' : '', label);
    if (sortKey) {
      const isName = sortKey === 'name' && (curSort === 'name-asc' || curSort === 'name-desc');
      const active = isName || curSort === sortKey;
      if (active) {
        th.classList.add('sorted');
        const arrow = curSort === 'name-desc' || curSort === 'updated' ? '▼' : '▲';
        th.append(h('span', 'sort-ind', arrow));
      }
      th.addEventListener('click', () => {
        if (sortKey === 'name') {
          patchSettings({ sort: curSort === 'name-asc' ? 'name-desc' : 'name-asc' });
        } else {
          patchSettings({ sort: sortKey });
        }
        renderLibrary();
      });
    }
    return th;
  };

  hrow.append(
    makeTh('App', 'name'),
    makeTh('Status', 'status'),
    makeTh('Version', null),
    makeTh('Released', 'updated'),
    makeTh('Author', null),
    makeTh('', null),
  );
  thead.append(hrow);
  table.append(thead);

  const tbody = h('tbody');
  for (const a of list) {
    const tr = h('tr');
    tr.dataset.appId = a.id;
    tr.setAttribute('style', appStyleVars(a));

    // App cell
    const tdApp = h('td');
    const appBox = h('div', 'tbl-app');
    const icon = a.icon || APP_ICONS[a.id];
    const ic = h('div', `tbl-icon${icon ? ' has-icon' : ''}${a.installed ? '' : ' dim'}`);
    if (icon) {
      const img = h('img');
      img.src = icon;
      img.alt = '';
      ic.append(img);
    } else {
      ic.textContent = a.name[0];
    }
    const tx = h('div');
    const nm = h('div', 'tbl-name');
    renderWordmark(a.name, nm);
    tx.append(nm, h('div', 'tbl-tag', a.tagline || ''));
    appBox.append(ic, tx);
    tdApp.append(appBox);

    // Status cell
    const tdStatus = h('td');
    const bi = statusBadgeInfo(a) || { text: 'Installed', tone: 'good' };
    const badge = h('span', `hud-badge ${bi.tone || ''}`);
    if (bi.dot) badge.append(h('span', 'running-dot'));
    badge.append(bi.text);
    tdStatus.append(badge);

    // Version cell
    const tdVer = h('td', 'tbl-ver');
    if (a.installed && a.updateAvailable && a.latest) {
      tdVer.append(`${a.installed} → `, h('span', 'new-ver', a.latest.version));
    } else if (a.installed) {
      tdVer.textContent = a.installed;
    } else if (a.latest) {
      tdVer.textContent = a.latest.version;
    } else {
      tdVer.textContent = '—';
    }

    // Released cell
    const tdRel = h('td', 'tbl-date', (a.latest && a.latest.published && fmtDate(a.latest.published)) || '—');

    // Author cell
    const tdAuth = h('td', 'tbl-author', a.publisher || (a.thirdParty ? 'ArtCraft' : 'Javacrafter'));

    // Actions cell
    const tdAct = h('td');
    const acts = h('div', 'tbl-acts');
    acts.append(buildPrimaryActions(a, true));

    const pinned = isPinned(a.id);
    const pinBtn = h('button', `sq-btn pin-btn${pinned ? ' pinned' : ''}`);
    pinBtn.type = 'button';
    pinBtn.title = pinned ? `Unpin ${a.name}` : `Pin ${a.name} to top`;
    pinBtn.innerHTML = pinSvg(pinned);
    pinBtn.addEventListener('click', () => togglePin(a.id));
    acts.append(pinBtn);

    const menuBtn = h('button', 'sq-btn menu-btn');
    menuBtn.type = 'button';
    menuBtn.title = `More actions for ${a.name}`;
    menuBtn.innerHTML = dotsSvg();
    menuBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      openCardMenu(a, menuBtn);
    });
    acts.append(menuBtn);

    tdAct.append(acts);
    tr.append(tdApp, tdStatus, tdVer, tdRel, tdAuth, tdAct);
    tbody.append(tr);
  }

  table.append(tbody);
  wrap.append(table);
  return wrap;
}

function renderGridSection(title, list) {
  const sec = h('div', 'section-group');
  if (title) {
    const hdr = h('div', 'section-label');
    hdr.append(h('span', 'section-heading', title));
    sec.append(hdr);
  }
  const grid = h('div', 'apps-grid');
  for (const a of list) {
    grid.append(buildAppCard(a));
  }
  sec.append(grid);
  return sec;
}

function renderLibrary() {
  const all = library.apps;
  const libApps  = all.filter((a) => tabOf(a) === 'library');
  const tempApps = all.filter((a) => tabOf(a) === 'temporary');
  const gameApps = all.filter((a) => tabOf(a) === 'games');

  // Update tab badges (both Modern & Legacy)
  setBadge('lib-badge', libApps);
  setBadge('temp-badge', tempApps);
  setBadge('games-badge', gameApps);
  setBadge('legacy-lib-badge', libApps);
  setBadge('legacy-temp-badge', tempApps);
  setBadge('legacy-games-badge', gameApps);

  // Refresh button spinning state
  const checking = all.some((a) => a.task.state === 'checking');
  el.btnRefresh.disabled = checking;
  el.btnRefresh.classList.toggle('spin', checking);

  // Update All button (across all eligible installed apps with updates)
  const updatable = all.filter((a) => a.installed && a.updateAvailable && isSupported(a) && !a.running
    && !['downloading', 'installing', 'queued'].includes(a.task.state));
  el.btnUpdateAll.classList.toggle('on', updatable.length > 0);
  el.btnUpdateAll.textContent = updatable.length === 1 ? 'Update 1 app' : `Update all (${updatable.length})`;

  // Active tab list + search filter
  const tabList = state.tab === 'temporary' ? tempApps : state.tab === 'games' ? gameApps : libApps;
  el.catalogSummary.textContent = summarizeApps(tabList, state.tab === 'games' ? 'game' : 'app');

  const q = state.search.trim().toLowerCase();
  const filtered = sortApps(tabList.filter((a) => matchesSearch(a, q)));

  // Preserve open log scroll positions
  const scroll = {};
  document.querySelectorAll('.log-body').forEach((b) => {
    scroll[b.dataset.id] = { top: b.scrollTop, end: b.scrollTop + b.clientHeight >= b.scrollHeight - 4 };
  });

  el.catalogContent.replaceChildren();

  if (filtered.length === 0) {
    const emptyMsg = q
      ? `No matches for “${state.search.trim()}”.`
      : state.tab === 'temporary'
        ? 'No temporary apps right now.'
        : state.tab === 'games'
          ? 'No games yet. They’ll appear here once they’re added.'
          : 'No apps found.';
    el.catalogContent.append(h('div', 'apps-empty', emptyMsg));
  } else if (state.settings.view === 'table') {
    const pinned = filtered.filter((a) => isPinned(a.id));
    const installed = filtered.filter((a) => !isPinned(a.id) && a.installed);
    const notInstalled = filtered.filter((a) => !isPinned(a.id) && !a.installed);
    el.catalogContent.append(buildTableView([...pinned, ...installed, ...notInstalled]));
  } else {
    const pinned = filtered.filter((a) => isPinned(a.id));
    const installed = filtered.filter((a) => !isPinned(a.id) && a.installed);
    const notInstalled = filtered.filter((a) => !isPinned(a.id) && !a.installed);

    if (pinned.length) {
      el.catalogContent.append(renderGridSection('Pinned', pinned));
    }
    if (installed.length) {
      el.catalogContent.append(renderGridSection(pinned.length ? 'Installed' : '', installed));
    }
    if (notInstalled.length) {
      const showHeader = pinned.length > 0 || installed.length > 0;
      el.catalogContent.append(renderGridSection(showHeader ? 'Not installed' : '', notInstalled));
    }
  }

  document.querySelectorAll('.log-body').forEach((b) => {
    const s = scroll[b.dataset.id];
    b.scrollTop = !s || s.end ? b.scrollHeight : s.top;
  });

  // If card menu is currently open for an app, refresh its items in case state changed
  if (state.openMenuId) {
    const openApp = library.apps.find((x) => x.id === state.openMenuId);
    if (openApp) fillCardMenu(openApp);
    else closeCardMenu();
  }

  renderBell();
}

/* --------------------------------------------------- card overflow menu */

function closeCardMenu() {
  state.openMenuId = null;
  el.cardMenu.classList.remove('on');
}

function openCardMenu(a, anchor) {
  if (state.openMenuId === a.id && el.cardMenu.classList.contains('on')) {
    closeCardMenu();
    return;
  }
  closeBell();
  state.openMenuId = a.id;
  fillCardMenu(a);
  el.cardMenu.classList.add('on');

  const r = anchor.getBoundingClientRect();
  const mw = el.cardMenu.offsetWidth || 200;
  const mh = el.cardMenu.offsetHeight || 160;
  let left = r.right - mw;
  let top = r.bottom + 4;
  if (left < 8) left = 8;
  if (top + mh > window.innerHeight - 12) top = Math.max(8, r.top - mh - 4);
  el.cardMenu.style.left = `${left}px`;
  el.cardMenu.style.top = `${top}px`;
}

function fillCardMenu(a) {
  const busy = ['checking', 'queued', 'downloading', 'installing', 'verifying', 'repairing', 'stopping'].includes(a.task.state);
  el.cardMenu.replaceChildren();

  const addItem = (label, onClick, opts = {}) => {
    const b = h('button', `cm-item${opts.cls ? ` ${opts.cls}` : ''}`, label);
    b.type = 'button';
    if (opts.disabled) b.disabled = true;
    b.addEventListener('click', (e) => {
      e.stopPropagation();
      onClick();
      if (!opts.keepOpen) closeCardMenu();
    });
    el.cardMenu.append(b);
  };

  if (a.installed) {
    if (a.bat && !a.running && !a.starting && a.task.state !== 'stopping') {
      addItem('Launch with .bat', () => post({ cmd: 'appLaunchBat', id: a.id }), { disabled: busy });
    }
    addItem('Open install folder', () => post({ cmd: 'appFolder', id: a.id }));
    addItem('Verify & repair files', () => post({ cmd: 'appVerify', id: a.id }), { disabled: busy });
  }

  if (a.latest && a.latest.page) {
    addItem('Release notes ↗', () => post({ cmd: 'openUrl', url: a.latest.page }));
  }
  addItem('View on GitHub ↗', () => post({ cmd: 'openUrl', url: `https://github.com/${a.repo}` }));

  if (a.log && a.log.length) {
    const isOpen = !!library.logOpen[a.id];
    addItem(isOpen ? 'Hide activity log' : 'Show activity log', () => {
      library.logOpen[a.id] = !isOpen;
      renderLibrary();
    });
  }

  if (a.installed) {
    el.cardMenu.append(h('div', 'cm-sep'));
    const armed = library.armed === a.id;
    const label = armed
      ? (CURRENT_OS === 'MacOS' ? 'Click again to move to Trash' : 'Click again to delete app & data')
      : 'Uninstall';
    addItem(label, () => uninstallClick(a.id), {
      cls: armed ? 'danger armed' : 'danger',
      disabled: busy,
      keepOpen: !armed,
    });
  }
}

function uninstallClick(id) {
  clearTimeout(library.armedTimer);
  if (library.armed === id) {
    library.armed = null;
    closeCardMenu();
    post({ cmd: 'appUninstall', id });
  } else {
    library.armed = id;
    library.armedTimer = setTimeout(() => {
      library.armed = null;
      if (state.openMenuId === id) {
        const a = library.apps.find((x) => x.id === id);
        if (a) fillCardMenu(a);
      }
    }, 4500);
    const a = library.apps.find((x) => x.id === id);
    if (a) fillCardMenu(a);
  }
}

/* --------------------------------------------------- toolbar events */

el.btnRefresh.addEventListener('click', () => post({ cmd: 'appCheck' }));
el.btnUpdateAll.addEventListener('click', () => post({ cmd: 'appUpdateAll' }));
el.sortSelect.addEventListener('change', () => {
  patchSettings({ sort: el.sortSelect.value });
  renderLibrary();
});

el.searchInput.addEventListener('input', () => {
  state.search = el.searchInput.value;
  el.searchClear.classList.toggle('on', state.search.length > 0);
  renderLibrary();
});
el.searchClear.addEventListener('click', () => {
  state.search = '';
  el.searchInput.value = '';
  el.searchClear.classList.remove('on');
  el.searchInput.focus();
  renderLibrary();
});

[el.vtGrid, el.vtTable].forEach((b) => b.addEventListener('click', () => {
  patchSettings({ view: b.dataset.view });
  renderLibrary();
}));

/* ------------------------------------------- bottom-right popup toast */

function hideToast() {
  clearTimeout(state.toastTimer);
  state.toastMode = null;
  el.toast.classList.remove('on', 'done');
}

function renderUpdateToast() {
  // Startup splash handles launch-time updates directly on full screen; don't show toast on top of splash.
  if (state.splashActive) return;

  const u = state.update;

  // 5-minute background check detected a launcher update -> bottom-right popup!
  if (state.updateMode === 'background' && u.state === 'available' && u.release) {
    if (state.bgToastDismissed === u.release.version) return;
    clearTimeout(state.toastTimer);
    state.toastMode = 'launcher-bg';
    el.toast.classList.remove('done');
    el.toast.classList.add('on');
    el.toastTitle.textContent = 'Launcher update available';
    el.toastDesc.textContent = 'A newer version of Launcher is ready to download and install.';
    el.toastProg.classList.remove('on');
    el.toastActs.classList.add('on');
    el.toastLater.textContent = 'Dismiss';
    el.toastNow.textContent = 'Update now';
    return;
  }

  // Downloading a launcher update triggered from the background popup
  if (state.updateMode === 'background' && u.state === 'downloading') {
    clearTimeout(state.toastTimer);
    state.toastMode = 'launcher-dl';
    el.toast.classList.remove('done');
    el.toast.classList.add('on');
    el.toastTitle.textContent = 'Downloading update';
    el.toastDesc.textContent = u.total ? `${pct(u)}% of ${fmtMB(u.total)}` : 'Starting download…';
    el.toastProg.classList.add('on');
    el.toastBar.style.width = `${pct(u)}%`;
    el.toastActs.classList.remove('on');
    return;
  }

  // Downloaded & verified -> ask to restart now or later (no surprise reboot while in app!)
  if (state.updateMode === 'background' && u.state === 'ready' && !state.deferred) {
    clearTimeout(state.toastTimer);
    state.toastMode = 'launcher-ready';
    el.toast.classList.add('on', 'done');
    el.toastTitle.textContent = 'Update ready to install';
    el.toastDesc.textContent = 'Restart now to finish updating, or it will install when you close Launcher.';
    el.toastProg.classList.remove('on');
    el.toastActs.classList.add('on');
    el.toastLater.textContent = 'Later';
    el.toastNow.textContent = 'Restart now';
    return;
  }

  if (state.toastMode && state.toastMode.startsWith('launcher-')) {
    hideToast();
  }
}

function showAppUpdateToast(newlyUpdated) {
  if (!newlyUpdated.length || !state.settings.notifyAppUpdates) return;
  if (state.toastMode && state.toastMode.startsWith('launcher-')) return;

  clearTimeout(state.toastTimer);
  state.toastMode = 'app-bg';
  const first = newlyUpdated[0];
  state.toastTargetTab = tabOf(first);

  el.toast.classList.remove('done');
  el.toast.classList.add('on');
  if (newlyUpdated.length === 1) {
    el.toastTitle.textContent = `Update available: ${first.name}`;
    el.toastDesc.textContent = `v${first.latest.version} is ready to install.`;
  } else {
    el.toastTitle.textContent = `${newlyUpdated.length} app updates available`;
    el.toastDesc.textContent = `${newlyUpdated.map((a) => a.name).slice(0, 3).join(', ')} can be updated.`;
  }
  el.toastProg.classList.remove('on');
  el.toastActs.classList.add('on');
  el.toastLater.textContent = 'Dismiss';
  el.toastNow.textContent = 'View';
}

function flashToast(title, desc) {
  if (state.toastMode && state.toastMode.startsWith('launcher-')) return;
  clearTimeout(state.toastTimer);
  state.toastMode = 'flash';
  el.toast.classList.add('on', 'done');
  el.toastTitle.textContent = title;
  el.toastDesc.textContent = desc;
  el.toastProg.classList.remove('on');
  el.toastActs.classList.remove('on');
  state.toastTimer = setTimeout(hideToast, 4200);
}

el.toastNow.addEventListener('click', () => {
  if (state.toastMode === 'launcher-bg') {
    post({ cmd: 'downloadUpdate' });
  } else if (state.toastMode === 'launcher-ready') {
    post({ cmd: 'restartNow' });
  } else if (state.toastMode === 'app-bg') {
    if (state.toastTargetTab) setTab(state.toastTargetTab);
    hideToast();
  }
});

el.toastLater.addEventListener('click', () => {
  if (state.toastMode === 'launcher-bg' && state.update.release) {
    state.bgToastDismissed = state.update.release.version;
    hideToast();
    renderBanner();
  } else if (state.toastMode === 'launcher-ready') {
    state.deferred = true;
    hideToast();
    renderUpdate();
  } else {
    hideToast();
  }
});

function checkBackgroundAppUpdates() {
  const stillChecking = library.apps.some((a) => a.task.state === 'checking');
  if (stillChecking) return;

  // Record initial updates silently on startup (no popup flood on boot)
  if (!state.initialAppsSettled) {
    state.initialAppsSettled = true;
    for (const a of library.apps) {
      if (a.installed && a.updateAvailable && a.latest) {
        state.notifiedAppVers.add(`${a.id}:${a.latest.version}`);
      }
    }
    return;
  }

  if (!state.bgCheckPending) return;
  state.bgCheckPending = false;

  const newlyFound = [];
  for (const a of library.apps) {
    if (a.installed && a.updateAvailable && a.latest) {
      const key = `${a.id}:${a.latest.version}`;
      if (!state.notifiedAppVers.has(key)) {
        state.notifiedAppVers.add(key);
        newlyFound.push(a);
      }
    }
  }
  if (newlyFound.length) {
    showAppUpdateToast(newlyFound);
  }
}

/* -------------------------------------------------------------- host */

let menuCheck = null;

function reportMenuCheck() {
  if (menuCheck === 'launcher') {
    if (!['upToDate', 'available', 'ready', 'error'].includes(state.update.state)) return;
    menuCheck = null;
    const d = describe(state.update);
    flashToast(d.title, d.detail);
  } else if (menuCheck === 'apps') {
    if (library.apps.some((a) => a.task.state === 'checking')) return;
    menuCheck = null;
    const n = library.apps.filter((a) => a.updateAvailable).length;
    if (n) flashToast(`${n} app update${n > 1 ? 's' : ''} available`, 'Press Update on each one to install it.');
    else flashToast('Your apps are up to date', 'Everything installed is on its newest release.');
  }
}

window.onHost = (msg) => {
  switch (msg.type) {
    case 'settings':
      state.settings = Object.assign({}, state.settings, msg.settings);
      renderSettings();
      renderLibrary();
      break;
    case 'update':
      state.update = msg.update;
      state.updateMode = msg.mode || (msg.auto ? 'startup' : 'manual');
      renderUpdate();
      reportMenuCheck();
      break;
    case 'bgCheck':
      state.bgCheckPending = true;
      break;
    case 'window':
      el.root.classList.toggle('maximized', !!msg.maximized);
      break;
    case 'menuCheck':
      if (msg.page === 'settings') openSettings();
      else setTab(msg.page || 'library');
      menuCheck = msg.check;
      break;
    case 'apps':
      library.apps = msg.apps;
      renderLibrary();
      renderActivity();
      checkBackgroundAppUpdates();
      reportMenuCheck();
      break;
  }
};

/* ------------------------------------------------------------ window */

document.getElementById('top').addEventListener('mousedown', (e) => {
  if (e.button !== 0 || e.detail > 1) return;
  if (e.target.closest('button, input, a, nav, .popover')) return;
  post({ cmd: 'window', action: 'drag' });
});

document.getElementById('top').addEventListener('dblclick', (e) => {
  if (e.target.closest('button, input, a, nav, .popover')) return;
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

document.addEventListener('click', (e) => {
  if (!e.target.closest('#card-menu, .menu-btn')) closeCardMenu();
  if (!e.target.closest('.bell-wrap')) closeBell();
});

document.addEventListener('contextmenu', (e) => {
  if (!e.target.closest('input[type=text], .notes-body, .log-body')) e.preventDefault();
});

document.addEventListener('keydown', (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key === ',') {
    e.preventDefault();
    state.settingsOpen ? closeSettings() : openSettings();
  } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') {
    e.preventDefault();
    if (state.settingsOpen) closeSettings();
    el.searchInput.focus();
    el.searchInput.select();
  } else if (e.key === 'Escape') {
    if (el.cardMenu.classList.contains('on')) { closeCardMenu(); return; }
    if (el.bellPopover.classList.contains('on')) { closeBell(); return; }
    if (state.settingsOpen) { closeSettings(); return; }
    if (document.activeElement === el.searchInput && state.search) {
      state.search = '';
      el.searchInput.value = '';
      el.searchClear.classList.remove('on');
      renderLibrary();
    }
  }
});

/* -------------------------------------------------------------- boot */

renderSettings();
renderUpdate();
if (state.splashActive) startSplash();

if (BOOT.justUpdated && !state.splashActive) {
  flashToast('Launcher updated', 'You’re running the newest release.');
}

let readySent = false;
const ready = () => {
  if (readySent) return;
  readySent = true;
  post({ cmd: 'ready' });
};
requestAnimationFrame(ready);
setTimeout(ready, 50);
