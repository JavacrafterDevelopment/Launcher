/* ============================================================================
   Launcher front-end.

   The Rust host owns all state: settings, self-update, and the app library.
   The page sends commands through window.ipc and the host answers by calling
   window.onHost with whole snapshots.
   ========================================================================== */

const BOOT = window.BOOT || {
  // Opened as a plain file (browser preview): stand in for the host.
  version: '0.3.19',
  repo: 'JavacrafterDevelopment/Launcher',
  githubOrgUrl: 'https://github.com/JavacrafterDevelopment',
  os: /Mac/i.test(navigator.platform || '') ? 'MacOS' : 'Windows',
  settings: {
    theme: 'light',
    look: 'modern',
    language: 'auto',
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
  currentVersion:  $('current-version'),
  last:            $('upd-last'),
  setLanguage:     $('set-language'),
  setCheckEvery:   $('set-check-every'),
  setAuto:         $('set-auto'),
  setSimDl:        $('set-sim-dl'),
  setNotify:       $('set-notify'),
  setPre:          $('set-pre'),
  portableDesc:    $('portable-desc'),
  installDisplay:  $('install-folder-display'),
  btnBrowseInst:   $('btn-browse-install'),
  btnDefaultInst:  $('btn-default-install'),
  btnData:         $('btn-data'),
  lookButtons:     document.querySelectorAll('#look-group .cell'),
  themeButtons:    document.querySelectorAll('#theme-group .cell'),
  // Custom GitHub Apps & Private Repo Picker
  ghRepoUrl:       $('gh-repo-url'),
  btnGhAddRepo:    $('btn-gh-add-repo'),
  ghRepoMsg:       $('gh-repo-msg'),
  ghPrivWrap:      $('gh-private-repos-wrap'),
  ghPrivFilter:    $('gh-private-filter'),
  btnGhRefRepos:   $('btn-gh-refresh-repos'),
  ghPrivList:      $('gh-private-repos-list'),
  customAppsWrap:  $('custom-apps-list-wrap'),
  customAppsCount: $('custom-apps-count'),
  customAppsList:  $('custom-apps-list'),
  // GitHub Account & OS Credential Store
  ghAuthPill:      $('gh-auth-pill'),
  ghAuthPillText:  $('gh-auth-pill-text'),
  ghSignedIn:      $('gh-auth-signed-in'),
  ghSignedOut:     $('gh-auth-signed-out'),
  ghUserAvatar:    $('gh-user-avatar'),
  ghUserName:      $('gh-user-name'),
  btnGhLogout:     $('btn-gh-logout'),
  linkGhTokens:    $('link-gh-tokens'),
  ghPatInput:      $('gh-pat-input'),
  btnGhPat:        $('btn-gh-pat'),
  ghAuthMsg:       $('gh-auth-msg'),
  ghStoreName:     $('gh-store-name'),
  ghCredCount:     $('gh-cred-count'),
  btnGhRefCreds:   $('btn-gh-refresh-creds'),
  ghCredList:      $('gh-cred-list'),
  btnGhClearOld:   $('btn-gh-clear-old'),
  btnGhClearAll:   $('btn-gh-clear-all'),
  // Category Picker Modal Popup
  addAppModal:     $('add-app-modal'),
  addAppBack:      $('add-app-backdrop'),
  addAppClose:     $('add-app-close'),
  addAppSlug:      $('add-app-repo-slug'),
  addAppMeta:      $('add-app-meta'),
  addAppName:      $('add-app-name'),
  addAppCats:      document.querySelectorAll('#add-app-categories .cat-choice'),
  addAppCancel:    $('add-app-cancel'),
  addAppConfirm:   $('add-app-confirm'),
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
    language: 'auto',
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
    customApps: [],
    githubUser: null,
    githubAvatar: null,
  }, BOOT.settings || {}),
  auth:             Object.assign({
    loggedIn: false,
    username: null,
    avatarUrl: null,
    storeName: CURRENT_OS === 'MacOS' ? 'macOS Keychain' : 'Windows Credential Manager',
    currentKey: '',
  }, BOOT.auth || {}),
  authBusy:         false,
  authMsg:          null,           // { kind: 'ok'|'bad'|'info', text }
  credentials:      [],
  credsLoaded:      false,
  clearAllArmed:    false,
  clearAllTimer:    null,
  privateRepos:     [],
  privLoading:      false,
  privError:        null,
  privFilter:       '',
  repoBusy:         false,
  repoMsg:          null,           // { kind: 'ok'|'bad'|'info', text }
  pendingRepo:      null,           // RepoPreview currently open in category modal
  pendingCategory:  'apps',         // 'apps' | 'temporary' | 'games'
  removeArmedId:    null,
  removeArmedTimer: null,
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

/* -------------------------------------------------------------- i18n */

const SUPPORTED_LANGS = ['en', 'es', 'ru'];

