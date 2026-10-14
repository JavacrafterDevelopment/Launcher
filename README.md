# Launcher

A small, portable launcher for Javacrafter apps and games, written in Rust. It
keeps itself up to date from this repository's GitHub releases.

Built on [wry](https://github.com/tauri-apps/wry) and
[tao](https://github.com/tauri-apps/tao): a borderless native window hosting the
system WebView2, the same engine and the same cream-walnut-and-gilt look as
[Lighthouse](https://github.com/JavacrafterDevelopment/Lighthouse). The release
exe is a few MB and has no runtime to install.

> The library (installing and launching your published apps) is the next stage.
> This first stage is the shell and self-update.

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

## Updates

On every start (unless **Update automatically** is off in Settings) the launcher
asks GitHub for this repository's releases. If one is newer than itself it:

1. downloads the release's `Launcher-windows-x64.zip` — the whole program folder —
   into `.launcher-update\` beside the exe, with progress in the corner of the window,
2. checks the zip against the SHA-256 GitHub publishes for every release asset,
3. unpacks it and checks **every file** against `launcher-manifest.json`: each must
   be there with exactly the listed size and SHA-256, and no path may point outside
   the folder or into `data\`,
4. counts down five seconds — **Restart now** or **Later** — then swaps the files
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

A release is picked up when its tag is a semver version (`v0.2.0`, `v1.0.0-beta.1`)
and it has an asset named `Launcher-windows-x64.zip`. Drafts are ignored.

If GitHub's API limit (60 requests an hour per IP) is used up, the launcher finds
the newest stable release through the website instead. GitHub's own SHA-256 isn't
available that way, but the per-file manifest check still applies.

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

Debug builds read `LAUNCHER_UPDATE_API` and use it in place of
`https://api.github.com`, so a local server that answers
`/repos/JavacrafterDevelopment/Launcher/releases` with a newer release (pointing
at a zip from `tools\package.ps1`) lets you run the whole
download-verify-swap-restart cycle. Release builds ignore it.

## Layout

```
src/main.rs        window, webview, messages between page and host
src/updater.rs     GitHub release check, download, verify, swap, relaunch
src/settings.rs    data\settings.json beside the exe
src/instance.rs    one launcher per folder
ui/                the page: index.html, app.css, app.js (embedded in the exe)
assets/            icon (regenerate with tools\make-icon.ps1)
tools/package.ps1  builds the release folder, manifest and update zip
package/           (optional) extra files to ship beside the exe
```
