# MCP Hangar

[![CI](https://github.com/atlas-jedi/mcp-hangar/actions/workflows/ci.yml/badge.svg)](https://github.com/atlas-jedi/mcp-hangar/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/atlas-jedi/mcp-hangar)](https://github.com/atlas-jedi/mcp-hangar/releases/latest)

**English:** [README.md](README.md)

Interface gráfica nativa para Windows que mostra e gerencia todos os MCP
servers que o seu [Claude Code](https://code.claude.com) conhece — respondendo
com clareza a pergunta que importa: **"se eu alterar isto aqui, altera também
nos meus outros computadores?"**

## Por quê

Os MCP servers do Claude Code vêm de lugares diferentes, com alcances muito
diferentes:

| Escopo exibido | Onde vive | Alcance |
|---|---|---|
| **Conta (claude.ai)** | Connectors no claude.ai | **TODAS as máquinas** logadas na conta |
| **Usuário** | `~/.claude.json` (nível raiz) | Só este computador, todos os projetos |
| **Local (projeto)** | `~/.claude.json` (`projects.<dir>`) | Só este computador, um projeto |
| **Projeto (.mcp.json)** | `.mcp.json` num repositório | Quem usa o repo (via git) — não a conta |
| **Plugin** | Plugin instalado do Claude Code | Só este computador |

O MCP Hangar lista todos lado a lado, rotula o alcance de cada um, mostra o
status de saúde e permite adicionar / editar / remover os que são baseados em
arquivo (connectors de conta são gerenciados no claude.ai — o app te leva até lá).

## Recursos

- UI Win32 nativa (controles reais, ~2 MB, pouca RAM), escrita em Rust memory-safe
- Rotulagem de alcance conta vs máquina vs projeto, com explicação por servidor
- Status de saúde via `claude mcp list` (conectado / precisa autenticar / pendente)
- Adicionar / editar / remover via CLI oficial `claude mcp` — nunca editando
  `~/.claude.json` na mão — com backups automáticos com timestamp em
  `~/.claude/backups/mcp-hangar`
- Mostra os **nomes** das variáveis de ambiente; valores nunca são lidos nem exibidos
- UI bilíngue (Português / English), alternável em tempo de execução
- Notificação de atualização via GitHub Releases (só avisa, nada instala sozinho)

## Instalação

Baixe `MCP-Hangar-Setup-<versão>-x86.exe` na
[última release](https://github.com/atlas-jedi/mcp-hangar/releases/latest) e
siga o assistente. Pasta padrão: `C:\Program Files (x86)\Software Imperial\MCP Hangar`.
Cada release também traz um zip portátil.

Requer o Claude Code instalado (o app usa o CLI `claude`).

## Compilar do código-fonte

```
rustup toolchain install stable-i686-pc-windows-gnu --profile minimal
cargo +stable-i686-pc-windows-gnu build --release
```

Builds locais com GNU também precisam dos binutils do MinGW-w64 i686 (`as`, `ar`, `windres`) no PATH — o toolchain embutido do rustup cobre apenas a linkedição. Os binários de release não têm esse requisito (compilados no CI com MSVC).

Os binários de release são compilados no CI com `--target i686-pc-windows-msvc`
(x86 — menor em disco e RAM). Modos headless para script/verificação:
`mcp-hangar.exe --dump [arquivo]` e `mcp-hangar.exe --check-update [arquivo]`.

## Licença

[MIT](LICENSE) — © Software Imperial.
