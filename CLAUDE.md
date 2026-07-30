# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

UAD-ng (Universal Android Debloater Next Generation) — a Rust workspace that drives the `adb` CLI to
uninstall/disable/restore Android system and third-party packages, guided by a curated package
database (`resources/assets/uad_lists.json`).

**This checkout is a fork.** `origin` = `TerminatedProcess/...`, `upstream` = `Universal-Debloater-Alliance/...`.
`main` is a pure mirror of upstream; all local work lives on `mryan`. Sync with the `gitupdate` fish
function in `.salias_f` (ff-only on `main`, rebase `mryan` on top, force-with-lease push). Never commit
to `main` — it must stay fast-forwardable.

Fork-local features not present upstream (see git log on `mryan`): AI enrichment/chat
(`crates/uad-core/src/ai.rs`), "User apps" third-party-only filter, last-selected-device persistence.

## Commands

```sh
cargo build --release                 # whole workspace (alias: `build`)
cargo run -p uad-gui                  # GUI, binary name `uad-ng`
cargo run -p uad-cli -- <args>        # CLI, binary name `uad`
cargo install --path crates/uad-gui   # alias: `deploy`; then `run` launches it detached

cargo test                            # all tests
cargo test -p uad-core config::       # one module
cargo test test_last_device_roundtrip # one test

cargo fmt -- --check
cargo clippy --all-features -- -D clippy::all -W clippy::style
RUSTFLAGS='-D warnings' cargo check --all-features
```

Lints are strict: workspace-wide `clippy::pedantic` at `warn`, `undocumented_unsafe_blocks = "forbid"`,
`disallowed_types`/`disallowed_methods` at `deny`. CI runs check/test on Linux+Windows+macOS but
clippy/fmt only on Linux.

Note: `.github/workflows/ci.yml` path filters still reference `src/**`, which no longer exists after the
move to `crates/*` — CI won't auto-trigger on most source edits. Run the lint commands locally.

Nix users: `nix develop` provides the toolchain plus `android-tools` and the X11/Wayland runtime libs
the GUI needs.

## Architecture

Three crates:

- **`uad-core`** — all device/state logic, no UI. Depended on by both front-ends.
- **`uad-gui`** (bin `uad-ng`) — iced 0.14 GUI. Default features `wgpu,self-update,img`.
- **`uad-cli`** (bin `uad`) — clap subcommands plus a rustyline REPL (`uad repl`).

### uad-core

- `adb.rs` — type-state builder over the `adb` CLI: `ACommand::new().shell(serial).pm().list_packages_sys(..)`.
  Deliberately a **thin 1-to-1 wrapper**: no chaining, no synthesized commands, no piping (see the module
  doc for why). If you need a new ADB capability, extend these builders rather than reaching for
  `std::process::Command`. `PackageId::new` enforces the valid-package-name invariant.
- `sync.rs` — the layer above ADB: `Phone`/`User` discovery, `apply_pkg_state_commands` (maps
  wanted-state × current-state × Android SDK level → shell commands), state verification,
  cross-user-behavior detection, and OEM fallback strategies (`attempt_fallback`).
  `request_builder` re-validates the package name and returns no commands on failure — the
  no-injection guarantee lives at the sink, keep it there.
- `uad_lists.rs` — the package database. `load_debloat_lists(remote)` fetches from GitHub (retry 60×1s),
  writes to `CACHE_DIR`, falls back to the cached copy, then to the `include_str!`-embedded
  `resources/assets/uad_lists.json`. Defines `UadList`, `Removal`, `PackageState`.
- `utils.rs` — `fetch_packages` joins the live device package lists (`-s` system + `-3` third-party,
  each in all/enabled/disabled variants) against the debloat list to produce `Vec<CorePackage>`.
- `config.rs` — `config.toml` in `CONFIG_DIR`. Gotcha: `last_device_id` is a top-level scalar and
  **must stay the first struct field**, because TOML requires scalars before table headers
  (guarded by `test_last_device_roundtrip`).
- `save.rs` — JSON backups of uninstalled/disabled packages per device, restore command generation.
- `ai.rs` (fork-local) — posts to a local multi-provider proxy (`UAD_AI_PROXY_URL`, default
  `http://localhost:6500`; optional `UAD_AI_PROXY_KEY`) on `/structured` and `/chat`. Results are
  cached in `CACHE_DIR/ai_enrich.json`. Every failure path returns `None` — the app must work with
  the proxy down.
- `CONFIG_DIR` / `CACHE_DIR` are `LazyLock` statics under the OS dirs + `/uad`.

### uad-gui

Standard iced Elm loop. `gui.rs` holds `UadGui` (root state: device list, selected device, update state)
and dispatches to three views under `views/`: `list` (the apps screen), `settings`, `about`. Each view
owns its own `Message` enum; the root wraps them (`Message::AppsAction(..)` etc.) and sometimes calls
`self.update(..)` recursively to chain side effects.

`views/list.rs` (~2k lines) is the heart: package table, filters (list/state/removal/search/user-apps-only),
selection, the apply-selection modal, the AI enrichment + chat panel, and the
verify-then-fallback flow after each ADB action. Blocking ADB work is dispatched via `Task::perform`.

Styling: `theme.rs` (4 themes, palette cached in a `LazyLock` because iced calls `palette()` in a hot
loop) and `style.rs`.

**Text widgets:** `iced::widget::text` and `iced::widget::Text` are clippy-denied (`clippy.toml`).
Always use `crate::widgets::text::text`, which enables advanced shaping — see issues #523 / #858.

### uad-cli

`main.rs` defines the clap tree; `commands.rs` implements each subcommand against `uad-core`;
`repl.rs` is the interactive mode; `filters.rs` maps CLI enums onto core enums. All mutating
subcommands support `--dry-run`.

## Conventions

- Conventional Commits; trunk-based development (short-lived `feature/*`, `fix/*`, `deps/*` branches).
- If you mass-edit `uad_lists.json` (or anything) via a script or an LLM, say so explicitly in the PR
  description, including the exact command — this is a hard rule in `CONTRIBUTING.md`.
- Releases are tag-triggered (`git tag -s v1.2.3`); see `RELEASE.md`. Package-list updates ship
  without a release because the app fetches them at launch.
- Logs go to `CACHE_DIR/uadng.log` (GUI, `fern`).
