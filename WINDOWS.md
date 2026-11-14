# Native Windows (x64)

The native Windows path covers the shared Rust/Lua core, TUI plugin and the
Herdr desktop app. It does **not** require WSL, Git Bash, `sh`, `cat` or `cp`.
First baseline: Windows x64, MSVC, local workspaces. ARM64/x86, network-share
reliability, long-path policy, ConPTY focus/paste and Windows keyboard mappings
are not certified by the repository's Node tests. UNC paths retain their server
and share; they are never reinterpreted as a local `/share/...` file.

**Safety revision (2026-11-14, not product approval):** targeted native entry/
installer tests now cover two private app/core stand-ins with real Child-owned
Neovim daemons: foreign dirty buffers/comments survive reconcile and uninstall,
identical tab keys do not collide, a new runtime reuses the old daemon, and a
restored home pointing to another home's entry is rejected before stop/setup.
Core configuration changes and old unscoped markers fail closed. Lua is now
116/116 on Windows, including requested 0700 construction and injected partial
copy failure. Unix 0700/0644 permission assertions are written but **not run**;
Windows does not prove Unix permissions.

**Correctness revision (2026-11-14, awaiting independent re-review):** Git ignore
listing now streams NUL names over stdin/stdout and distinguishes no matches /
non-repositories from errors. Git/fff literal `~` names stay inside the repo;
Unix Windows-looking names retain their backslashes through tree/open/copy/delete/
git/search paths. Windows unit `TestDaemon` cleanup now retains the original
spawned Child handles; stale/unowned identities cannot grant PID-kill authority.
Unix signal cleanup is unchanged.

**Binding-edge revision (2026-11-14, awaiting independent re-review):** installer
binding comparison is ordinal/case-sensitive, including selector path spelling.
AppHome equality is now the native directory's volume + 128-bit file ID, shared
by instance scope and the installer's native `app-home-check`; no unconditional
lowercasing. The real-entry regression covers ordinary case aliases, preserved
identity after directory rename, distinct replacement directories, dirty daemons,
and wrong-home refusal before app/core calls. This machine refused enabling a
private case-sensitive directory (`0x00000005 Access denied`); the actual
case-sensitive two-home branch is written but **not run**. Incomplete bound
runtime/state selectors are rejected, not migrated.

**Persistent-binding revision (2026-11-14, writer validation, not approval):**
binding **v3** records installation-time AppHome volume serial + 128-bit directory
ID in the immutable sidecar. Every entry verifies the recorded path still names
that directory before commands, and scope uses that verified record. The real
unchanged-entry regression renames A to M, restores a new directory at A with the
original mod ownership, and leaves the entry/sidecar/digest byte-for-byte unchanged:
current/stop/fs/setup and installer upgrade/uninstall reject before app/core calls,
with old dirty Child, comment, marker/state and both directory snapshots preserved.
Restoring the original directory makes the original entry work again. Candidates
on fresh install **and** upgrade must pass schema/required-field validation before
staging; usage/exit 2 alone is not compatibility evidence. Native entry and
installer reject v1/v2, missing identity and unknown/invalid schema, without repair.

Current isolated Windows checks: fmt and no-run passed; full Rust **250 unit +
9 integration-target tests = 259 passed, 0 failed/ignored**, including four
repeated namespace checks, not nine independent CLI scenarios. Lua **116/116**;
both procedural Node suites; PS parse/transaction/real-Binary; and five native
protocol groups passed. The native suite uses actual Git and executable listing,
TreeModel hide/event filtering, 499 long names, real text/image cache in sidecar
home and APP_BIN stand-in -> real nvim -> Lua SELF re-entry. It clears inherited
entry/selectors and removes sh/cat/cp from PATH. Disabling entry::configure made
its positive control fail. `-Binary` now runs this suite instead of app-config.
These are headless protocol tests, **not** Herdr/GPUI/ConPTY acceptance. WSL has
no installed distribution and the existing Docker Linux engine was unavailable;
Unix filesystem/permission execution remains pending. Old unmarked-listener
migration and root ZIP traversal negative fixtures still need dedicated runs.

