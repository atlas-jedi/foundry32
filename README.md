# Foundry32

[![CI](https://github.com/atlas-jedi/foundry32/actions/workflows/ci.yml/badge.svg)](https://github.com/atlas-jedi/foundry32/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/atlas-jedi/foundry32)](https://github.com/atlas-jedi/foundry32/releases/latest)

**Português:** [README.pt-BR.md](README.pt-BR.md)

A native Windows tool hub — **install** (straight off the web), **run** and
**uninstall** developer tools from one place. Old-school Windows Classic chrome
on the outside; modern plumbing underneath — HTTPS downloads with SHA-256
verification, no elevation required, DPI-aware, bilingual.

## How it works

Foundry32 reads a small catalog of tools, installs the ones you pick into your
own user profile, and launches them — no admin rights, nothing touched outside
`%LOCALAPPDATA%\Software Imperial\Foundry32`.

- **Install** downloads the tool's signed-off binary over HTTPS, streams it to a
  temp file, and verifies its **SHA-256** against the catalog before it's placed
  — a mismatched or tampered download never lands.
- **Run** launches the tool detached, in its own directory.
- **Update / Uninstall** are one click; a running tool is handled gracefully
  (updates swap the exe aside, uninstall asks you to close it first).

The catalog is fetched from the latest release and cached, with an embedded copy
as an offline fallback.

## Tools

| Tool | What it does |
|---|---|
| **MCP Console** | See and manage every MCP server your [Claude Code](https://code.claude.com) install knows about — with a clear answer to *"if I change this here, does it change on my other computers too?"* Labels each server's reach (account / machine / project), shows health, and edits the file-based ones via the official `claude mcp` CLI (never by hand-editing `~/.claude.json`). Env var **names** shown, values never read. |

More tools land in the catalog over time — the hub picks them up without needing
a new Foundry32 build.

## Install

Download `Foundry32-Setup-<version>-x86.exe` from the
[latest release](https://github.com/atlas-jedi/foundry32/releases/latest) and
run the wizard. Default folder:
`C:\Program Files (x86)\Software Imperial\Foundry32`. A portable zip is also
attached to each release.

> Coming from **MCP Hangar** (the standalone predecessor of MCP Console)?
> Uninstall it first — Foundry32 is a separate product and installs MCP Console
> for you from the catalog.

### Windows SmartScreen

Releases are not code-signed yet — free signing for open-source projects via the
[SignPath Foundation](https://signpath.org) is being set up
([details](docs/code-signing.md)). Until then, SmartScreen shows "Windows
protected your PC" on a fresh download: click **More info → Run anyway**. Every
release binary is built publicly by
[GitHub Actions](.github/workflows/release.yml) straight from a version tag, and
each one's SHA-256 is published in `SHA256SUMS.txt`, so you can audit exactly
what went into it — or build from source below.

## Build from source

```
rustup toolchain install stable-i686-pc-windows-gnu --profile minimal
cargo +stable-i686-pc-windows-gnu build --release --workspace
```

This produces `target/i686-pc-windows-gnu/release/foundry32.exe` and
`mcp-console.exe`. Local GNU builds also need i686 MinGW-w64 binutils (`as`,
`ar`, `windres`) on PATH — rustup's bundled toolchain covers linking only.
Release binaries are built by CI with `--target i686-pc-windows-msvc` (x86 —
smaller on disk and in RAM) and have no such requirement.

Headless modes for scripting/verification: `foundry32.exe --dump-catalog [file]`
and `--check-update [file]`; `mcp-console.exe --dump [file]`.

## License

[MIT](LICENSE) — © Software Imperial.