const I18N = {
  ru: {
    'Checking for updates…': 'Проверка обновлений…',
    'Skip for now': 'Пропустить',
    'Update found — starting download…': 'Найдено обновление — начинаем скачивание…',
    'Downloading update — {pct}% ({done} of {total})': 'Скачивание обновления — {pct}% ({done} из {total})',
    'Downloading update…': 'Скачивание обновления…',
    'Installing update & restarting…': 'Установка обновления и перезапуск…',
    'Javacrafter App Suite': 'Набор приложений Javacrafter',
    'Your apps': 'Ваши приложения',
    'Apps': 'Приложения',
    'App updates available': 'Доступны обновления приложений',
    'Short-lived tools': 'Временные утилиты',
    'Temporary Apps': 'Временные',
    'Your games': 'Ваши игры',
    'Games': 'Игры',
    'Game updates available': 'Доступны обновления игр',
    'Settings (Ctrl+,)': 'Настройки (Ctrl+,)',
    'Settings': 'Настройки',
    'Updates & notices': 'Обновления и уведомления',
    'Updates': 'Обновления',
    'Available updates': 'Доступные обновления',
    'Close': 'Закрыть',
    'Minimize': 'Свернуть',
    'Maximize': 'Развернуть',
    'UPDATE': 'ОБНОВЛЕНИЕ',
    'Update & restart': 'Обновить и перезапустить',
    'Dismiss': 'Закрыть',
    'Check for updates': 'Проверить обновления',
    'Update all': 'Обновить все',
    'Update 1 app': 'Обновить 1 приложение',
    'Update all ({count})': 'Обновить все ({count})',
    'Sort': 'Сортировка',
    'Sort apps': 'Сортировка приложений',
    'Name (A–Z)': 'Имя (А–Я)',
    'Name (Z–A)': 'Имя (Я–А)',
    'Status': 'Статус',
    'Recently updated': 'Недавно обновлённые',
    'Search apps': 'Поиск приложений',
    'Clear search': 'Очистить поиск',
    'Show apps as cards': 'Показывать приложения карточками',
    'Grid': 'Сетка',
    'Show apps as a table': 'Показывать приложения таблицей',
    'Table': 'Таблица',
    'Language': 'Язык',
    'Auto': 'Авто',
    'Open the releases page on GitHub': 'Открыть страницу релизов на GitHub',
    'Automatically': 'Автоматически',
    'Only when I ask': 'Только по запросу',
    'Automatically download and install launcher updates': 'Автоматически скачивать и устанавливать обновления лаунчера',
    'Checks on startup while loading, and updates before the launcher opens.': 'Проверяет при запуске во время загрузки и обновляется до открытия лаунчера.',
    'Simultaneous downloads': 'Одновременные загрузки',
    'No limit': 'Без ограничений',
    'Downloads running together share your connection, so each one is slower.': 'Одновременные загрузки делят ваше подключение, поэтому каждая идёт медленнее.',
    'Notify me when an installed app has an update': 'Уведомлять, когда у установленного приложения есть обновление',
    'Include pre-releases (betas and release candidates)': 'Включать предварительные релизы (бета и RC)',
    'Betas and release candidates can replace your stable installation.': 'Бета-версии и кандидаты в релизы могут заменить стабильную установку.',
    'Not checked': 'Не проверялось',
    'The launcher can check GitHub for a newer release.': 'Лаунчер может проверить наличие новой версии на GitHub.',
    'View on GitHub': 'Открыть на GitHub',
    'Download & install': 'Скачать и установить',
    'Restart to update': 'Перезапустить для обновления',
    'Current Version': 'Текущая версия',
    'Portable': 'Портативность',
    'Everything stays in this folder': 'Всё хранится в этой папке',
    'Open folder': 'Открыть папку',
    'Default Install Folder': 'Папка установки по умолчанию',
    'Install folder': 'Папка установки',
    'Apps are placed inside this folder.': 'Приложения размещаются внутри этой папки.',
    'Browse…': 'Обзор…',
    'Use default': 'По умолчанию',
    'New installs use this folder. Updates stay in their current location.': 'Новые установки используют эту папку. Обновления остаются на прежнем месте.',
    'Appearance': 'Внешний вид',
    'Theme': 'Оформление',
    'Switch between the crisp Modern layout and the classic Legacy look.': 'Переключайтесь между современным макетом и классическим оформлением.',
    'Modern': 'Современная',
    'Legacy': 'Классическая',
    'Color mode': 'Цветовая тема',
    'Follow your operating system, or pick Light or Dark.': 'Следуйте настройкам системы или выберите светлую или тёмную тему.',
    'Match system': 'Как в системе',
    'Light': 'Светлая',
    'Dark': 'Тёмная',
    'Light mode colours': 'Цвета светлой темы',
    'Used in Legacy look while light mode is active.': 'Используются в классическом оформлении при светлой теме.',
    'Dark mode colours': 'Цвета тёмной темы',
    'Used in Legacy look while dark mode is active.': 'Используются в классическом оформлении при тёмной теме.',
    'Walnut & Gilt': 'Орех и позолота',
    'Sage': 'Шалфей',
    'Slate': 'Сланец',
    'Rose': 'Роза',
    'Charcoal & Gold': 'Уголь и золото',
    'Ocean': 'Океан',
    'Forest': 'Лес',
    'Plum': 'Слива',
    'Default': 'По умолчанию',
    '{name} (default)': '{name} (по умолчанию)',
    'Quit Launcher': 'Выйти из Launcher',
    'Done': 'Готово',
    'Update settings': 'Настройки обновлений',
    'Update': 'Обновить',
    'Restart': 'Перезапуск',
    'Details': 'Подробнее',
    'JavacrafterDevelopment on GitHub': 'JavacrafterDevelopment на GitHub',
    'Update now': 'Обновить сейчас',
    'just now': 'только что',
    'yesterday': 'вчера',
    '{n} min ago': '{n} мин. назад',
    '{n} h ago': '{n} ч. назад',
    'Checked {ago}': 'Проверено {ago}',
    'Checking…': 'Проверка…',
    'Asking GitHub for the latest release of {repo}.': 'Запрос последнего релиза {repo} на GitHub.',
    'Up to date': 'Актуальная версия',
    'You are running the newest release on GitHub.': 'У вас установлена последняя версия с GitHub.',
    'Launcher update available': 'Доступно обновление Launcher',
    'Update available': 'Доступно обновление',
    'A newer Launcher release is out ({size}).': 'Вышла новая версия Launcher ({size}).',
    'Downloading update — {pct}%': 'Скачивание обновления — {pct}%',
    'Downloading…': 'Скачивание…',
    '{done} of {total}': '{done} из {total}',
    'Starting the download…': 'Запуск скачивания…',
    'Update installs when you close the launcher': 'Обновление установится при закрытии лаунчера',
    'Update is ready to install': 'Обновление готово к установке',
    'Ready to restart': 'Готово к перезапуску',
    'Downloaded and verified. Restart to switch over, or it installs itself when you close the launcher.': 'Скачано и проверено. Перезапустите сейчас или обновление установится при закрытии лаунчера.',
    'Update failed': 'Ошибка обновления',
    'Queued: {name}': 'В очереди: {name}',
    'Updating {name} — {pct}%': 'Обновление {name} — {pct}%',
    'Updating {name}…': 'Обновление {name}…',
    'Downloading {name} — {pct}%': 'Скачивание {name} — {pct}%',
    'Downloading {name}…': 'Скачивание {name}…',
    'Installing {name}…': 'Установка {name}…',
    'Verifying {name} — {done} of {total} files': 'Проверка {name} — {done} из {total} файлов',
    'Verifying {name}…': 'Проверка {name}…',
    'Repairing {name} — {pct}%': 'Восстановление {name} — {pct}%',
    'Repairing {name}…': 'Восстановление {name}…',
    'Downloading Launcher update — {pct}%': 'Скачивание обновления Launcher — {pct}%',
    '+{n} more': 'ещё +{n}',
    'Checked {ago} · using GitHub': 'Проверено {ago} · через GitHub',
    'Using GitHub': 'Через GitHub',
    'Downloading Launcher update…': 'Скачивание обновления Launcher…',
    '{done} of {total} · {pct}%': '{done} из {total} · {pct}%',
    'Launcher update is ready to install.': 'Обновление Launcher готово к установке.',
    'Restart now': 'Перезапустить',
    'A newer Launcher update is available.': 'Доступна новая версия Launcher.',
    'Pre-release notes': 'Заметки к предварительному релизу',
    'Release notes': 'Заметки к релизу',
    'No release notes.': 'Заметки к релизу отсутствуют.',
    'Everything is up to date.': 'Всё обновлено до последних версий.',
    '{n} installed': '{n} уст.',
    'Pinned': 'Закреплённые',
    'Installed': 'Установлены',
    'status.Installed': 'Установлено',
    'Not installed': 'Не установлены',
    'No matches for “{q}”.': 'Ничего не найдено по запросу «{q}».',
    'No temporary apps right now.': 'Сейчас нет временных приложений.',
    'No games yet. They’ll appear here once they’re added.': 'Игр пока нет. Они появятся здесь после добавления.',
    'No apps found.': 'Приложения не найдены.',
    'Queued · Cancel': 'В очереди · Отмена',
    'Stopping…': 'Остановка…',
    'Stop': 'Стоп',
    'Ask {name} to quit': 'Запросить выход из {name}',
    'Open': 'Открыть',
    'Update {name} to v{version}': 'Обновить {name} до v{version}',
    'Starting…': 'Запуск…',
    '{platforms} only': 'Только {platforms}',
    'Not on {os}': 'Нет для {os}',
    'Install': 'Установить',
    'Queued': 'В очереди',
    'Installing…': 'Установка…',
    'Verifying…': 'Проверка…',
    'Repairing…': 'Восстановление…',
    'Error': 'Ошибка',
    'Running': 'Запущено',
    'Available to install': 'Доступно для установки',
    'No release yet': 'Релизов пока нет',
    'No release': 'Нет релиза',
    'Unavailable': 'Недоступно',
    'Downloading {done} of {total} · {pct}%': 'Скачивание {done} из {total} · {pct}%',
    'Verifying files and installing…': 'Проверка файлов и установка…',
    'Waiting for another download to finish…': 'Ожидание завершения другой загрузки…',
    'Verifying {done} of {total} files': 'Проверка {done} из {total} файлов',
    'Repairing {pct}% of {total}': 'Восстановление {pct}% из {total}',
    'Reading package index…': 'Чтение индекса пакета…',
    'Log': 'Журнал',
    'Hide log': 'Скрыть журнал',
    'Show log': 'Показать журнал',
    'by {name}': 'от {name}',
    'Not portable': 'Не портативно',
    'Keeps its settings and data in {folder}Data, inside its own folder.': 'Хранит настройки и данные в {folder}Data внутри собственной папки.',
    'Keeps its settings in ~/Library/Application Support/{name}, outside its own folder.': 'Хранит настройки в ~/Library/Application Support/{name}, вне собственной папки.',
    'Download size: {size}': 'Размер загрузки: {size}',
    'Last updated: {date}': 'Обновлено: {date}',
    'Unpin {name}': 'Открепить {name}',
    'Pin {name} to top': 'Закрепить {name} сверху',
    'More actions for {name}': 'Другие действия для {name}',
    'App': 'Приложение',
    'Version': 'Версия',
    'Released': 'Дата релиза',
    'Author': 'Автор',
    'Launch with .bat': 'Запустить через .bat',
    'Open install folder': 'Открыть папку установки',
    'Verify & repair files': 'Проверить и восстановить файлы',
    'Release notes ↗': 'Заметки к релизу ↗',
    'View on GitHub ↗': 'Открыть на GitHub ↗',
    'Hide activity log': 'Скрыть журнал действий',
    'Show activity log': 'Показать журнал действий',
    'Click again to move to Trash': 'Нажмите ещё раз, чтобы переместить в Корзину',
    'Click again to delete app & data': 'Нажмите ещё раз, чтобы удалить приложение и данные',
    'Uninstall': 'Удалить',
    'A newer version of Launcher is ready to download and install.': 'Новая версия Launcher готова к скачиванию и установке.',
    'Downloading update': 'Скачивание обновления',
    '{pct}% of {total}': '{pct}% из {total}',
    'Starting download…': 'Запуск скачивания…',
    'Update ready to install': 'Обновление готово к установке',
    'Restart now to finish updating, or it will install when you close Launcher.': 'Перезапустите сейчас или обновление установится при закрытии Launcher.',
    'Later': 'Позже',
    'Update available: {name}': 'Доступно обновление: {name}',
    'v{version} is ready to install.': 'Версия v{version} готова к установке.',
    '{names} can be updated.': 'Можно обновить: {names}.',
    'View': 'Показать',
    'Press Update on each one to install it.': 'Нажмите «Обновить» на каждом, чтобы установить.',
    'Your apps are up to date': 'Все приложения актуальны',
    'Everything installed is on its newest release.': 'Все установленные приложения обновлены до последних версий.',
    'Launcher updated': 'Launcher обновлён',
    'You’re running the newest release.': 'У вас установлена новейшая версия.',
    'Add App from GitHub': 'Добавить приложение с GitHub',
    'Installs & updates from GitHub releases': 'Установка и обновления из релизов GitHub',
    'GitHub repository URL': 'Ссылка на репозиторий GitHub',
    'Add app': 'Добавить',
    'Paste any public or private GitHub repository link to add it to your launcher with automatic update checks.': 'Вставьте ссылку на любой публичный или приватный репозиторий GitHub, чтобы добавить его с автоматической проверкой обновлений.',
    'Your Private Repositories': 'Ваши приватные репозитории',
    'Refresh': 'Обновить',
    'Filter private repositories…': 'Фильтр приватных репозиториев…',
    'Added GitHub Apps': 'Добавленные приложения GitHub',
    'GitHub Account': 'Аккаунт GitHub',
    'Not signed in': 'Вход не выполнен',
    'Signed in': 'Вход выполнен',
    'Signing in…': 'Выполняется вход…',
    'Private repositories enabled · 5,000 GitHub requests/hour': 'Доступ к приватным репозиториям включён · 5 000 запросов/час',
    'Sign out': 'Выйти',
    'Sign in with a Personal Access Token to pull private repositories and raise GitHub\'s rate limit. Tokens are stored securely in your OS credential store.': 'Войдите с помощью персонального токена доступа, чтобы загружать приватные репозитории и увеличить лимит запросов GitHub. Токены безопасно хранятся в системном хранилище учётных данных.',
    'Personal Access Token': 'Персональный токен',
    'Cancel': 'Отмена',
    'Create token on GitHub ↗': 'Создать токен на GitHub ↗',
    'Connect token': 'Подключить токен',
    'Use a classic token with \'repo\' scope, or a fine-grained token with \'Contents: Read-only\' access.': 'Используйте классический токен с правом «repo» или fine-grained токен с доступом «Contents: Read-only».',
    'Manage credentials saved by this copy or older/deleted copies of Launcher.': 'Управление учётными данными, сохранёнными этой или старыми/удалёнными копиями Launcher.',
    'Clear old copies': 'Очистить старые копии',
    'Clear old copies ({n})': 'Очистить старые копии ({n})',
    'Clear all from credential store': 'Очистить все из хранилища',
    'Click again to clear all credentials': 'Нажмите ещё раз, чтобы очистить все учётные данные',
    'This copy': 'Эта копия',
    'Deleted copy': 'Удалённая копия',
    'Old / other copy': 'Старая / другая копия',
    'Delete': 'Удалить',
    'Private': 'Приватный',
    'Public': 'Публичный',
    'Add': 'Добавить',
    'Added': 'Добавлено',
    'Remove': 'Убрать',
    'Confirm': 'Подтвердить',
    'Remove from Launcher': 'Убрать из Launcher',
    'Click again to remove from Launcher': 'Нажмите ещё раз, чтобы убрать из Launcher',
    'Can’t remove {name}': 'Нельзя убрать {name}',
    '{name} is still installed. Uninstall it first before removing it from Launcher.': '{name} всё ещё установлено. Сначала удалите приложение, прежде чем убирать его из Launcher.',
    'Private apps removed': 'Приватные приложения убраны',
    'Removed uninstalled private app {name} because your GitHub token was removed.': 'Неустановленное приватное приложение {name} убрано, так как ваш токен GitHub был удалён.',
    'Removed {n} uninstalled private apps ({names}) because your GitHub token was removed.': 'Убрано неустановленных приватных приложений: {n} ({names}), так как ваш токен GitHub был удалён.',
    'Add Repository to Launcher': 'Добавить репозиторий в Launcher',
    'Display name': 'Отображаемое имя',
    'Where should this appear?': 'Где должно отображаться?',
    'Main app library': 'Основная библиотека приложений',
    'Short-lived tools & utilities': 'Временные инструменты и утилиты',
    'Games library': 'Библиотека игр',
    'Add to Launcher': 'Добавить в Launcher',
  },
  es: {
    'Checking for updates…': 'Buscando actualizaciones…',
    'Skip for now': 'Omitir por ahora',
    'Update found — starting download…': 'Actualización encontrada — iniciando descarga…',
    'Downloading update — {pct}% ({done} of {total})': 'Descargando actualización — {pct}% ({done} de {total})',
    'Downloading update…': 'Descargando actualización…',
    'Installing update & restarting…': 'Instalando actualización y reiniciando…',
    'Javacrafter App Suite': 'Suite de aplicaciones Javacrafter',
    'Your apps': 'Tus aplicaciones',
    'Apps': 'Aplicaciones',
    'App updates available': 'Actualizaciones de aplicaciones disponibles',
    'Short-lived tools': 'Herramientas temporales',
    'Temporary Apps': 'Temporales',
    'Your games': 'Tus juegos',
    'Games': 'Juegos',
    'Game updates available': 'Actualizaciones de juegos disponibles',
    'Settings (Ctrl+,)': 'Ajustes (Ctrl+,)',
    'Settings': 'Ajustes',
    'Updates & notices': 'Actualizaciones y avisos',
    'Updates': 'Actualizaciones',
    'Available updates': 'Actualizaciones disponibles',
    'Close': 'Cerrar',
    'Minimize': 'Minimizar',
    'Maximize': 'Maximizar',
    'UPDATE': 'ACTUALIZACIÓN',
    'Update & restart': 'Actualizar y reiniciar',
    'Dismiss': 'Descartar',
    'Check for updates': 'Buscar actualizaciones',
    'Update all': 'Actualizar todas',
    'Update 1 app': 'Actualizar 1 aplicación',
    'Update all ({count})': 'Actualizar todas ({count})',
    'Sort': 'Ordenar',
    'Sort apps': 'Ordenar aplicaciones',
    'Name (A–Z)': 'Nombre (A–Z)',
    'Name (Z–A)': 'Nombre (Z–A)',
    'Status': 'Estado',
    'Recently updated': 'Actualizadas recientemente',
    'Search apps': 'Buscar aplicaciones',
    'Clear search': 'Borrar búsqueda',
    'Show apps as cards': 'Mostrar las aplicaciones como tarjetas',
    'Grid': 'Cuadrícula',
    'Show apps as a table': 'Mostrar las aplicaciones como tabla',
    'Table': 'Tabla',
    'Language': 'Idioma',
    'Auto': 'Automático',
    'Open the releases page on GitHub': 'Abrir la página de versiones en GitHub',
    'Automatically': 'Automáticamente',
    'Only when I ask': 'Solo cuando lo pida',
    'Automatically download and install launcher updates': 'Descargar e instalar actualizaciones del lanzador automáticamente',
    'Checks on startup while loading, and updates before the launcher opens.': 'Comprueba al iniciar mientras carga y actualiza antes de abrir el lanzador.',
    'Simultaneous downloads': 'Descargas simultáneas',
    'No limit': 'Sin límite',
    'Downloads running together share your connection, so each one is slower.': 'Las descargas simultáneas comparten tu conexión, así que cada una es más lenta.',
    'Notify me when an installed app has an update': 'Avisarme cuando una aplicación instalada tenga una actualización',
    'Include pre-releases (betas and release candidates)': 'Incluir versiones preliminares (betas y versiones candidatas)',
    'Betas and release candidates can replace your stable installation.': 'Las betas y versiones candidatas pueden reemplazar tu instalación estable.',
    'Not checked': 'Sin comprobar',
    'The launcher can check GitHub for a newer release.': 'El lanzador puede comprobar en GitHub si hay una versión más reciente.',
    'View on GitHub': 'Ver en GitHub',
    'Download & install': 'Descargar e instalar',
    'Restart to update': 'Reiniciar para actualizar',
    'Current Version': 'Versión actual',
    'Portable': 'Portátil',
    'Everything stays in this folder': 'Todo permanece en esta carpeta',
    'Open folder': 'Abrir carpeta',
    'Default Install Folder': 'Carpeta de instalación predeterminada',
    'Install folder': 'Carpeta de instalación',
    'Apps are placed inside this folder.': 'Las aplicaciones se guardan dentro de esta carpeta.',
    'Browse…': 'Examinar…',
    'Use default': 'Usar predeterminada',
    'New installs use this folder. Updates stay in their current location.': 'Las instalaciones nuevas usan esta carpeta. Las actualizaciones se mantienen en su ubicación actual.',
    'Appearance': 'Apariencia',
    'Theme': 'Estilo',
    'Switch between the crisp Modern layout and the classic Legacy look.': 'Cambia entre el diseño moderno y el aspecto clásico.',
    'Modern': 'Moderno',
    'Legacy': 'Clásico',
    'Color mode': 'Modo de color',
    'Follow your operating system, or pick Light or Dark.': 'Sigue el tema de tu sistema operativo o elige Claro u Oscuro.',
    'Match system': 'Igual que el sistema',
    'Light': 'Claro',
    'Dark': 'Oscuro',
    'Light mode colours': 'Colores del modo claro',
    'Used in Legacy look while light mode is active.': 'Usados en el aspecto clásico cuando el modo claro está activo.',
    'Dark mode colours': 'Colores del modo oscuro',
    'Used in Legacy look while dark mode is active.': 'Usados en el aspecto clásico cuando el modo oscuro está activo.',
    'Walnut & Gilt': 'Nogal y dorado',
    'Sage': 'Salvia',
    'Slate': 'Pizarra',
    'Rose': 'Rosa',
    'Charcoal & Gold': 'Carbón y oro',
    'Ocean': 'Océano',
    'Forest': 'Bosque',
    'Plum': 'Ciruela',
    'Default': 'Predeterminado',
    '{name} (default)': '{name} (predeterminado)',
    'Quit Launcher': 'Salir de Launcher',
    'Done': 'Listo',
    'Update settings': 'Ajustes de actualizaciones',
    'Update': 'Actualizar',
    'Restart': 'Reiniciar',
    'Details': 'Detalles',
    'JavacrafterDevelopment on GitHub': 'JavacrafterDevelopment en GitHub',
    'Update now': 'Actualizar ahora',
    'just now': 'ahora mismo',
    'yesterday': 'ayer',
    '{n} min ago': 'hace {n} min',
    '{n} h ago': 'hace {n} h',
    'Checked {ago}': 'Comprobado {ago}',
    'Checking…': 'Comprobando…',
    'Asking GitHub for the latest release of {repo}.': 'Consultando en GitHub la última versión de {repo}.',
    'Up to date': 'Al día',
    'You are running the newest release on GitHub.': 'Tienes la versión más reciente de GitHub.',
    'Launcher update available': 'Actualización de Launcher disponible',
    'Update available': 'Actualización disponible',
    'A newer Launcher release is out ({size}).': 'Hay una nueva versión de Launcher ({size}).',
    'Downloading update — {pct}%': 'Descargando actualización — {pct}%',
    'Downloading…': 'Descargando…',
    '{done} of {total}': '{done} de {total}',
    'Starting the download…': 'Iniciando descarga…',
    'Update installs when you close the launcher': 'La actualización se instalará al cerrar el lanzador',
    'Update is ready to install': 'Actualización lista para instalar',
    'Ready to restart': 'Listo para reiniciar',
    'Downloaded and verified. Restart to switch over, or it installs itself when you close the launcher.': 'Descargada y verificada. Reinicia ahora o se instalará al cerrar el lanzador.',
    'Update failed': 'Error de actualización',
    'Queued: {name}': 'En cola: {name}',
    'Updating {name} — {pct}%': 'Actualizando {name} — {pct}%',
    'Updating {name}…': 'Actualizando {name}…',
    'Downloading {name} — {pct}%': 'Descargando {name} — {pct}%',
    'Downloading {name}…': 'Descargando {name}…',
    'Installing {name}…': 'Instalando {name}…',
    'Verifying {name} — {done} of {total} files': 'Verificando {name} — {done} de {total} archivos',
    'Verifying {name}…': 'Verificando {name}…',
    'Repairing {name} — {pct}%': 'Reparando {name} — {pct}%',
    'Repairing {name}…': 'Reparando {name}…',
    'Downloading Launcher update — {pct}%': 'Descargando actualización de Launcher — {pct}%',
    '+{n} more': '+{n} más',
    'Checked {ago} · using GitHub': 'Comprobado {ago} · con GitHub',
    'Using GitHub': 'Con GitHub',
    'Downloading Launcher update…': 'Descargando actualización de Launcher…',
    '{done} of {total} · {pct}%': '{done} de {total} · {pct}%',
    'Launcher update is ready to install.': 'La actualización de Launcher está lista para instalar.',
    'Restart now': 'Reiniciar ahora',
    'A newer Launcher update is available.': 'Hay una actualización más reciente de Launcher.',
    'Pre-release notes': 'Notas de la versión preliminar',
    'Release notes': 'Notas de la versión',
    'No release notes.': 'Sin notas de la versión.',
    'Everything is up to date.': 'Todo está actualizado.',
    '{n} installed': '{n} inst.',
    'Pinned': 'Fijadas',
    'Installed': 'Instaladas',
    'status.Installed': 'Instalada',
    'Not installed': 'No instaladas',
    'No matches for “{q}”.': 'No hay resultados para «{q}».',
    'No temporary apps right now.': 'No hay aplicaciones temporales ahora mismo.',
    'No games yet. They’ll appear here once they’re added.': 'Aún no hay juegos. Aparecerán aquí cuando se añadan.',
    'No apps found.': 'No se encontraron aplicaciones.',
    'Queued · Cancel': 'En cola · Cancelar',
    'Stopping…': 'Deteniendo…',
    'Stop': 'Detener',
    'Ask {name} to quit': 'Pedir a {name} que se cierre',
    'Open': 'Abrir',
    'Update {name} to v{version}': 'Actualizar {name} a v{version}',
    'Starting…': 'Iniciando…',
    '{platforms} only': 'Solo {platforms}',
    'Not on {os}': 'No en {os}',
    'Install': 'Instalar',
    'Queued': 'En cola',
    'Installing…': 'Instalando…',
    'Verifying…': 'Verificando…',
    'Repairing…': 'Reparando…',
    'Error': 'Error',
    'Running': 'En ejecución',
    'Available to install': 'Disponible para instalar',
    'No release yet': 'Aún sin versión',
    'No release': 'Sin versión',
    'Unavailable': 'No disponible',
    'Downloading {done} of {total} · {pct}%': 'Descargando {done} de {total} · {pct}%',
    'Verifying files and installing…': 'Verificando archivos e instalando…',
    'Waiting for another download to finish…': 'Esperando a que termine otra descarga…',
    'Verifying {done} of {total} files': 'Verificando {done} de {total} archivos',
    'Repairing {pct}% of {total}': 'Reparando {pct}% de {total}',
    'Reading package index…': 'Leyendo el índice del paquete…',
    'Log': 'Registro',
    'Hide log': 'Ocultar registro',
    'Show log': 'Mostrar registro',
    'by {name}': 'por {name}',
    'Not portable': 'No portátil',
    'Keeps its settings and data in {folder}Data, inside its own folder.': 'Guarda sus ajustes y datos en {folder}Data, dentro de su propia carpeta.',
    'Keeps its settings in ~/Library/Application Support/{name}, outside its own folder.': 'Guarda sus ajustes en ~/Library/Application Support/{name}, fuera de su propia carpeta.',
    'Download size: {size}': 'Tamaño de descarga: {size}',
    'Last updated: {date}': 'Última actualización: {date}',
    'Unpin {name}': 'Quitar fijación de {name}',
    'Pin {name} to top': 'Fijar {name} arriba',
    'More actions for {name}': 'Más acciones para {name}',
    'App': 'Aplicación',
    'Version': 'Versión',
    'Released': 'Publicada',
    'Author': 'Autor',
    'Launch with .bat': 'Iniciar con .bat',
    'Open install folder': 'Abrir carpeta de instalación',
    'Verify & repair files': 'Verificar y reparar archivos',
    'Release notes ↗': 'Notas de la versión ↗',
    'View on GitHub ↗': 'Ver en GitHub ↗',
    'Hide activity log': 'Ocultar registro de actividad',
    'Show activity log': 'Mostrar registro de actividad',
    'Click again to move to Trash': 'Haz clic de nuevo para mover a la Papelera',
    'Click again to delete app & data': 'Haz clic de nuevo para eliminar la app y sus datos',
    'Uninstall': 'Desinstalar',
    'A newer version of Launcher is ready to download and install.': 'Hay una versión más reciente de Launcher lista para descargar e instalar.',
    'Downloading update': 'Descargando actualización',
    '{pct}% of {total}': '{pct}% de {total}',
    'Starting download…': 'Iniciando descarga…',
    'Update ready to install': 'Actualización lista para instalar',
    'Restart now to finish updating, or it will install when you close Launcher.': 'Reinicia ahora para terminar, o se instalará al cerrar Launcher.',
    'Later': 'Más tarde',
    'Update available: {name}': 'Actualización disponible: {name}',
    'v{version} is ready to install.': 'v{version} está lista para instalar.',
    '{names} can be updated.': '{names} se pueden actualizar.',
    'View': 'Ver',
    'Press Update on each one to install it.': 'Pulsa Actualizar en cada una para instalarla.',
    'Your apps are up to date': 'Tus aplicaciones están al día',
    'Everything installed is on its newest release.': 'Todo lo instalado está en su versión más reciente.',
    'Launcher updated': 'Launcher actualizado',
    'You’re running the newest release.': 'Estás usando la versión más reciente.',
    'Add App from GitHub': 'Añadir aplicación desde GitHub',
    'Installs & updates from GitHub releases': 'Instala y actualiza desde lanzamientos de GitHub',
    'GitHub repository URL': 'URL del repositorio de GitHub',
    'Add app': 'Añadir app',
    'Paste any public or private GitHub repository link to add it to your launcher with automatic update checks.': 'Pega cualquier enlace de repositorio público o privado de GitHub para añadirlo con comprobación automática de actualizaciones.',
    'Your Private Repositories': 'Tus repositorios privados',
    'Refresh': 'Actualizar',
    'Filter private repositories…': 'Filtrar repositorios privados…',
    'Added GitHub Apps': 'Aplicaciones de GitHub añadidas',
    'GitHub Account': 'Cuenta de GitHub',
    'Not signed in': 'Sin iniciar sesión',
    'Signed in': 'Sesión iniciada',
    'Signing in…': 'Iniciando sesión…',
    'Private repositories enabled · 5,000 GitHub requests/hour': 'Repositorios privados habilitados · 5.000 solicitudes/hora',
    'Sign out': 'Cerrar sesión',
    'Sign in with a Personal Access Token to pull private repositories and raise GitHub\'s rate limit. Tokens are stored securely in your OS credential store.': 'Inicia sesión con un token de acceso personal para obtener repositorios privados y aumentar el límite de GitHub. Los tokens se guardan de forma segura en el almacén de credenciales del sistema.',
    'Personal Access Token': 'Token de acceso personal',
    'Cancel': 'Cancelar',
    'Create token on GitHub ↗': 'Crear token en GitHub ↗',
    'Connect token': 'Conectar token',
    'Use a classic token with \'repo\' scope, or a fine-grained token with \'Contents: Read-only\' access.': 'Usa un token clásico con permiso «repo» o un token detallado con acceso «Contents: Read-only».',
    'Manage credentials saved by this copy or older/deleted copies of Launcher.': 'Administra las credenciales guardadas por esta copia o copias antiguas/eliminadas de Launcher.',
    'Clear old copies': 'Borrar copias antiguas',
    'Clear old copies ({n})': 'Borrar copias antiguas ({n})',
    'Clear all from credential store': 'Borrar todo del almacén de credenciales',
    'Click again to clear all credentials': 'Haz clic de nuevo para borrar todas las credenciales',
    'This copy': 'Esta copia',
    'Deleted copy': 'Copia eliminada',
    'Old / other copy': 'Copia antigua / otra',
    'Delete': 'Eliminar',
    'Private': 'Privado',
    'Public': 'Público',
    'Add': 'Añadir',
    'Added': 'Añadido',
    'Remove': 'Quitar',
    'Confirm': 'Confirmar',
    'Remove from Launcher': 'Quitar de Launcher',
    'Click again to remove from Launcher': 'Haz clic de nuevo para quitar de Launcher',
    'Can’t remove {name}': 'No se puede quitar {name}',
    '{name} is still installed. Uninstall it first before removing it from Launcher.': '{name} todavía está instalado. Desinstálalo primero antes de quitarlo de Launcher.',
    'Private apps removed': 'Apps privadas eliminadas',
    'Removed uninstalled private app {name} because your GitHub token was removed.': 'Se quitó la app privada sin instalar {name} porque se eliminó tu token de GitHub.',
    'Removed {n} uninstalled private apps ({names}) because your GitHub token was removed.': 'Se quitaron {n} apps privadas sin instalar ({names}) porque se eliminó tu token de GitHub.',
    'Add Repository to Launcher': 'Añadir repositorio a Launcher',
    'Display name': 'Nombre para mostrar',
    'Where should this appear?': '¿Dónde debería aparecer?',
    'Main app library': 'Biblioteca principal de apps',
    'Short-lived tools & utilities': 'Herramientas y utilidades temporales',
    'Games library': 'Biblioteca de juegos',
    'Add to Launcher': 'Añadir a Launcher',
  },
};

