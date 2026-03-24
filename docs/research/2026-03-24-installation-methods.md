# Research: Cross-Platform Rust CLI Distribution Methods

**Date:** 2026-03-24
**Scope:** How top Rust CLI tools distribute binaries; best primary install method for APEX

---

## Part 1: Tool-by-Tool Analysis

### 1. ripgrep (rg)

| Attribute | Detail |
|---|---|
| Primary method | OS package managers (`brew install ripgrep`, `apt install ripgrep`, `choco install ripgrep`) |
| Steps | 1 command |
| Cross-platform | macOS, Linux, Windows via native package managers |
| Pre-requisites | Package manager (brew, apt, choco, etc.) |
| Install time | 2-10s (pre-built binary from package manager) |
| Notes | Also provides pre-built tarballs on GitHub Releases. In Homebrew core (not a tap). No custom installer script. |

### 2. bat

| Attribute | Detail |
|---|---|
| Primary method | OS package managers (`brew install bat`, `apt install bat`) |
| Steps | 1 command |
| Cross-platform | macOS, Linux, Windows |
| Pre-requisites | Package manager |
| Install time | 2-10s |
| Notes | In Homebrew core and most Linux distro repos. Falls back to `cargo install --locked bat`. |

### 3. delta (git-delta)

| Attribute | Detail |
|---|---|
| Primary method | OS package managers (`brew install git-delta`, `dnf install git-delta`) |
| Steps | 1 command |
| Cross-platform | macOS, Linux, Windows |
| Pre-requisites | Package manager |
| Install time | 2-10s |
| Notes | Also on webinstall.dev: `curl https://webi.sh/delta | sh`. In Homebrew core. |

### 4. starship

| Attribute | Detail |
|---|---|
| Primary method | `curl -sS https://starship.rs/install.sh \| sh` |
| Steps | 1 command (pipe to shell) |
| Cross-platform | macOS, Linux. PowerShell script for Windows. |
| Pre-requisites | curl |
| Install time | 3-5s |
| Notes | Installs to /usr/local/bin by default (may need sudo). Also available via brew, cargo, scoop, etc. |

### 5. mise (formerly rtx)

| Attribute | Detail |
|---|---|
| Primary method | `curl https://mise.run \| sh` |
| Steps | 1 command (pipe to shell) |
| Cross-platform | macOS, Linux. Windows via scoop/winget/choco. |
| Pre-requisites | curl |
| Install time | 3-5s |
| Notes | Installs to ~/.local/bin/mise. Also brew, cargo, npm (`npx @jdxcode/mise`). |

### 6. uv (astral.sh)

| Attribute | Detail |
|---|---|
| Primary method | `curl -LsSf https://astral.sh/uv/install.sh \| sh` |
| Steps | 1 command (pipe to shell) |
| Cross-platform | macOS, Linux. PowerShell for Windows. |
| Pre-requisites | curl (no Python needed) |
| Install time | 3-5s |
| Notes | Uses cargo-dist-generated installer. Installs to ~/.local/bin. Modifies shell profile for PATH. Also brew, pip, conda, docker. |

### 7. deno

| Attribute | Detail |
|---|---|
| Primary method | `curl -fsSL https://deno.land/install.sh \| sh` |
| Steps | 1 command (pipe to shell) |
| Cross-platform | macOS, Linux. PowerShell for Windows. |
| Pre-requisites | curl |
| Install time | 3-5s |
| Notes | Installs to ~/.deno/bin. Also brew, choco, npm. |

### 8. bun

| Attribute | Detail |
|---|---|
| Primary method | `curl -fsSL https://bun.com/install \| bash` |
| Steps | 1 command (pipe to shell) |
| Cross-platform | macOS, Linux. PowerShell for Windows. |
| Pre-requisites | curl + unzip (Linux) |
| Install time | 3-5s |
| Notes | Installs to ~/.bun/bin. Also brew, npm, docker. |

### 9. zoxide

| Attribute | Detail |
|---|---|
| Primary method | OS package managers (`brew install zoxide`, `apt install zoxide`) |
| Steps | 1 command |
| Cross-platform | macOS, Linux, Windows |
| Pre-requisites | Package manager |
| Install time | 2-10s |
| Notes | Also cargo install, conda, etc. No custom installer script. |

### 10. cargo-binstall

| Attribute | Detail |
|---|---|
| Primary method | `cargo install cargo-binstall` (then use `cargo binstall <crate>`) |
| Steps | 2 commands (install binstall, then use it) |
| Cross-platform | Anywhere Rust/cargo exists |
| Pre-requisites | Rust toolchain |
| Install time | 2-3 min to install binstall; 3-5s per subsequent binary install |
| Notes | Fetches pre-built binaries from GitHub Releases. Falls back to cargo install. Requires `[package.metadata.binstall]` in Cargo.toml for custom URL patterns. |

---

## Part 2: Distribution Infrastructure Analysis

### cargo-binstall

