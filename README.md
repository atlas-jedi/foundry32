# MCP Hangar

[![CI](https://github.com/atlas-jedi/mcp-hangar/actions/workflows/ci.yml/badge.svg)](https://github.com/atlas-jedi/mcp-hangar/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/atlas-jedi/mcp-hangar)](https://github.com/atlas-jedi/mcp-hangar/releases/latest)

**Português:** [README.pt-BR.md](README.pt-BR.md)

A native Windows GUI to see and manage every MCP server your
[Claude Code](https://code.claude.com) installation knows about — with a clear
answer to the question that matters: **"if I change this here, does it change on
my other computers too?"**

## Why

Claude Code MCP servers come from different places with very different reach:

| Scope shown | Where it lives | Reach |
|---|---|---|
| **Account (claude.ai)** | Connectors on claude.ai | **ALL machines** logged into the account |
| **User** | `~/.claude.json` (top level) | This computer only, all projects |
| **Local (project)** | `~/.claude.json` (`projects.<dir>`) | This computer only, one project |
| **Project (.mcp.json)** | `.mcp.json` in a repo | Everyone using that repo (via git) — not the account |
| **Plugin** | An installed Claude Code plugin | This computer only |

MCP Hangar lists them all side by side, labels the reach of each one, shows
health status, and lets you add / edit / remove the file-based ones (account
connectors are managed on claude.ai — the app links you there).

## Features

- Native Win32 UI (real controls, ~2 MB, low RAM), written in memory-safe Rust
- Account vs machine vs project reach labeling, with per-server explanation
- Health status from `claude mcp list` (connected / needs auth / pending)
- Add / edit / remove servers via the official `claude mcp` CLI — never by
  hand-editing `~/.claude.json` — with automatic timestamped backups in
  `~/.claude/backups/mcp-hangar`
- Env var **names** shown, values never read or displayed
- Bilingual UI (English / Português), switchable at runtime
- Update notification from GitHub Releases (notify-only, nothing auto-installs)

## Install

Download `MCP-Hangar-Setup-<version>-x86.exe` from the
[latest release](https://github.com/atlas-jedi/mcp-hangar/releases/latest) and
run the wizard. Default folder: `C:\Program Files (x86)\Software Imperial\MCP Hangar`.
A portable zip is also attached to each release.

Requires Claude Code installed (the app shells out to the `claude` CLI).

### Windows SmartScreen

Releases are not code-signed yet — free signing for open-source projects via
the [SignPath Foundation](https://signpath.org) is being set up
([details](docs/code-signing.md)). Until then, SmartScreen shows
"Windows protected your PC" on a fresh download: click
**More info → Run anyway**. Every release binary is built publicly by
[GitHub Actions](.github/workflows/release.yml) straight from a version tag,
so you can audit exactly what went into it — or build from source below.

## Build from source

```
rustup toolchain install stable-i686-pc-windows-gnu --profile minimal
cargo +stable-i686-pc-windows-gnu build --release
```

Local GNU builds also need i686 MinGW-w64 binutils (`as`, `ar`, `windres`) on PATH — rustup's bundled toolchain covers linking only. Release binaries do not have this requirement (built by CI with MSVC).

Release binaries are built by CI with `--target i686-pc-windows-msvc` (x86 —
smaller on disk and in RAM). Headless modes for scripting/verification:
`mcp-hangar.exe --dump [file]` and `mcp-hangar.exe --check-update [file]`.

## License

[MIT](LICENSE) — © Software Imperial.
