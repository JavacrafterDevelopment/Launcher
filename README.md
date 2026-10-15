# Launcher

A small, portable launcher for Javacrafter apps and games, written in Rust. It
installs, launches and updates apps published on GitHub, and keeps itself up to
date from this repository's releases.

Built on [wry](https://github.com/tauri-apps/wry) and
[tao](https://github.com/tauri-apps/tao): a borderless native window hosting the
system WebView2, the same engine and the same cream-walnut-and-gilt look as
[Lighthouse](https://github.com/JavacrafterDevelopment/Lighthouse). The release
exe is a few MB and has no runtime to install.

---

## Running it

Put `Launcher.exe` in any folder — a USB stick, another PC — and run it. Nothing
is installed and nothing is written outside that folder:

```
Launcher\
  Launcher.exe
  launcher-manifest.json   every program file with its SHA-256
  data\
    settings.json     your settings
    webview\          the WebView2 profile
  portableApplications\
    Lighthouse\       each installed app in its own folder
    ScreenshotApp\
```

It needs the Microsoft Edge WebView2 Runtime, which ships with Windows 11 and
current Windows 10. For a machine without it, drop a
[fixed-version runtime](https://developer.microsoft.com/microsoft-edge/webview2/)
into a `WebView2\` folder beside the exe and the launcher uses that instead.

Only one launcher runs per folder; opening it again brings the existing window
forward.

| | |
|---|---|
| **Ctrl+,** | Settings |
| **Esc** | Back to the library |

## Library

The Library page lists the apps in [src/apps.rs](src/apps.rs):

| App | Repository |
|---|---|
| Lighthouse | [JavacrafterDevelopment/Lighthouse](https://github.com/JavacrafterDevelopment/Lighthouse) |
| ScreenshotApp | [JavacrafterDevelopment/ScreenshotApp](https://github.com/JavacrafterDevelopment/ScreenshotApp) |

**Install** puts an app in `portableApplications\<Name>\`. Each card then offers
**Launch** (started the way Explorer would, so an app that wants administrator
rights can ask), its folder, its release notes, and uninstall (two clicks; it
deletes the folder, the app's settings included).

App updates are **found** automatically — on every start and from **Check for
updates** — but **never installed on their own**. A card with a newer release gets
a gilt **Update to vX** button and the Library tab shows a count. An app updates
with exactly the same verified download and file swap as the launcher itself (see
below), never touches the app's `data\` folder so its settings survive, and won't
start while the app is running (close it first).

### Adding an app

Each app repo needs the same release pipeline as this one: a workflow that, on a
`v*` tag, builds the app into a folder, packs it with `tools/release-package.ps1`
as `<Name>-windows-x64.zip`, publishes the release, and writes `update.json` to an
`updates` branch with `tools/publish-update.ps1`. Lighthouse and ScreenshotApp both
have it in `.github/workflows/release.yml` — copy theirs. Then add an entry to
`CATALOG` in [src/apps.rs](src/apps.rs): the name must match the zip's `<Name>`.

Apps version independently: Lighthouse can be on v0.0.5 while ScreenshotApp is on
v0.0.1.

## Updates

On every start (unless **Update automatically** is off in Settings) the launcher
reads `update.json` from the repo's `updates` branch:

```
https://raw.githubusercontent.com/JavacrafterDevelopment/Launcher/updates/update.json
```

That's GitHub's CDN, not the API, so the API's 60-requests-an-hour limit doesn't
apply. The release workflow writes it; it has a `stable` channel (newest normal
release) and a `beta` channel (newest of any kind, read when **Include
pre-releases** is on). Each entry gives the version, notes, the package's URL,
size and SHA-256, and the size and SHA-256 of **every file** in the package.

If a release is newer than the running copy, the launcher:

1. downloads the release's `Launcher-windows-x64.zip` — the whole program folder —
   into `.launcher-update\` beside the exe, with progress in the corner of the window,
2. checks the zip against the SHA-256 in `update.json`,
3. unpacks it and checks **every file** against the zip's `launcher-manifest.json`:
   each must be there with exactly the listed size and SHA-256, and no path may
   point outside the folder or into `data\`,
4. checks that manifest is identical to the file list in `update.json`. The two
   come from different places, so a swapped zip can't bring a matching list,
5. counts down five seconds — **Restart now** or **Later** — then swaps the files
   in and starts the new version.

The swap renames each file being replaced to `<name>.launcher-old` (Windows allows
renaming a running exe or loaded DLL, just not overwriting it) and moves the new
one into place. Files the installed version's manifest lists but the new one
doesn't are retired the same way. If anything fails partway, everything already
moved is put back. The next start deletes the `.launcher-old` files.

**Later** keeps the verified update and swaps it in when you close the launcher,
so the next start is already the new version. Your `data\` folder is never
touched by an update.

**Settings → Updates** shows the installed version, **Check for updates**, the
release notes of anything new, and **Download & install** / **Restart to update**
buttons for doing it by hand. **Include pre-releases** also considers releases
marked as pre-release on GitHub.

If `update.json` can't be reached, the launcher asks the GitHub API instead: a
release counts when its tag is a semver version (`v0.2.0`, `v1.0.0-beta.1`) and it
has a `Launcher-windows-x64.zip` asset (drafts are ignored), and the zip is checked
against GitHub's own SHA-256 for it. If the API's limit is used up too, it finds
the newest stable release through the website; no published SHA-256 is available
that way, but the per-file manifest check still applies.

## Publishing a release

The [Release workflow](.github/workflows/release.yml) does it for you:

```bash
# 1. bump `version` in Cargo.toml, e.g. to 0.2.0, then:
git commit -am "Release 0.2.0"
git tag v0.2.0
git push origin main v0.2.0
```

GitHub Actions builds `Launcher.exe` with MSVC (C runtime and WebView2 loader
linked in statically), packs the folder with [tools/package.ps1](tools/package.ps1),
and publishes the release with auto-generated notes. It attaches:

- `Launcher-windows-x64.zip` — the full folder plus manifest; what updates use, and
  the download to hand to people,
- `Launcher.exe` — the bare exe, which is also what 0.1.0 updates from.

A tag with a hyphen (`v0.3.0-beta.1`) is published as a pre-release.

Last, [tools/publish-update.ps1](tools/publish-update.ps1) writes the release into
`update.json` and the workflow pushes it to the `updates` branch, creating the
branch the first time. That branch only ever holds `update.json`, so `main` never
gets bot commits and you never have to pull after a release. A pre-release updates
only the `beta` channel; a normal release updates both.

### Shipping more than the exe

Put files in a `package\` folder at the repo root (subfolders are fine) and they
ship beside `Launcher.exe` in every release, listed in the manifest and delivered
by updates. Remove a file from `package\` and the next update removes it from
installed copies too.

## Building locally

Needs [Rust](https://rustup.rs). Either toolchain works:

```bash
powershell -ExecutionPolicy Bypass -File build.ps1
```

That writes the portable folder to `dist\Launcher\` and the same
`dist\Launcher-windows-x64.zip` package a release ships.

- **MSVC** (`x86_64-pc-windows-msvc`, needs Visual Studio Build Tools): one
  self-contained `Launcher.exe`, same as the release.
- **GNU** (`x86_64-pc-windows-gnu`, no Visual Studio needed): links with Rust's
  bundled LLD (see [.cargo/config.toml](.cargo/config.toml)), and puts
  `WebView2Loader.dll` beside the exe. `build.ps1` sets up the `dlltool` the GNU
  toolchain needs from rustup's `llvm-tools` component.

`cargo run` works for development; debug builds keep a console window and allow
the WebView devtools.

### Testing an update without publishing

Debug builds read two overrides; release builds ignore both:

- `LAUNCHER_UPDATE_JSON` — a URL used in place of the `update.json` address. Build
  a newer version, run `tools\package.ps1` and `tools\publish-update.ps1` on it,
  point the package URL at a local server, and you can run the whole
  download-verify-swap-restart cycle.
- `LAUNCHER_UPDATE_API` — used in place of `https://api.github.com`, for testing
  the API fallback.

## Layout

```
src/main.rs        window, webview, messages between page and host
src/updater.rs     release check, download, verify, swap (launcher and apps)
src/apps.rs        the app catalog: install, update, launch, uninstall
src/settings.rs    data\settings.json beside the exe
src/instance.rs    one launcher per folder
ui/                the page: index.html, app.css, app.js (embedded in the exe)
assets/            icon (regenerate with tools\make-icon.ps1)
tools/package.ps1  builds the release folder, manifest and update zip
tools/publish-update.ps1  writes a release into update.json
package/           (optional) extra files to ship beside the exe
```