const PLURALS = {
  daysAgo: {
    en: ['{n} day ago', '{n} days ago'],
    es: ['hace {n} día', 'hace {n} días'],
    ru: ['{n} день назад', '{n} дня назад', '{n} дней назад'],
  },
  appCount: {
    en: ['{n} app', '{n} apps'],
    es: ['{n} aplicación', '{n} aplicaciones'],
    ru: ['{n} приложение', '{n} приложения', '{n} приложений'],
  },
  gameCount: {
    en: ['{n} game', '{n} games'],
    es: ['{n} juego', '{n} juegos'],
    ru: ['{n} игра', '{n} игры', '{n} игр'],
  },
  updateCount: {
    en: ['{n} update', '{n} updates'],
    es: ['{n} actualización', '{n} actualizaciones'],
    ru: ['{n} обновление', '{n} обновления', '{n} обновлений'],
  },
  appUpdatesAvailable: {
    en: ['{n} app update available', '{n} app updates available'],
    es: ['{n} actualización de aplicación disponible', '{n} actualizaciones de aplicaciones disponibles'],
    ru: ['Доступно {n} обновление', 'Доступно {n} обновления', 'Доступно {n} обновлений'],
  },
  repairFiles: {
    en: ['Repair {n} file', 'Repair {n} files'],
    es: ['Reparar {n} archivo', 'Reparar {n} archivos'],
    ru: ['Восстановить {n} файл', 'Восстановить {n} файла', 'Восстановить {n} файлов'],
  },
  damagedFiles: {
    en: ['{n} file damaged or missing. Press Repair to fix.', '{n} files damaged or missing. Press Repair to fix.'],
    es: ['{n} archivo dañado o faltante. Pulsa Reparar para corregirlo.', '{n} archivos dañados o faltantes. Pulsa Reparar para corregirlos.'],
    ru: ['{n} файл повреждён или отсутствует. Нажмите «Восстановить».', '{n} файла повреждены или отсутствуют. Нажмите «Восстановить».', '{n} файлов повреждены или отсутствуют. Нажмите «Восстановить».'],
  },
  logLines: {
    en: ['{n} line', '{n} lines'],
    es: ['{n} línea', '{n} líneas'],
    ru: ['{n} строка', '{n} строки', '{n} строк'],
  },
};