**Earlier local validation (2026-11-14, before safety review):** Rust 1.100.0 `x86_64-pc-windows-msvc` and
Neovim 0.12.6, with private test HOME/config/state/runtime directories:

- `cargo fmt --check` and `cargo test --locked --no-run`: passed.
- `cargo test --locked`: 248 unit + 7 integration-target tests passed, zero
  failed/ignored. This includes real Neovim named-pipe startup/concurrent reuse,
  close hooks and refusal to discard a daemon that swallows `qa!`. Three of the
  seven integration-target tests are the shared pipe-namespace check; the
  Unix-only daemon-GC/hung-process and sidebar CLI cases are **not** Windows coverage.
- Headless Lua: 115/115 passed, including Windows junction/copy guards.
- Both Node scripts and PowerShell ParseOnly/FixtureOnly/real `-Binary` sidecar
  smoke passed. CIM snapshot plus five process-race/error fixtures passed.

The earlier installer tests used stand-ins and only sidecar-backed `app-config`;
that smoke was insufficient and has now been replaced as described above. Native
Herdr/GPUI, ConPTY focus/paste, TUI keyboard mappings and full manual acceptance
remain **unverified**. No Unix regression suite, CI workflow or release build
was run in this Windows recovery stage; no Windows asset was published.

## Prerequisites

Install these yourself; the scripts do not install system tools:

- Herdr desktop app and `hc.exe` on PATH (or pass `-AppBin` for the app). For the
  TUI, `herdr.exe` and its Windows plugin support are required.
- Git for Windows' **git.exe** on PATH; no Unix shell is used.
- Neovim **0.10+**, `nvim.exe` on PATH. Get it from <https://neovim.io>. Alternatively
  set `[sidebar] nvim_bin = 'C:\Tools\Neovim\bin\nvim.exe'` in the plugin config
  (default `$HOME/.config/herdr-nvim/config.toml`; `HERDR_NVIM_CONFIG` overrides it).
  The missing-Neovim card only gives manual instructions on Windows; it does not
  invoke winget or download Neovim.
- Windows PowerShell 5.1 or PowerShell 7, in an x64 process.
- **Source builds only:** Rust's MSVC toolchain and Visual Studio C++ build tools.
  An existing x64 developer environment is used, otherwise `vswhere` discovers
  the installation (not a hardcoded Visual Studio year).

```powershell
Get-Command git.exe, nvim.exe, herdr-app.exe, hc.exe
nvim --version
```

Agent discovery still depends on Herdr/hc's integration status and cwd metadata.
This repository does not launch pi's npm shim or implement ConPTY. If Herdr does
not report the agent's foreground cwd after `cd`, relative-path accuracy depends
on that upstream metadata. Session fallback chooses the newest log for the agent
and cwd; simultaneous agents of the same kind/cwd can be ambiguous.

## Desktop installation

From a trusted checkout, open the app on the target home, then:

```powershell
git clone https://github.com/ChmaraX/herdr-nvim.git
Set-Location herdr-nvim
# Matching manifest-version release ZIP and SHA-256, once published:
.\install.ps1 -AppHome "$HOME\.config\herdr-app"
# This checkout before its Windows assets have been released, with existing tools:
.\install.ps1 -Source -AppHome "$HOME\.config\herdr-app"
# Or a binary you built yourself:
.\herdr-app\install.ps1 -Binary .\target\release\herdr-nvim.exe -AppHome "$HOME\.config\herdr-app"
```

`-AppBin C:\path\herdr-app.exe` and `-RuntimeBase C:\path\runtime` are optional.
Paths with spaces should be quoted. Release checksums detect corrupt downloads;
they are not signatures independent of the GitHub release channel. The root
script verifies and stages the bundle before running its installer. If the exact
asset is unavailable, it fails rather than mixing a different version's binary
and Lua. Use `-Source` explicitly for development.