**What it is:** A cargo subcommand that downloads pre-built binaries from GitHub Releases instead of compiling from source.

**How it works:**
1. Reads crate metadata from crates.io
2. Looks for release assets matching `{name}-{target}-v{version}.{archive-format}` at `{repo}/releases/download/v{version}/`
3. Falls back to quickinstall cache, then to `cargo install`

**Configuration (Cargo.toml):**
```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }.tar.gz"
bin-dir = "{ bin }{ binary-ext }"
pkg-fmt = "tgz"
```

**APEX compatibility:** Current release archive naming is `apex-{target}.tar.gz` (no version in filename). The default binstall pattern expects `{name}-{target}-v{version}`. Either rename archives or add explicit `pkg-url`.

**Verdict:** Good for Rust developers. Requires cargo as prerequisite. Not a primary install method.

### cargo-dist (Axo)

**What it is:** A release automation tool that generates CI workflows, shell/PowerShell installers, Homebrew formulas, npm packages, and updaters.

**What it generates:**
- `.github/workflows/release.yml` (build + publish pipeline)
- Shell installer script (curl-sh style) hosted on GitHub Releases
- PowerShell installer for Windows
- Homebrew formula (auto-updated)
- npm package
- Optional self-updater binary

**Key feature:** The generated shell installer is hosted as a release asset (not a raw repo file), so the URL is stable per-version:
```
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/USER/REPO/releases/latest/download/TOOL-installer.sh | sh
```

**How uv uses it:** Astral hosts the installer at `https://astral.sh/uv/install.sh` which redirects to the cargo-dist-generated installer on GitHub Releases.

**Verdict:** The gold standard for Rust CLI distribution. Replaces your hand-written release.yml + install.sh + Homebrew formula with auto-generated, tested versions. Used by uv, cargo-dist itself, and many others.

### npm Binary Shim Pattern

**How esbuild/turbo/Sentry CLI do it:**
1. Main package (`@scope/cli`) has `optionalDependencies` pointing to platform packages
2. Platform packages (`@scope/cli-darwin-arm64`, `@scope/cli-linux-x64`, etc.) each contain a single binary
3. npm/pnpm/yarn only download the matching platform package based on `os` and `cpu` fields
4. A `postinstall` script wires up the binary, with fallback download if optionalDependencies was skipped

**Verdict:** Powerful for JavaScript ecosystem reach. Complex to maintain (12+ packages for full platform matrix). Worth it only if JS developers are a primary audience.

### Homebrew: Tap vs Core

| | Tap | Core |
|---|---|---|
| Control | Full (your repo) | None (PR to homebrew-core) |
| Install command | `brew install user/tap/formula` | `brew install formula` |
| Requirements | GitHub repo | Significant usage, builds from source on all macOS versions + Linux, open-source license |
| Update process | You push | Homebrew bot auto-bumps |
| Trust level | Lower (third-party) | High (curated) |

**For APEX now:** Tap is correct. Core requires substantial community adoption.

### GitHub CLI Extensions

**How it works:** `gh extension install user/gh-tool` downloads a binary from the repo's releases, matching `{os}-{arch}` naming.

**Limitations:** Only works as a `gh` subcommand (`gh apex`), not standalone. Requires gh CLI installed. Not appropriate for a standalone tool.

**Verdict:** Not suitable for APEX.

---

## Part 3: Pattern Summary

Two dominant patterns emerge:

### Pattern A: "curl | sh" (uv, deno, bun, starship, mise)
```
curl -LsSf https://example.com/install.sh | sh
```
- Used by the fastest-growing modern CLI tools
- Single command, works everywhere curl exists
- Installs to ~/.local/bin (no sudo)
- Controversy: piping to shell is a security concern (mitigated by HTTPS, checksums, reproducible builds)
- Fast: 3-5 seconds

### Pattern B: Package managers first (ripgrep, bat, delta, zoxide)
```
brew install tool    # macOS
apt install tool     # Debian/Ubuntu
```
- Used by established tools that are in distro repos and Homebrew core
- Most familiar UX for users
- Requires being accepted into package manager ecosystems (slow process)
- Different commands per OS

---

## Part 4: Recommendation for APEX

### The Best Primary Install Method

**Use cargo-dist to generate a shell installer hosted on GitHub Releases, fronted by a vanity URL.**

The recommended primary command:

```
curl -LsSf https://apex.coverage.sh/install.sh | sh
```

Or if no vanity domain:

```
curl -LsSf https://github.com/sahajamoth/apex/releases/latest/download/apex-installer.sh | sh
```

### Why This Wins

| Criterion | Score |
|---|---|
| Works on macOS + Linux | Yes (shell script detects platform) |
| One command | Yes |
| No pipes to untrusted code | The script is auditable, versioned, checksummed (same pattern as uv, deno, bun) |
| No pre-installed tools | Only curl (present on 99%+ of macOS/Linux systems) |
| Fast | 3-5 seconds (downloads ~5MB binary) |
| No sudo | Installs to ~/.local/bin or ~/.apex/bin |
| Windows support | Separate PowerShell installer (same pattern as uv) |