const TAGLINES = {
  ru: {
    lighthouse: 'Мгновенный поиск по всем файлам и папкам на всех дисках со значками Проводника.',
    screenshotapp: 'Скриншоты по таймеру и горячим клавишам, с нумерацией и сохранением куда угодно.',
    rustwinutil: 'WinUtil от Chris Titus Tech, переписанный на Rust: установка программ, твики и исправление Windows.',
    kittyconnect: 'Хабы, голосовые комнаты, обмен файлами и хостинг игр с друзьями через зашифрованные туннели TailCat. Без аккаунта и настройки роутера.',
    inkcognito: '3D-мультиплеерная игра: создайте мир, поделитесь адресом TailCat и играйте с друзьями.',
    photocraft: 'Растровый фоторедактор с открытым исходным кодом, написанный с нуля на чистом Rust.',
    vectorcraft: 'Векторный редактор и инструмент иллюстрации с открытым кодом на чистом Rust.',
    filmcraft: 'Нелинейный видеоредактор с открытым исходным кодом, написанный с нуля на чистом Rust.',
    effectcraft: 'Инструмент моушн-графики и композитинга с открытым кодом на чистом Rust.',
    lightcraft: 'Неразрушающий редактор RAW и фотопроцесса с открытым кодом на чистом Rust.',
    designcraft: 'Редактор вёрстки и публикаций с открытым исходным кодом на чистом Rust.',
    pdfcraft: 'Чтение и редактирование PDF с открытым исходным кодом на чистом Rust.',
    figcraft: 'Инструмент дизайна интерфейсов и прототипирования с открытым кодом на чистом Rust.',
    soundcraft: 'Цифровая аудиостанция с открытым исходным кодом, написанная с нуля на чистом Rust.',
    cadcraft: 'Инструмент САПР и черчения с открытым исходным кодом на чистом Rust.',
    deckcraft: 'Редактор презентаций и слайд-шоу с открытым исходным кодом на чистом Rust.',
    gridcraft: 'Редактор электронных таблиц с открытым исходным кодом на чистом Rust.',
    wordcraft: 'Текстовый процессор с открытым исходным кодом, написанный с нуля на чистом Rust.',
    tempcraftbackup: 'Сохраняет исходный код всех приложений Craft в один архив 7z.',
  },
  es: {
    lighthouse: 'Búsqueda instantánea en todos los archivos y carpetas de cada unidad, con los iconos del Explorador.',
    screenshotapp: 'Capturas de pantalla con temporizador y atajos, numeradas y guardadas donde prefieras.',
    rustwinutil: 'WinUtil de Chris Titus Tech reescrito en Rust: instala aplicaciones, aplica ajustes y repara Windows.',
    kittyconnect: 'Salas, voz, intercambio de archivos y partidas con amigos mediante túneles TailCat cifrados. Sin cuenta ni configurar el router.',
    inkcognito: 'Juego multijugador 3D: crea un mundo, comparte tu dirección TailCat y juega con amigos.',
    photocraft: 'Editor de fotos rasterizadas de código abierto escrito desde cero en Rust puro.',
    vectorcraft: 'Herramienta de arte vectorial e ilustración de código abierto escrita desde cero en Rust puro.',
    filmcraft: 'Editor de vídeo no lineal de código abierto escrito desde cero en Rust puro.',
    effectcraft: 'Herramienta de gráficos en movimiento y composición de código abierto en Rust puro.',
    lightcraft: 'Editor no destructivo de fotos y RAW de código abierto escrito desde cero en Rust puro.',
    designcraft: 'Editor de maquetación y publicación de código abierto escrito desde cero en Rust puro.',
    pdfcraft: 'Lector y editor de PDF de código abierto escrito desde cero en Rust puro.',
    figcraft: 'Herramienta de diseño de interfaces y prototipado interactivo de código abierto en Rust puro.',
    soundcraft: 'Estación de audio digital de código abierto escrita desde cero en Rust puro.',
    cadcraft: 'Herramienta de diseño asistido por ordenador y dibujo técnico en Rust puro.',
    deckcraft: 'Editor de presentaciones y diapositivas de código abierto escrito desde cero en Rust puro.',
    gridcraft: 'Editor de hojas de cálculo de código abierto escrito desde cero en Rust puro.',
    wordcraft: 'Procesador de textos de código abierto escrito desde cero en Rust puro.',
    tempcraftbackup: 'Guarda una copia del código fuente actual de todas las aplicaciones Craft en un archivo 7z.',
  },
};

function detectSystemLang() {
  const langs = Array.isArray(navigator.languages) && navigator.languages.length
    ? navigator.languages
    : [navigator.language || 'en'];
  for (const raw of langs) {
    const base = String(raw || '').toLowerCase().split(/[-_]/)[0];
    if (SUPPORTED_LANGS.includes(base)) return base;
  }
  return 'en';
}

function activeLang() {
  const pref = state.settings.language || 'auto';
  if (SUPPORTED_LANGS.includes(pref)) return pref;
  return detectSystemLang();
}