The app's Files button opens the installed native tree and nvim. Windows installs
**no default global key bindings**: existing `keys.json` entries are untouched.
Use the app's key configuration to bind `mod.open:herdr-nvim`, `herdr-nvim.pick`
and optionally `herdr-nvim.reveal` to keys that work in your app/terminal. The
Unix README's `cmd` keys and TUI function-key transport are not a verified Windows
mapping. The welcome card displays bindings actually present in your keys file.

### Upgrade, rollback and uninstall

- Runtime copies live under `%LOCALAPPDATA%/herdr-nvim/app/<stamp>/`. Each is
  immutable: `bin/herdr-nvim.exe` plus `bin/herdr-nvim.app.json` supplies
  HOST/SELF/PLUGIN_ROOT/APP_HOME/APP_BIN plus a versioned core binding before
  any threads or children start.
  JS calls that `.exe` directly; picker finishers, watcher, pane argv and Lua
  inherit the same contract. No `.ps1`/`.cmd` file association is needed.
- Both mods are staged and checked, then swapped. Failed swaps/setup restore the
  previous whole mods. The shared `app-setup` keeps conflicting keys and patches
  only its layout footprint. These are not a transaction with concurrent edits
  by another app process: if setup partly fails, rerun it after fixing the reported
  error. A failed runtime remains safe to inspect; it is not automatically reused.
- Foreign same-name mods are refused. An exclusive installer lock prevents two
  cooperating installers racing. No upgrade stops daemons or overwrites old exe/Lua.
- **Windows never automatically prunes runtimes**, including on uninstall. This
  deliberately retains old daemons' Lua, watchers and locked exe files. Save and
  quit stale tab nvims to load the new runtime on the next open. Remove obsolete
  runtime directories only after verifying no process still uses them.

```powershell
.\install.ps1 -Uninstall -AppHome "$HOME\.config\herdr-app"
# --force equivalent, discards work only if the daemon can answer and quit:
.\install.ps1 -Uninstall -Force -AppHome "$HOME\.config\herdr-app"
```

Uninstall first verifies the ownership marker's immutable sidecar digest and
its **recorded installation-time** AppHome directory identity through the installed
native entry. A restored copy at the same path is a different object, not the old
installation. Missing/legacy metadata, replaced directories, linked home aliases
or a filesystem unable to supply a reliable directory ID are refused before
app/core/stop/setup/delete; no old sidecar is rewritten. Ordinary case aliases of
the same directory are accepted. Renaming a directory retains its identity, but
an existing sidecar requires its recorded path to resolve to that **same original
directory**, not merely to exist. Restore the original directory at that path to
use the original entry. There is no automatic relocation/rebinding: any permanent
migration is explicit, after saving/quitting via the original runtime and verifying
exit. Do not copy A's installed mods into B and uninstall B to migrate them.

After binding verification, uninstall stops first. Unsaved buffers, pending comments, a hung daemon or a legacy
marker make a normal uninstall fail while keeping the mods/runtime. A responsive
forced stop uses Neovim's `qa!`. There is **no automatic Windows process kill**
for hung/swallowing-qa daemons: the error preserves the marker and explains how to
save/quit or inspect the exact process manually. Never kill every `nvim.exe`.

### Upgrading older Windows daemons

