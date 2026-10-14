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

1. downloads its `Launcher.exe` asset beside the running exe, with progress in the
   corner of the window,
2. checks the size, that it's a Windows program, and GitHub's SHA-256 digest,
3. counts down five seconds — **Restart now** or **Later** — then renames itself
   to `Launcher.exe.old`, moves the new file into its place and starts it.

**Later** keeps the verified download and swaps it in when you close the launcher,
so the next start is already the new version. The leftover `.old` is deleted on
the next start.

**Settings → Updates** shows the installed version, **Check for updates**, the
release notes of anything new, and **Download & install** / **Restart to update**
buttons for doing it by hand. **Include pre-releases** also considers releases
marked as pre-release on GitHub.

A release is picked up when its tag is a semver version (`v0.2.0`, `v1.0.0-beta.1`)
and it has an asset named `Launcher.exe`. Drafts are ignored.

## Publishing a release

The [Release workflow](.github/workflows/release.yml) does it for you:

```bash
# 1. bump `version` in Cargo.toml, e.g. to 0.2.0, then:
git commit -am "Release 0.2.0"
git tag v0.2.0
git push origin main v0.2.0
```

GitHub Actions builds `Launcher.exe` with MSVC (C runtime and WebView2 loader
linked in statically, so it's one file) and publishes the release with
auto-generated notes. A tag with a hyphen (`v0.3.0-beta.1`) is published as a
pre-release.

## Building locally

Needs [Rust](https://rustup.rs). Either toolchain works:

```bash
powershell -ExecutionPolicy Bypass -File build.ps1
```

That writes the portable folder to `dist\Launcher\` (`-Zip` also makes a zip).

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
`/repos/JavacrafterDevelopment/Launcher/releases` with a newer release lets you
run the whole download-verify-swap-restart cycle. Release builds ignore it.

## Layout

```
src/main.rs        window, webview, messages between page and host
src/updater.rs     GitHub release check, download, verify, swap, relaunch
src/settings.rs    data\settings.json beside the exe
src/instance.rs    one launcher per folder
ui/                the page: index.html, app.css, app.js (embedded in the exe)
assets/            icon (regenerate with tools\make-icon.ps1)
```