function localeTag() {
  const lang = activeLang();
  return lang === 'ru' ? 'ru-RU' : lang === 'es' ? 'es-ES' : 'en-US';
}

function fmtVars(str, vars) {
  if (!vars) return str;
  return str.replace(/\{(\w+)\}/g, (_, k) => (vars[k] != null ? String(vars[k]) : `{${k}}`));
}

function t(key, vars) {
  const lang = activeLang();
  const str = (lang !== 'en' && I18N[lang] && I18N[lang][key]) || key;
  return fmtVars(str, vars);
}

function pluralIndex(lang, n) {
  const abs = Math.abs(Math.trunc(n));
  if (lang === 'ru') {
    const mod100 = abs % 100;
    if (mod100 >= 11 && mod100 <= 14) return 2;
    const mod10 = abs % 10;
    if (mod10 === 1) return 0;
    if (mod10 >= 2 && mod10 <= 4) return 1;
    return 2;
  }
  return abs === 1 ? 0 : 1;
}

function tPlural(key, n, vars) {
  const lang = activeLang();
  const entry = PLURALS[key];
  const forms = (entry && (entry[lang] || entry.en)) || ['{n}', '{n}'];
  const form = forms[Math.min(pluralIndex(lang, n), forms.length - 1)];
  return fmtVars(form, Object.assign({ n }, vars));
}

function tTagline(app) {
  const lang = activeLang();
  return (lang !== 'en' && TAGLINES[lang] && TAGLINES[lang][app.id]) || app.tagline || '';
}

function renderPortableDesc() {
  if (!el.portableDesc) return;
  const lang = activeLang();
  const code = (txt) => {
    const c = document.createElement('code');
    c.textContent = txt;
    return c;
  };
  el.portableDesc.replaceChildren();
  if (lang === 'ru') {
    el.portableDesc.append(
      'Настройки и профиль браузера хранятся в папке ',
      code('data\\'),
      ' рядом с ',
      code('Launcher.exe'),
      '. Скопируйте папку на флешку или другой ПК — всё перенесётся вместе с вами.',
    );
  } else if (lang === 'es') {
    el.portableDesc.append(
      'Los ajustes y el perfil del navegador se guardan en ',
      code('data\\'),
      ' junto a ',
      code('Launcher.exe'),
      '. Copia la carpeta a una memoria USB o a otro equipo y todo irá contigo.',
    );
  } else {
    el.portableDesc.append(
      'Settings and the browser profile live in ',
      code('data\\'),
      ' beside ',
      code('Launcher.exe'),
      '. Copy the folder to a stick or another PC and it comes with you.',
    );
  }
}

function applyLanguage() {
  el.root.lang = activeLang();
  document.querySelectorAll('[data-i18n]').forEach((node) => {
    node.textContent = t(node.dataset.i18n);
  });
  document.querySelectorAll('[data-i18n-title]').forEach((node) => {
    node.title = t(node.dataset.i18nTitle);
  });
  document.querySelectorAll('[data-i18n-placeholder]').forEach((node) => {
    node.placeholder = t(node.dataset.i18nPlaceholder);
  });
  document.querySelectorAll('[data-i18n-aria]').forEach((node) => {
    node.setAttribute('aria-label', t(node.dataset.i18nAria));
  });
  if (el.currentVersion) {
    el.currentVersion.textContent = `${t('Current Version')} : ${BOOT.version}`;
  }
  renderPortableDesc();
}

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
  if (d < 45) return t('just now');
  if (d < 3600) return t('{n} min ago', { n: Math.round(d / 60) });
  if (d < 86400) return t('{n} h ago', { n: Math.round(d / 3600) });
  const days = Math.round(d / 86400);
  return days === 1 ? t('yesterday') : tPlural('daysAgo', days);
}

function fmtDate(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  return d.toLocaleDateString(localeTag(), { month: 'short', day: 'numeric', year: 'numeric' });
}

const timeOf = (ms) => new Date(ms).toLocaleTimeString(localeTag(), { hour: '2-digit', minute: '2-digit', second: '2-digit' });

/* --------------------------------------------------- startup splash */