Old pipe names (`\\.\pipe\herdr-nvim-<tab>`) were global. New names include a
stable user/runtime/host **and app/core-instance** namespace. App registry
enumeration and related state use the same instance subdirectory; TUI stays
separate. Binding format **v3** requires `core.app_home_identity.volume_serial`
(16 lowercase hexadecimal digits) and `directory_id` (32 lowercase hex digits,
nonzero). Rust captures them from an opened AppHome directory handle during
`app-core-binding`; the installer stores the result unchanged in the immutable
sidecar. Early configure checks the recorded path against this record; scope then
uses that same verified ID plus exact selectors, not a fresh path lookup, lowercase
path or runtime stamp. `app-home-check` compares the selected target to the recorded
ID, not two paths' current objects. The schema version and selector list have one
source, `herdr-app/binding-schema.json`, embedded by Rust and shipped in the ZIP for
the installer; test fixtures read it too. Both candidate and installed guards reject
old/unknown versions, missing required fields and structurally invalid metadata.
Links/reparse ancestors and unavailable/zero directory IDs fail closed. Explicit
`HERDR_NVIM_RUNTIME_DIR` and `HERDR_NVIM_STATE_DIR` remain test overrides (with
instance scoping inside) for valid bindings; they do not rescue incomplete metadata.

Binding format **v1** used a path-based identity; **v2** recomputed the current
file ID without persisting the expected one. Both are now rejected by new entry,
installed-entry guard and candidate admission, including on fresh homes, before
stop/setup/swap. No first-run identity backfill, version rewriting or silent new
namespace is supported. Save/quit via the original runtime, verify all old processes have
exited, then migrate manually. Do not edit the version or delete markers to bypass
this check. Old runtimes and sidecars are deliberately retained.

The native entry pins `hc.exe` and the installation-time `HC_SESSION`,
`HC_SOCKET`, HOME/USERPROFILE, XDG config/state/runtime, TEMP/TMP and Windows
app-data selectors, including unset values. Changing the calling terminal's
HC_SESSION cannot silently redirect an installed entry. The current hc contract
uses HC_SESSION/`connect --session`, HC_SOCKET and config.toml's socket setting;
AppHome alone is **not** a core identity. Default storage uses the bound path
context, not a caller's transient XDG/TEMP values. Capture and entry verification
use the same storage resolver: runtime requires at least one absolute captured
`XDG_RUNTIME_DIR`, `TMP`, `TEMP` (in that order); state requires `XDG_STATE_HOME`,
otherwise `HOME` or `USERPROFILE` plus `.local/state`. All-unset combinations are
rejected before commands run, including already-written incomplete bindings.
There is no bound-entry fallback to the calling terminal or OS temp defaults.

For this conservative revision, the entire core config at the captured config
home is fingerprinted locally (no content/credentials copied into the sidecar).
Any byte change, absence-to-presence or read failure blocks the entry, even an
unrelated config edit. The fingerprint detects configuration drift; it is not
security authentication. Bound selector paths and a configured socket must be
absolute; relative endpoints are refused rather than resolved against a caller's
working directory. Reinstallation also refuses to change an existing
binding rather than abandoning its active daemons. Comparisons are ordinal and
case-sensitive, even for path selectors that happen to name the same directory:
`HC_SESSION=A` to `a` or a path spelling-only change is still refused. Selectors
are not normalized to lowercase. Save and quit the original
nvims, verify their exit, and explicitly repair/migrate the old installation;
there is no automatic migration or runtime deletion. Ordinary version upgrades
with unchanged v3 binding continue to share the original daemon/state. This is a
trusted local-tool binding, not protection against malicious concurrent directory
replacement, file-ID reuse or cloned volumes. Network filesystems and directory
ID query failures have not been independently certified on this machine.

Old **unscoped v2** app markers are checked in the previous directory as well,
and unmarked old v2/global listeners block replacement. Old/raw or mismatched `.pipe` markers are
**not silently adopted, removed or replaced**. Save (`:wa`) and quit (`:qa`) the
old pane, verify its process and old `--listen` endpoint have exited, then remove
only the marker named in the error. A legacy listener without a marker also
blocks starting a replacement. CIM errors/timeouts mean “unknown”, not “gone”.
Never remove a marker just to bypass this check while the old process runs.

## TUI

```powershell
herdr plugin install ChmaraX/herdr-nvim
# In a development checkout, populate bin/herdr-nvim.exe explicitly:
.\herdr\install.ps1 -Source
```