### Why Not The Others

| Method | Why not primary |
|---|---|
| `brew install` | Requires Homebrew. Different command on Linux. Good secondary. |
| `cargo install` | Requires Rust toolchain. 5 min compile. Good for Rust devs only. |
| `cargo binstall` | Requires cargo + cargo-binstall. Niche. Good tertiary. |
| `npx @apex-coverage/cli` | Requires Node.js. Good for JS ecosystem reach. |
| `pipx install` | Requires Python. Niche. |
| `nix run` | Requires Nix. Niche. |
| Direct tarball | Multiple platform-specific URLs. Not user-friendly. |

### Implementation Plan

#### Step 1: Adopt cargo-dist

Run `cargo dist init` in the workspace. This will:
- Add `[workspace.metadata.dist]` to `Cargo.toml`
- Generate `.github/workflows/release.yml` (replaces current hand-written one)
- Generate shell + PowerShell installer scripts as release assets
- Optionally generate a Homebrew formula

#### Step 2: Add cargo-binstall metadata

In `crates/apex-cli/Cargo.toml`:
```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/apex-{ target }.tar.gz"
bin-dir = "{ bin }{ binary-ext }"
pkg-fmt = "tgz"
```

This makes `cargo binstall apex-cli` work with existing release assets (no archive renaming needed).

#### Step 3: Set up vanity URL (optional but recommended)

Route `https://apex.coverage.sh/install.sh` to the GitHub Releases installer script via a simple redirect (Cloudflare Pages, Netlify redirect, or S3 redirect rule). This is what uv does with `https://astral.sh/uv/install.sh`.

#### Step 4: Tier the install methods in README

```
# Quick install (macOS / Linux)
curl -LsSf https://github.com/sahajamoth/apex/releases/latest/download/apex-installer.sh | sh

# Homebrew
brew install sahajamoth/tap/apex

# Cargo (pre-built binary, no compile)
cargo binstall apex-cli

# Cargo (from source)
cargo install --git https://github.com/sahajamoth/apex

# npm
npx @apex-coverage/cli run --target ./src

# Nix
nix run github:sahajamoth/apex
```

### Install Priority Ranking (Final)

1. **curl installer** (primary -- cargo-dist generated, ~3s)
2. **brew** (secondary for macOS users -- tap for now)
3. **cargo binstall** (tertiary for Rust devs -- ~3s, pre-built)
4. **cargo install** (fallback -- 5 min compile)
5. **npx** (JS ecosystem reach)
6. **nix run** (Nix users)
7. **pipx** (Python ecosystem reach)

---

## Part 5: Security Considerations

### "curl | sh" Security

The `curl | sh` pattern is controversial but industry-standard for developer tools. Mitigations:

1. **HTTPS only** -- cargo-dist installer enforces `--proto '=https' --tlsv1.2`
2. **Checksums** -- installer verifies SHA256 of downloaded binary against checksums in the release
3. **Auditable** -- installer script is a release asset (versioned, immutable per release)
4. **No sudo** -- installs to user directory (~/.local/bin)
5. **Signed releases** -- GitHub's artifact attestation can sign release assets

### For users who refuse curl|sh

Provide explicit manual instructions:
```
# Download directly
wget https://github.com/sahajamoth/apex/releases/latest/download/apex-aarch64-apple-darwin.tar.gz
tar xzf apex-aarch64-apple-darwin.tar.gz
mv apex ~/.local/bin/
```

---

## Sources

- [ripgrep GitHub Releases](https://github.com/burntsushi/ripgrep/releases)
- [ripgrep Installation DeepWiki](https://deepwiki.com/BurntSushi/ripgrep/1.1-installation)
- [starship.rs installer](https://starship.rs/)
- [uv Installation Docs](https://docs.astral.sh/uv/getting-started/installation/)
- [mise installation](https://mise.jdx.dev/)
- [deno installation](https://docs.deno.com/runtime/getting_started/installation/)
- [bun installation](https://bun.com/docs/installation)
- [cargo-binstall GitHub](https://github.com/cargo-bins/cargo-binstall)
- [cargo-dist documentation](https://axodotdev.github.io/cargo-dist/)
- [cargo-dist shell installer docs](https://opensource.axo.dev/cargo-dist/book/installers/shell.html)
- [esbuild platform-specific binaries](https://deepwiki.com/evanw/esbuild/6.2-platform-specific-binaries)
- [Sentry: Publishing Binaries on npm](https://sentry.engineering/blog/publishing-binaries-on-npm)
- [Homebrew Acceptable Formulae](https://docs.brew.sh/Acceptable-Formulae)
- [Homebrew Taps docs](https://docs.brew.sh/Taps)
- [gh extension install docs](https://cli.github.com/manual/gh_extension_install)