function startSplash() {
  if (!state.splashActive) return;
  el.splash.classList.add('on');
  el.splashStatus.textContent = t('Checking for updates…');
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
    el.splashStatus.textContent = t('Checking for updates…');
    el.splashBar.classList.add('indeterminate');
    el.splashSkip.classList.remove('on');
  } else if (u.state === 'available') {
    clearTimeout(state.splashTimeout);
    el.splashStatus.textContent = t('Update found — starting download…');
    el.splashBar.classList.add('indeterminate');
    el.splashSkip.classList.add('on');
  } else if (u.state === 'downloading') {
    clearTimeout(state.splashTimeout);
    const p = pct(u);
    el.splashStatus.textContent = u.total
      ? t('Downloading update — {pct}% ({done} of {total})', { pct: p, done: fmtMB(u.done), total: fmtMB(u.total) })
      : t('Downloading update…');
    el.splashBar.classList.remove('indeterminate');
    el.splashBar.style.width = `${p}%`;
    el.splashSkip.classList.add('on');
  } else if (u.state === 'ready') {
    clearTimeout(state.splashTimeout);
    el.splashStatus.textContent = t('Installing update & restarting…');
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
  post({ cmd: 'githubListCredentials' });
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
      const localizedName = t(s.name);
      b.title = i === 0 ? t('{name} (default)', { name: localizedName }) : localizedName;
      b.innerHTML = `<span class="sw" style="background:${s.paper}">
          <span class="sw-card" style="background:${s.card}">
            <span class="sw-line" style="background:${s.ink}"></span>
            <span class="sw-btn" style="background:${s.accent}"></span>
            <span class="sw-dot" style="background:${s.gilt}"></span>
          </span>
        </span>`;
      const label = document.createElement('span');
      label.className = 'sw-name';
      label.textContent = localizedName;
      if (i === 0) {
        const tag = document.createElement('span');
        tag.className = 'sw-default';
        tag.textContent = t('Default');
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
  if (el.setLanguage) {
    el.setLanguage.value = ['auto', 'en', 'es', 'ru'].includes(s.language) ? s.language : 'auto';
  }
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
  applyLanguage();
  renderSchemes();
  renderLastCheck();
  renderActivity();
  renderGitHubSettings();
}

function patchSettings(patch) {
  Object.assign(state.settings, patch);
  renderSettings();
  post({ cmd: 'settings', ...patch });
}

if (el.setLanguage) {
  el.setLanguage.addEventListener('change', () => {
    patchSettings({ language: el.setLanguage.value });
    renderUpdate();
    renderLibrary();
  });
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
  const ts = state.settings.lastCheck;
  el.last.textContent = ts ? t('Checked {ago}', { ago: ago(ts) }) : '';
}

setInterval(() => {
  renderLastCheck();
  renderActivity();
}, 30000);

/* ----------------------------------------------------------- updates */

function describe(u) {
  switch (u.state) {
    case 'checking':
      return { pill: 'busy', short: t('Checking for updates…'),
               title: t('Checking…'), detail: t('Asking GitHub for the latest release of {repo}.', { repo: BOOT.repo }) };
    case 'upToDate':
      return { pill: 'ok', short: t('Up to date'),
               title: t('Up to date'), detail: t('You are running the newest release on GitHub.') };
    case 'available':
      return { pill: 'gilt', short: t('Launcher update available'),
               title: t('Update available'), detail: t('A newer Launcher release is out ({size}).', { size: fmtMB(u.release.size) }) };
    case 'downloading':
      return { pill: 'busy', short: t('Downloading update — {pct}%', { pct: pct(u) }),
               title: t('Downloading…'),
               detail: u.total ? t('{done} of {total}', { done: fmtMB(u.done), total: fmtMB(u.total) }) : t('Starting the download…') };
    case 'ready':
      return { pill: 'gilt',
               short: state.deferred ? t('Update installs when you close the launcher') : t('Update is ready to install'),
               title: t('Ready to restart'),
               detail: t('Downloaded and verified. Restart to switch over, or it installs itself when you close the launcher.') };
    case 'error':
      return { pill: 'bad', short: u.message, title: t('Update failed'), detail: u.message };
    default:
      return { pill: '', short: t('Not checked'),
               title: t('Not checked'), detail: t('The launcher can check GitHub for a newer release.') };
  }
}

function activityText(a) {
  const task = a.task;
  const p = task.total ? Math.floor((task.done / task.total) * 100) : 0;
  switch (task.state) {
    case 'queued':      return t('Queued: {name}', { name: a.name });
    case 'downloading':
      if (a.installed) {
        return task.total ? t('Updating {name} — {pct}%', { name: a.name, pct: p }) : t('Updating {name}…', { name: a.name });
      }
      return task.total ? t('Downloading {name} — {pct}%', { name: a.name, pct: p }) : t('Downloading {name}…', { name: a.name });
    case 'installing':  return t('Installing {name}…', { name: a.name });
    case 'verifying':
      return task.total
        ? t('Verifying {name} — {done} of {total} files', { name: a.name, done: task.done, total: task.total })
        : t('Verifying {name}…', { name: a.name });
    case 'repairing':
      return task.total
        ? t('Repairing {name} — {pct}%', { name: a.name, pct: p })
        : t('Repairing {name}…', { name: a.name });
    default:            return null;
  }
}

function renderActivity() {
  const jobs = [];
  const u = state.update;
  if (u.state === 'downloading') jobs.push(t('Downloading Launcher update — {pct}%', { pct: pct(u) }));
  for (const a of library.apps) {
    const text = activityText(a);
    if (text) jobs.push(text);
  }
  const active = jobs.length > 0;
  const checkingAny = u.state === 'checking' || library.apps.some((a) => a.task.state === 'checking');

  el.lamp.className = active || checkingAny ? 'busy' : '';
  if (active) {
    el.statusText.textContent = jobs[0] + (jobs.length > 1 ? ` · ${t('+{n} more', { n: jobs.length - 1 })}` : '');
    el.statusText.title = jobs.join('\n');
  } else if (checkingAny) {
    el.statusText.textContent = t('Checking for updates…');
    el.statusText.title = '';
  } else if (state.settings.lastCheck) {
    el.statusText.textContent = t('Checked {ago} · using GitHub', { ago: ago(state.settings.lastCheck) });
    el.statusText.title = '';
  } else {
    el.statusText.textContent = t('Using GitHub');
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
  el.statusUpd.title = nudge ? d.short : t('Update settings');
  if (nudge) el.statusUpdT.textContent = t(nudge);

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
    el.bannerText.textContent = t('Downloading Launcher update…');
    el.bannerProgress.classList.add('on');
    el.bannerProgLabel.textContent = u.total
      ? t('{done} of {total} · {pct}%', { done: fmtMB(u.done), total: fmtMB(u.total), pct: pct(u) })
      : t('Downloading…');
    el.bannerBar.style.width = `${pct(u)}%`;
    el.bannerAction.style.display = 'none';
  } else if (u.state === 'ready') {
    el.bannerText.textContent = t('Launcher update is ready to install.');
    el.bannerProgress.classList.remove('on');
    el.bannerAction.style.display = 'inline-flex';
    el.bannerAction.textContent = t('Restart now');
  } else {
    el.bannerText.textContent = t('A newer Launcher update is available.');
    el.bannerProgress.classList.remove('on');
    el.bannerAction.style.display = 'inline-flex';
    el.bannerAction.textContent = t('Update & restart');
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
  el.notesTitle.textContent = rel.prerelease ? t('Pre-release notes') : t('Release notes');
  el.notesLink.dataset.url = rel.page;
  const body = (rel.notes || '').trim();
  el.notesBody.replaceChildren();
  if (body) {
    el.notesBody.textContent = body;
  } else {
    const m = h('span', 'muted', t('No release notes.'));
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
    el.bellBody.append(h('div', 'popover-empty', t('Everything is up to date.')));
    return;
  }

  if (launcherHas) {
    const row = h('div', 'popover-item');
    const info = h('div', 'popover-item-info');
    info.append(
      h('div', 'popover-item-name', 'Launcher'),
      h('div', 'popover-item-meta', state.update.state === 'ready' ? t('Ready to restart') : t('Update available')),
    );
    const act = h('button', 'btn primary compact', state.update.state === 'ready' ? t('Restart') : t('Update'));
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
    const act = h('button', 'btn primary compact', t('Update'));
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
  const countStr = tPlural(noun === 'game' ? 'gameCount' : 'appCount', list.length);
  return `${countStr} · ${t('{n} installed', { n: installed })}`
    + (updates ? ` · ${tPlural('updateCount', updates)}` : '');
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
  const hay = `${a.name} ${a.tagline || ''} ${tTagline(a)} ${a.repo || ''} ${a.publisher || ''}`.toLowerCase();
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
  const task = a.task;
  const busy = ['checking', 'queued', 'downloading', 'installing', 'verifying', 'repairing', 'stopping'].includes(task.state);
  const supported = isSupported(a);
  const latest = a.latest;
  const damaged = (a.damaged || []).length;
  const szCls = compact ? 'compact' : 'small';

  if (task.state === 'queued') {
    const b = h('button', `btn ${szCls} main-btn`, t('Queued · Cancel'));
    b.type = 'button';
    b.addEventListener('click', () => post({ cmd: 'appCancelQueue', id: a.id }));
    return b;
  }

  if (a.installed) {
    if (damaged) {
      const fix = h('button', `btn ${szCls} gilt main-btn`, tPlural('repairFiles', damaged));
      fix.type = 'button';
      fix.disabled = busy;
      fix.addEventListener('click', () => post({ cmd: 'appRepair', id: a.id }));
      return fix;
    }

    if (task.state === 'stopping') {
      const b = h('button', `btn ${szCls} danger main-btn`, t('Stopping…'));
      b.type = 'button';
      b.disabled = true;
      return b;
    }

    if (a.running) {
      const stop = h('button', `btn ${szCls} danger main-btn`, t('Stop'));
      stop.type = 'button';
      stop.disabled = busy;
      stop.title = t('Ask {name} to quit', { name: a.name });
      stop.addEventListener('click', () => post({ cmd: 'appStop', id: a.id }));
      return stop;
    }

    if (a.updateAvailable && supported) {
      const split = h('div', 'split-btns');
      const open = h('button', `btn ${szCls}`, t('Open'));
      open.type = 'button';
      open.disabled = busy;
      open.addEventListener('click', () => post({ cmd: 'appLaunch', id: a.id }));

      const up = h('button', `btn ${szCls} primary`, t('Update'));
      up.type = 'button';
      up.title = t('Update {name} to v{version}', { name: a.name, version: latest.version });
      up.disabled = busy;
      up.addEventListener('click', () => post({ cmd: 'appInstall', id: a.id }));
      split.append(open, up);
      return split;
    }

    if (a.starting) {
      const b = h('button', `btn ${szCls} main-btn`, t('Starting…'));
      b.type = 'button';
      b.disabled = true;
      return b;
    }

    const open = h('button', `btn ${szCls} main-btn`, t('Open'));
    open.type = 'button';
    open.disabled = busy || !supported;
    open.addEventListener('click', () => post({ cmd: 'appLaunch', id: a.id }));
    return open;
  }

  if (!supported) {
    const plat = Array.isArray(a.platforms) && a.platforms.length
      ? t('{platforms} only', { platforms: a.platforms.join(' / ') })
      : t('Not on {os}', { os: CURRENT_OS });
    const b = h('button', `btn ${szCls} main-btn`, plat);
    b.type = 'button';
    b.disabled = true;
    return b;
  }

  const label = !latest && task.state === 'checking' ? t('Checking…') : t('Install');
  const inst = h('button', `btn ${szCls} primary main-btn`, label);
  inst.type = 'button';
  inst.disabled = busy || !latest;
  inst.addEventListener('click', () => post({ cmd: 'appInstall', id: a.id }));
  return inst;
}

function statusBadgeInfo(a) {
  const task = a.task;
  const supported = isSupported(a);
  if (task.state === 'queued') return { text: t('Queued'), tone: 'info' };
  if (task.state === 'downloading') return { text: t('Downloading…'), tone: 'info' };
  if (task.state === 'installing') return { text: t('Installing…'), tone: 'info' };
  if (task.state === 'verifying') return { text: t('Verifying…'), tone: 'info' };
  if (task.state === 'repairing') return { text: t('Repairing…'), tone: 'warn' };
  if (task.state === 'error') return { text: t('Error'), tone: 'bad' };
  if (a.running) return { text: t('Running'), tone: 'good', dot: true };
  if (a.installed) {
    if (a.updateAvailable) return { text: t('Update available'), tone: 'warn' };
    return null; // Installed & up to date: keep clean just like ArtCraft Launcher!
  }
  if (!supported) {
    const p = Array.isArray(a.platforms) && a.platforms.length
      ? t('{platforms} only', { platforms: a.platforms.join(' / ') })
      : t('Not on {os}', { os: CURRENT_OS });
    return { text: p, tone: '' };
  }
  if (a.latest) return { text: t('Available to install'), tone: 'info' };
  if (a.checked) return { text: t('No release yet'), tone: '' };
  return { text: t('Checking…'), tone: '' };
}

function fillAppStatus(status, a) {
  const task = a.task;
  const damaged = (a.damaged || []).length;
  status.className = 'app-status';
  status.replaceChildren();

  if (task.state === 'downloading') {
    const p = task.total ? Math.floor((task.done / task.total) * 100) : 0;
    const bar = h('div', 'progress on');
    const fill = h('div', 'bar');
    fill.style.width = `${p}%`;
    bar.append(fill);
    status.append(bar, task.total
      ? t('Downloading {done} of {total} · {pct}%', { done: fmtMB(task.done), total: fmtMB(task.total), pct: p })
      : t('Downloading…'));
  } else if (task.state === 'installing') {
    status.append(t('Verifying files and installing…'));
  } else if (task.state === 'queued') {
    status.append(t('Waiting for another download to finish…'));
  } else if (task.state === 'verifying' || task.state === 'repairing') {
    const frac = task.total ? task.done / task.total : 0;
    const bar = h('div', 'progress on');
    const fill = h('div', 'bar');
    fill.style.width = `${Math.floor(frac * 100)}%`;
    bar.append(fill);
    status.append(bar, task.state === 'verifying'
      ? (task.total ? t('Verifying {done} of {total} files', { done: task.done, total: task.total }) : t('Verifying…'))
      : (task.total ? t('Repairing {pct}% of {total}', { pct: Math.floor(frac * 100), total: fmtMB(task.total) }) : t('Reading package index…')));
  } else if (damaged && task.state !== 'error') {
    status.classList.add('bad');
    status.append(tPlural('damagedFiles', damaged));
  } else if (task.state === 'error') {
    status.classList.add('bad');
    status.append(task.message);
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
  toggle.title = open ? t('Hide log') : t('Show log');
  toggle.innerHTML = '<svg class="chev" width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">'
    + '<path d="M2 3.5 5 6.5 8 3.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  toggle.append(h('span', 'log-label', t('Log')));
  toggle.append(h('span', `log-last ${last.kind}`, open ? tPlural('logLines', a.log.length) : last.text));
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
    ver.textContent = Array.isArray(a.platforms)
      ? t('{platforms} only', { platforms: a.platforms.join(' / ') })
      : t('Unavailable');
  } else {
    ver.textContent = a.checked ? t('No release') : t('Checking…');
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
  const pubBadge = h('span', `hud-badge${pub === 'Javacrafter' || pub === 'ArtCraft' ? ' info' : ''}`, t('by {name}', { name: pub }));
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
  if (a.private) {
    badges.append(h('span', 'hud-badge warn', t('Private')));
  }
  if (a.portable != null) {
    const pTag = h('span', `hud-badge${a.portable ? ' good' : ''}`, a.portable ? t('Portable') : t('Not portable'));
    pTag.title = a.portable
      ? t('Keeps its settings and data in {folder}Data, inside its own folder.', { folder: a.folder })
      : t('Keeps its settings in ~/Library/Application Support/{name}, outside its own folder.', { name: a.name });
    badges.append(pTag);
  }
  body.append(badges);

  // Copy block: Download size (when install/update), tagline, last updated date
  const copy = h('div', 'app-copy');
  if ((!a.installed || a.updateAvailable) && supported && latest && latest.size > 0 && a.task.state === 'idle') {
    copy.append(h('div', 'app-size', t('Download size: {size}', { size: fmtMB(latest.size) })));
  }
  const tagline = tTagline(a);
  if (tagline) {
    const tag = h('div', 'app-tagline', tagline);
    tag.title = tagline;
    copy.append(tag);
  }
  const pubDate = latest && latest.published ? fmtDate(latest.published) : '';
  if (pubDate) {
    copy.append(h('div', 'app-updated', t('Last updated: {date}', { date: pubDate })));
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
  pinBtn.title = pinned ? t('Unpin {name}', { name: a.name }) : t('Pin {name} to top', { name: a.name });
  pinBtn.innerHTML = pinSvg(pinned);
  pinBtn.addEventListener('click', () => togglePin(a.id));
  actions.append(pinBtn);

  const menuBtn = h('button', 'sq-btn menu-btn');
  menuBtn.type = 'button';
  menuBtn.title = t('More actions for {name}', { name: a.name });
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
    makeTh(t('App'), 'name'),
    makeTh(t('Status'), 'status'),
    makeTh(t('Version'), null),
    makeTh(t('Released'), 'updated'),
    makeTh(t('Author'), null),
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
    tx.append(nm, h('div', 'tbl-tag', tTagline(a)));
    appBox.append(ic, tx);
    tdApp.append(appBox);

    // Status cell
    const tdStatus = h('td');
    const bi = statusBadgeInfo(a) || { text: activeLang() === 'en' ? 'Installed' : t('status.Installed'), tone: 'good' };
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
    pinBtn.title = pinned ? t('Unpin {name}', { name: a.name }) : t('Pin {name} to top', { name: a.name });
    pinBtn.innerHTML = pinSvg(pinned);
    pinBtn.addEventListener('click', () => togglePin(a.id));
    acts.append(pinBtn);

    const menuBtn = h('button', 'sq-btn menu-btn');
    menuBtn.type = 'button';
    menuBtn.title = t('More actions for {name}', { name: a.name });
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
  el.btnUpdateAll.textContent = updatable.length === 1
    ? t('Update 1 app')
    : t('Update all ({count})', { count: updatable.length });

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
      ? t('No matches for “{q}”.', { q: state.search.trim() })
      : state.tab === 'temporary'
        ? t('No temporary apps right now.')
        : state.tab === 'games'
          ? t('No games yet. They’ll appear here once they’re added.')
          : t('No apps found.');
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
      el.catalogContent.append(renderGridSection(t('Pinned'), pinned));
    }
    if (installed.length) {
      el.catalogContent.append(renderGridSection(pinned.length ? t('Installed') : '', installed));
    }
    if (notInstalled.length) {
      const showHeader = pinned.length > 0 || installed.length > 0;
      el.catalogContent.append(renderGridSection(showHeader ? t('Not installed') : '', notInstalled));
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
      addItem(t('Launch with .bat'), () => post({ cmd: 'appLaunchBat', id: a.id }), { disabled: busy });
    }
    addItem(t('Open install folder'), () => post({ cmd: 'appFolder', id: a.id }));
    addItem(t('Verify & repair files'), () => post({ cmd: 'appVerify', id: a.id }), { disabled: busy });
  }

  if (a.latest && a.latest.page) {
    addItem(t('Release notes ↗'), () => post({ cmd: 'openUrl', url: a.latest.page }));
  }
  addItem(t('View on GitHub ↗'), () => post({ cmd: 'openUrl', url: `https://github.com/${a.repo}` }));

  if (a.log && a.log.length) {
    const isOpen = !!library.logOpen[a.id];
    addItem(isOpen ? t('Hide activity log') : t('Show activity log'), () => {
      library.logOpen[a.id] = !isOpen;
      renderLibrary();
    });
  }

  if (a.installed) {
    el.cardMenu.append(h('div', 'cm-sep'));
    const armed = library.armed === a.id;
    const label = armed
      ? (CURRENT_OS === 'MacOS' ? t('Click again to move to Trash') : t('Click again to delete app & data'))
      : t('Uninstall');
    addItem(label, () => uninstallClick(a.id), {
      cls: armed ? 'danger armed' : 'danger',
      disabled: busy,
      keepOpen: !armed,
    });
  }

  if (a.custom) {
    if (!a.installed) el.cardMenu.append(h('div', 'cm-sep'));
    const armed = state.removeArmedId === a.id;
    const label = armed
      ? t('Click again to remove from Launcher')
      : t('Remove from Launcher');
    addItem(label, () => removeCustomClick(a.id), {
      cls: armed ? 'danger armed' : 'danger',
      disabled: busy,
      keepOpen: !armed && !a.installed,
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
    el.toastTitle.textContent = t('Launcher update available');
    el.toastDesc.textContent = t('A newer version of Launcher is ready to download and install.');
    el.toastProg.classList.remove('on');
    el.toastActs.classList.add('on');
    el.toastLater.textContent = t('Dismiss');
    el.toastNow.textContent = t('Update now');
    return;
  }

  // Downloading a launcher update triggered from the background popup
  if (state.updateMode === 'background' && u.state === 'downloading') {
    clearTimeout(state.toastTimer);
    state.toastMode = 'launcher-dl';
    el.toast.classList.remove('done');
    el.toast.classList.add('on');
    el.toastTitle.textContent = t('Downloading update');
    el.toastDesc.textContent = u.total ? t('{pct}% of {total}', { pct: pct(u), total: fmtMB(u.total) }) : t('Starting download…');
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
    el.toastTitle.textContent = t('Update ready to install');
    el.toastDesc.textContent = t('Restart now to finish updating, or it will install when you close Launcher.');
    el.toastProg.classList.remove('on');
    el.toastActs.classList.add('on');
    el.toastLater.textContent = t('Later');
    el.toastNow.textContent = t('Restart now');
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
    el.toastTitle.textContent = t('Update available: {name}', { name: first.name });
    el.toastDesc.textContent = t('v{version} is ready to install.', { version: first.latest.version });
  } else {
    el.toastTitle.textContent = tPlural('appUpdatesAvailable', newlyUpdated.length);
    el.toastDesc.textContent = t('{names} can be updated.', { names: newlyUpdated.map((a) => a.name).slice(0, 3).join(', ') });
  }
  el.toastProg.classList.remove('on');
  el.toastActs.classList.add('on');
  el.toastLater.textContent = t('Dismiss');
  el.toastNow.textContent = t('View');
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
    if (n) flashToast(tPlural('appUpdatesAvailable', n), t('Press Update on each one to install it.'));
    else flashToast(t('Your apps are up to date'), t('Everything installed is on its newest release.'));
  }
}

/* ------------------- GitHub Account, OS Credential Store & Custom Apps */

function setRepoMsg(kind, text) {
  state.repoMsg = text ? { kind, text } : null;
  renderGitHubSettings();
}

function setAuthMsg(kind, text) {
  state.authMsg = text ? { kind, text } : null;
  renderGitHubSettings();
}

function categoryLabel(cat) {
  if (cat === 'temporary') return t('Temporary Apps');
  if (cat === 'games') return t('Games');
  return t('Apps');
}

function inspectGitHubRepo(input) {
  const val = (input || '').trim();
  if (!val) {
    setRepoMsg('bad', t('Paste a GitHub repository link (for example, https://github.com/owner/repo).'));
    return;
  }
  state.repoBusy = true;
  setRepoMsg('info', t('Looking up repository on GitHub…'));
  post({ cmd: 'customAppInspect', input: val });
}

function openAddAppModal(preview) {
  state.pendingRepo = preview;
  state.pendingCategory = 'apps';
  el.addAppSlug.textContent = preview.repo;
  el.addAppName.value = preview.name || '';
  el.addAppMeta.replaceChildren();

  const badges = h('div', 'modal-meta-badges');
  if (preview.private) badges.append(h('span', 'hud-badge warn', t('Private')));
  else badges.append(h('span', 'hud-badge', t('Public')));
  if (preview.latestVersion) {
    badges.append(h('span', 'hud-badge good', `v${preview.latestVersion}`));
  } else if (preview.hasReleases) {
    badges.append(h('span', 'hud-badge warn', t('No matching {os} package yet', { os: CURRENT_OS })));
  } else {
    badges.append(h('span', 'hud-badge', t('No release yet')));
  }
  if (Array.isArray(preview.platforms) && preview.platforms.length) {
    badges.append(h('span', 'hud-badge info', preview.platforms.join(' · ')));
  }
  el.addAppMeta.append(badges);

  if (preview.tagline) {
    el.addAppMeta.append(h('div', '', preview.tagline));
  }
  if (preview.matchedAsset) {
    el.addAppMeta.append(h('div', 'meta', t('Package: {file}', { file: preview.matchedAsset })));
  }

  el.addAppCats.forEach((b) => b.classList.toggle('on', b.dataset.cat === state.pendingCategory));
  el.addAppModal.classList.add('on');
  setTimeout(() => el.addAppName.focus(), 20);
}

function closeAddAppModal() {
  state.pendingRepo = null;
  el.addAppModal.classList.remove('on');
}

function confirmAddAppModal() {
  const p = state.pendingRepo;
  if (!p) return;
  post({
    cmd: 'customAppAdd',
    repo: p.repo,
    name: el.addAppName.value.trim() || p.name,
    publisher: p.publisher,
    tagline: p.tagline,
    category: state.pendingCategory,
    platforms: p.platforms,
    private: !!p.private,
    ownFormat: !!p.ownFormat,
  });
  closeAddAppModal();
}

function removeCustomClick(id) {
  const live = library.apps.find((x) => x.id === id);
  if (live && live.installed) {
    clearTimeout(state.removeArmedTimer);
    state.removeArmedId = null;
    closeCardMenu();
    const msg = t('{name} is still installed. Uninstall it first before removing it from Launcher.', {
      name: live.name,
    });
    setRepoMsg('bad', msg);
    flashToast(t('Can’t remove {name}', { name: live.name }), msg);
    return;
  }
  clearTimeout(state.removeArmedTimer);
  if (state.removeArmedId === id) {
    state.removeArmedId = null;
    closeCardMenu();
    post({ cmd: 'customAppRemove', id });
  } else {
    state.removeArmedId = id;
    state.removeArmedTimer = setTimeout(() => {
      state.removeArmedId = null;
      if (state.openMenuId === id) {
        const a = library.apps.find((x) => x.id === id);
        if (a) fillCardMenu(a);
      }
      renderCustomAppsList();
    }, 4500);
    if (state.openMenuId === id) {
      const a = library.apps.find((x) => x.id === id);
      if (a) fillCardMenu(a);
    }
    renderCustomAppsList();
  }
}

function renderCustomAppsList() {
  const list = Array.isArray(state.settings.customApps) ? state.settings.customApps : [];
  el.customAppsWrap.classList.toggle('on', list.length > 0);
  el.customAppsCount.textContent = list.length ? String(list.length) : '';
  el.customAppsList.replaceChildren();

  for (const c of list) {
    const live = library.apps.find((x) => x.id === c.id);
    const row = h('div', 'gh-row');
    const main = h('div', 'gh-row-main');
    const title = h('div', 'gh-row-title');
    title.append(document.createTextNode(c.name));
    if (c.private) title.append(h('span', 'hud-badge warn', t('Private')));
    if (live && live.installed) title.append(h('span', 'hud-badge good', t('Installed')));
    main.append(title, h('div', 'gh-row-sub', c.repo));

    const acts = h('div', 'gh-row-acts');
    const sel = document.createElement('select');
    sel.className = 'select';
    sel.setAttribute('aria-label', t('Section for {name}', { name: c.name }));
    for (const [val, label] of [
      ['apps', t('Apps')],
      ['temporary', t('Temporary Apps')],
      ['games', t('Games')],
    ]) {
      const opt = document.createElement('option');
      opt.value = val;
      opt.textContent = label;
      if ((c.category || 'apps') === val) opt.selected = true;
      sel.append(opt);
    }
    sel.addEventListener('change', () => {
      post({ cmd: 'customAppMoveCategory', id: c.id, category: sel.value });
    });

    const armed = state.removeArmedId === c.id;
    const rm = h('button', `btn compact ${armed ? 'danger' : 'ghost'}`, armed ? t('Confirm') : t('Remove'));
    rm.type = 'button';
    rm.addEventListener('click', () => removeCustomClick(c.id));

    acts.append(sel, rm);
    row.append(main, acts);
    el.customAppsList.append(row);
  }
}

function renderPrivateRepos() {
  const show = !!state.auth.loggedIn;
  el.ghPrivWrap.classList.toggle('on', show);
  if (!show) return;

  el.btnGhRefRepos.disabled = state.privLoading;
  el.ghPrivList.replaceChildren();

  if (state.privLoading) {
    el.ghPrivList.append(h('div', 'gh-list-empty', t('Loading private repositories…')));
    return;
  }
  if (state.privError) {
    el.ghPrivList.append(h('div', 'gh-list-empty', state.privError));
    return;
  }

  const q = (state.privFilter || '').trim().toLowerCase();
  const filtered = state.privateRepos.filter((r) => {
    if (!q) return true;
    return `${r.fullName} ${r.description || ''}`.toLowerCase().includes(q);
  });

  if (filtered.length === 0) {
    el.ghPrivList.append(h('div', 'gh-list-empty', q
      ? t('No private repositories match “{q}”.', { q: state.privFilter.trim() })
      : t('No private repositories found for this GitHub account.')));
    return;
  }

  const existingRepos = new Set(library.apps.map((a) => (a.repo || '').toLowerCase()));
  for (const r of filtered) {
    const row = h('div', 'gh-row');
    const main = h('div', 'gh-row-main');
    const title = h('div', 'gh-row-title');
    title.append(document.createTextNode(r.fullName), h('span', 'hud-badge warn', t('Private')));
    main.append(title);
    if (r.description) main.append(h('div', 'gh-row-sub', r.description));

    const acts = h('div', 'gh-row-acts');
    const added = existingRepos.has(r.fullName.toLowerCase());
    const btn = h('button', `btn compact${added ? '' : ' primary'}`, added ? t('Added') : t('Add'));
    btn.type = 'button';
    btn.disabled = added || state.repoBusy;
    if (!added) {
      btn.addEventListener('click', () => inspectGitHubRepo(r.fullName));
    }
    acts.append(btn);
    row.append(main, acts);
    el.ghPrivList.append(row);
  }
}

function renderStoredCredentials() {
  const storeName = state.auth.storeName || (CURRENT_OS === 'MacOS' ? 'macOS Keychain' : 'Windows Credential Manager');
  el.ghStoreName.textContent = storeName;
  const creds = state.credentials || [];
  el.ghCredCount.textContent = state.credsLoaded
    ? (creds.length === 1 ? t('1 saved entry') : t('{n} saved entries', { n: creds.length }))
    : '';

  el.ghCredList.replaceChildren();
  if (!state.credsLoaded) {
    el.ghCredList.append(h('div', 'gh-list-empty', t('Checking OS credential store…')));
  } else if (creds.length === 0) {
    el.ghCredList.append(h('div', 'gh-list-empty', t('No Launcher credentials saved in {store}.', { store: storeName })));
  } else {
    for (const c of creds) {
      const row = h('div', 'gh-row');
      const main = h('div', 'gh-row-main');
      const title = h('div', 'gh-row-title');
      title.append(document.createTextNode(c.account ? `@${c.account}` : c.key));
      if (c.method && c.method !== 'unknown') {
        title.append(h('span', 'hud-badge info', c.method === 'oauth' ? 'OAuth' : 'PAT'));
      }
      if (c.isCurrent) {
        title.append(h('span', 'hud-badge good', t('This copy')));
      } else if (c.installDir && !c.dirExists) {
        title.append(h('span', 'hud-badge bad', t('Deleted copy')));
      } else {
        title.append(h('span', 'hud-badge warn', t('Old / other copy')));
      }
      const subText = c.installDir ? `${c.installDir} (${c.key})` : c.key;
      const sub = h('div', 'gh-row-sub', subText);
      sub.title = subText;
      main.append(title, sub);

      const acts = h('div', 'gh-row-acts');
      const del = h('button', 'btn compact ghost', t('Delete'));
      del.type = 'button';
      del.addEventListener('click', () => {
        post({ cmd: 'githubDeleteCredential', key: c.key, account: c.account || '' });
      });
      acts.append(del);
      row.append(main, acts);
      el.ghCredList.append(row);
    }
  }

  const oldCount = creds.filter((c) => !c.isCurrent).length;
  el.btnGhClearOld.disabled = oldCount === 0;
  el.btnGhClearOld.textContent = oldCount > 0
    ? t('Clear old copies ({n})', { n: oldCount })
    : t('Clear old copies');

  el.btnGhClearAll.disabled = creds.length === 0 && !state.auth.loggedIn;
  el.btnGhClearAll.textContent = state.clearAllArmed
    ? t('Click again to clear all credentials')
    : t('Clear all from credential store');
}

function renderGitHubSettings() {
  // Custom repo add controls
  el.btnGhAddRepo.disabled = state.repoBusy;
  el.btnGhAddRepo.textContent = state.repoBusy ? t('Checking…') : t('Add app');
  el.ghRepoMsg.className = `gh-msg${state.repoMsg ? ` on ${state.repoMsg.kind || ''}` : ''}`;
  el.ghRepoMsg.textContent = state.repoMsg ? state.repoMsg.text : '';

  renderPrivateRepos();
  renderCustomAppsList();

  // GitHub Account pill & panels
  const a = state.auth;
  if (a.loggedIn) {
    el.ghAuthPill.className = 'pill ok';
    el.ghAuthPillText.textContent = a.username ? `@${a.username}` : t('Signed in');
  } else if (state.authBusy) {
    el.ghAuthPill.className = 'pill busy';
    el.ghAuthPillText.textContent = t('Signing in…');
  } else {
    el.ghAuthPill.className = 'pill';
    el.ghAuthPillText.textContent = t('Not signed in');
  }

  el.ghSignedIn.classList.toggle('on', !!a.loggedIn);
  el.ghSignedOut.classList.toggle('on', !a.loggedIn);

  if (a.loggedIn) {
    el.ghUserName.textContent = a.username ? `@${a.username}` : t('GitHub User');
    el.ghUserAvatar.replaceChildren();
    if (a.avatarUrl) {
      const img = document.createElement('img');
      img.src = a.avatarUrl;
      img.alt = '';
      el.ghUserAvatar.append(img);
    } else {
      el.ghUserAvatar.textContent = (a.username || 'G')[0].toUpperCase();
    }
  } else {
    el.btnGhPat.disabled = state.authBusy;
    el.btnGhPat.textContent = state.authBusy ? t('Connecting…') : t('Connect token');
  }

  el.ghAuthMsg.className = `gh-msg${state.authMsg ? ` on ${state.authMsg.kind || ''}` : ''}`;
  el.ghAuthMsg.textContent = state.authMsg ? state.authMsg.text : '';

  renderStoredCredentials();
}

// Event bindings for Custom Apps & Category Modal
el.btnGhAddRepo.addEventListener('click', () => inspectGitHubRepo(el.ghRepoUrl.value));
el.ghRepoUrl.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') {
    e.preventDefault();
    inspectGitHubRepo(el.ghRepoUrl.value);
  }
});
el.btnGhRefRepos.addEventListener('click', () => {
  state.privLoading = true;
  state.privError = null;
  renderPrivateRepos();
  post({ cmd: 'githubFetchPrivateRepos' });
});
el.ghPrivFilter.addEventListener('input', () => {
  state.privFilter = el.ghPrivFilter.value;
  renderPrivateRepos();
});

el.addAppCats.forEach((btn) => btn.addEventListener('click', () => {
  state.pendingCategory = btn.dataset.cat;
  el.addAppCats.forEach((b) => b.classList.toggle('on', b.dataset.cat === state.pendingCategory));
}));
el.addAppCancel.addEventListener('click', closeAddAppModal);
el.addAppClose.addEventListener('click', closeAddAppModal);
el.addAppBack.addEventListener('click', closeAddAppModal);
el.addAppConfirm.addEventListener('click', confirmAddAppModal);
el.addAppName.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') {
    e.preventDefault();
    confirmAddAppModal();
  }
});