The manifest hook first downloads the **exact manifest version** x64 executable
and SHA-256. It probes for the expected usage/exit code before replacement. A
running/locked executable can cause a safe replacement error: close the relevant
plugin pane and retry. Source mode respects `CARGO_TARGET_DIR`, `-Target` and
`-Profile` by reading Cargo's executable artifact, not guessing a directory.
See [TUI.md](TUI.md) for plugin configuration and actions. `doctor`'s Unix pane
injection/cleanup is not supported on Windows; use isolated tests below instead.

## Native file operations

Tree listing and out-of-home preview reads use `app-files` JSON/argv commands in
Rust (500 entries/folder, 2 MiB text, 30 MiB image). Listing uses Git's NUL stdin/
stdout protocol, not Windows argv for filenames. Non-repositories and no ignore
matches return an empty ignore list; missing Git / configuration / ownership / IO
errors fail the command rather than silently pretending nothing is ignored.
Folder and watcher paths use
Windows drive/UNC-aware helpers; `/` and `C:/` never collapse to a drive-relative
path. Rename/move/delete still run in the tab's Neovim to preserve its buffers,
undo and LSP integration and the existing outside-root confirmation.

Lua copies use libuv, with a preflight limit of 10,000 entries/64 levels; copying
into oneself, symlinks, junctions and special files is refused even after outside
confirmation. This avoids link cycles/escape rather than following them. Ordinary
editor operations are not a sandbox against an adversary concurrently replacing
filesystem links. A failed IO copy may leave a partial destination; inspect it
before retrying. On Unix, directories are constructed owner-only (0700), then
ordinary source rwx bits are restored children-first, also after a copy error;
special mode bits are not propagated. This preserves private 0700 trees rather
than opening them as 0755. Unix OS-level validation remains pending.
Network shares are not a first-release reliability guarantee.

## Develop and validate (no real user home)

```powershell
cargo fmt --check
cargo test --locked --no-run
cargo test --locked
cargo test --locked --test windows_app_safety
nvim --headless --noplugin -u NONE -i NONE -l tests/run.lua
node herdr-app/e2e/js-checks.mjs
node herdr-app/e2e/windows-paths.mjs
node herdr-app/e2e/native-protocol.mjs .\target\debug\herdr-nvim.exe
.\tests\windows-install.ps1 -ParseOnly
.\tests\windows-install.ps1 -FixtureOnly
.\tests\windows-process.ps1 -Snapshot   # read-only CIM check, no termination
.\tests\windows-install.ps1 -Binary .\target\debug\herdr-nvim.exe
```

Run these with a disposable HOME/USERPROFILE, APPDATA/LOCALAPPDATA,
XDG config/data/state/cache/runtime and TEMP/TMP. Keep `CARGO_HOME` and
`RUSTUP_HOME` explicitly pointed at the existing toolchain when changing home;
do not copy Cargo credentials. Prepend the existing Rust/Neovim bins only to
the test process PATH, and require `cargo`, `rustc`, `nvim`, `git`, `node` and
`powershell.exe` before running (missing tools are not a pass).

The fixture smoke uses private .NET stand-ins and a temporary app home. It checks
ownership, staged check failure, rollback, locked-runtime upgrade, unsaved refusal
and uninstall preservation, **not** Herdr/GPUI or the real Rust sidecar. Passing a
real `-Binary` additionally runs the native Git/IO/cache/child-context protocol
suite without UI. Rust CLI tests compile their own Windows fake `herdr.exe`; they
must never fall through to a user's real Herdr. The Windows CI requires tools
up front so missing nvim cannot make a nominally green suite of skipped tests.

Before UI acceptance, read `.agents/skills/verify/SKILL.md`, use a separate app
home/core session, record actual tool versions, and verify Files/tree/preview,
drive roots, Chinese/space paths, two tabs, unsaved/comment stop, toggle retention,
agent pick/comment/ref/paste, and restart/upgrade. Linux Docker, macOS UI harnesses
and local Node mocks are not native Windows UI evidence.