// Event bindings for GitHub Account & OS Credential Store
el.linkGhTokens.addEventListener('click', (e) => {
  e.preventDefault();
  post({ cmd: 'openUrl', url: 'https://github.com/settings/tokens' });
});

function submitPat() {
  const tok = (el.ghPatInput.value || '').trim();
  if (!tok) {
    setAuthMsg('bad', t('Paste a GitHub Personal Access Token first.'));
    return;
  }
  state.authBusy = true;
  setAuthMsg('info', t('Verifying token with GitHub…'));
  post({ cmd: 'githubLoginPat', token: tok });
  el.ghPatInput.value = '';
}
el.btnGhPat.addEventListener('click', submitPat);
el.ghPatInput.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') {
    e.preventDefault();
    submitPat();
  }
});
el.btnGhLogout.addEventListener('click', () => {
  setAuthMsg('', '');
  post({ cmd: 'githubLogout' });
});
el.btnGhRefCreds.addEventListener('click', () => {
  post({ cmd: 'githubListCredentials' });
});
el.btnGhClearOld.addEventListener('click', () => {
  post({ cmd: 'githubClearOldCredentials' });
});
el.btnGhClearAll.addEventListener('click', () => {
  clearTimeout(state.clearAllTimer);
  if (state.clearAllArmed) {
    state.clearAllArmed = false;
    post({ cmd: 'githubClearAllCredentials' });
    renderStoredCredentials();
  } else {
    state.clearAllArmed = true;
    state.clearAllTimer = setTimeout(() => {
      state.clearAllArmed = false;
      renderStoredCredentials();
    }, 4500);
    renderStoredCredentials();
  }
});

window.onHost = (msg) => {
  switch (msg.type) {
    case 'settings':
      state.settings = Object.assign({}, state.settings, msg.settings);
      renderSettings();
      renderUpdate();
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
      renderPrivateRepos();
      renderCustomAppsList();
      checkBackgroundAppUpdates();
      reportMenuCheck();
      break;
    case 'githubAuth': {
      const wasLoggedIn = !!state.auth.loggedIn;
      state.auth = Object.assign({}, state.auth, msg.auth);
      state.authBusy = false;
      if (state.auth.loggedIn && !wasLoggedIn) {
        state.privLoading = true;
        state.privError = null;
        setAuthMsg('ok', t('Signed in to GitHub and saved to {store}.', { store: state.auth.storeName }));
      }
      renderGitHubSettings();
      break;
    }
    case 'githubAuthError':
      state.authBusy = false;
      setAuthMsg('bad', msg.message || t('GitHub sign-in failed.'));
      break;
    case 'githubCredentials':
      state.credsLoaded = true;
      state.credentials = Array.isArray(msg.credentials) ? msg.credentials : [];
      renderStoredCredentials();
      break;
    case 'githubCredentialsCleared': {
      const n = msg.removed || 0;
      const store = state.auth.storeName;
      if (msg.scope === 'all') {
        setAuthMsg('ok', n
          ? t('Cleared {n} credential(s) from {store}.', { n, store })
          : t('No Launcher credentials remained in {store}.', { store }));
      } else if (msg.scope === 'old') {
        setAuthMsg('ok', n
          ? t('Cleared {n} old/duplicate credential(s) from {store}.', { n, store })
          : t('No old or duplicate credentials found in {store}.', { store }));
      } else {
        setAuthMsg('ok', t('Removed credential from {store}.', { store }));
      }
      break;
    }
    case 'githubCredentialsError':
      setAuthMsg('bad', msg.message || t('Couldn’t access OS credential store.'));
      break;
    case 'githubPrivateRepos':
      state.privLoading = false;
      state.privError = null;
      state.privateRepos = Array.isArray(msg.repos) ? msg.repos : [];
      renderPrivateRepos();
      break;
    case 'githubPrivateReposError':
      state.privLoading = false;
      state.privError = msg.message || t('Couldn’t load private repositories.');
      renderPrivateRepos();
      break;
    case 'repoPreview':
      state.repoBusy = false;
      setRepoMsg('', '');
      openAddAppModal(msg.preview);
      break;
    case 'repoPreviewError':
      state.repoBusy = false;
      setRepoMsg('bad', msg.message || t('Couldn’t find that GitHub repository.'));
      break;
    case 'customAppAdded':
      el.ghRepoUrl.value = '';
      setRepoMsg('ok', t('Added {name} to {section}.', {
        name: msg.name,
        section: categoryLabel(msg.category),
      }));
      flashToast(
        t('Added {name}', { name: msg.name }),
        t('Populated into {section} — checking for releases.', { section: categoryLabel(msg.category) }),
      );
      break;
    case 'customAppRemoveBlocked': {
      const name = msg.name || '';
      const text = msg.message || t('{name} is still installed. Uninstall it first before removing it from Launcher.', { name });
      setRepoMsg('bad', text);
      flashToast(t('Can’t remove {name}', { name }), text);
      break;
    }
    case 'privateAppsPruned': {
      const names = Array.isArray(msg.names) ? msg.names : [];
      if (names.length) {
        const text = names.length === 1
          ? t('Removed uninstalled private app {name} because your GitHub token was removed.', { name: names[0] })
          : t('Removed {n} uninstalled private apps ({names}) because your GitHub token was removed.', {
            n: names.length,
            names: names.join(', '),
          });
        setRepoMsg('info', text);
        flashToast(t('Private apps removed'), text);
      }
      break;
    }
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
  if (!e.target.closest('input[type=text], input[type=password], .notes-body, .log-body')) e.preventDefault();
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
    if (el.addAppModal.classList.contains('on')) { closeAddAppModal(); return; }
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
  flashToast(t('Launcher updated'), t('You’re running the newest release.'));
}

let readySent = false;
const ready = () => {
  if (readySent) return;
  readySent = true;
  post({ cmd: 'ready' });
};
requestAnimationFrame(ready);
setTimeout(ready, 50);
