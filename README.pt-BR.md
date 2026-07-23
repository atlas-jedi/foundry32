# Foundry32

[![CI](https://github.com/atlas-jedi/foundry32/actions/workflows/ci.yml/badge.svg)](https://github.com/atlas-jedi/foundry32/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/atlas-jedi/foundry32)](https://github.com/atlas-jedi/foundry32/releases/latest)

**English:** [README.md](README.md)

Uma central de ferramentas nativa para Windows — **instale** (baixando direto da
web), **execute** e **desinstale** ferramentas de desenvolvimento num só lugar.
Estética Windows Classic old-school por fora; tecnologia moderna por dentro —
downloads HTTPS com verificação SHA-256, sem elevação, ciente de DPI, bilíngue.

## Como funciona

O Foundry32 lê um catálogo enxuto de ferramentas, instala as que você escolher
no seu perfil de usuário e as executa — sem privilégios de administrador, sem
tocar em nada fora de `%LOCALAPPDATA%\Software Imperial\Foundry32`.

- **Instalar** baixa o binário da ferramenta por HTTPS, grava num arquivo
  temporário e verifica o **SHA-256** contra o catálogo antes de instalar — um
  download corrompido ou adulterado nunca é aplicado.
- **Executar** abre a ferramenta destacada, no diretório dela.
- **Atualizar / Desinstalar** em um clique; uma ferramenta em execução é tratada
  com cuidado (a atualização move o exe para o lado, a desinstalação pede para
  fechá-la antes).

O catálogo é buscado da última release e mantido em cache, com uma cópia
embutida como fallback offline.

## Ferramentas

| Ferramenta | O que faz |
|---|---|
| **MCP Console** | Mostra e gerencia todos os MCP servers que o seu [Claude Code](https://code.claude.com) conhece — respondendo com clareza a *"se eu alterar isto aqui, altera também nos meus outros computadores?"* Rotula o alcance de cada servidor (conta / máquina / projeto), mostra o status de saúde e edita os baseados em arquivo via CLI oficial `claude mcp` (nunca editando `~/.claude.json` na mão). Mostra os **nomes** das variáveis de ambiente; valores nunca são lidos. |

Mais ferramentas entram no catálogo com o tempo — o hub as reconhece sem
precisar de uma nova versão do Foundry32.

## Instalação

Baixe `Foundry32-Setup-<versão>-x86.exe` na
[última release](https://github.com/atlas-jedi/foundry32/releases/latest) e siga
o assistente. Pasta padrão:
`C:\Program Files (x86)\Software Imperial\Foundry32`. Cada release também traz um
zip portátil.

> Vindo do **MCP Hangar** (o antecessor standalone do MCP Console)? Desinstale-o
> primeiro — o Foundry32 é um produto separado e instala o MCP Console para você
> a partir do catálogo.

### SmartScreen do Windows

As releases ainda não são assinadas digitalmente — a assinatura gratuita para
projetos open source via [SignPath Foundation](https://signpath.org) está sendo
configurada ([detalhes](docs/code-signing.md)). Até lá, o SmartScreen mostra "O
Windows protegeu o computador" ao executar um download recente: clique em **Mais
informações → Executar assim mesmo**. Todo binário de release é compilado
publicamente pelo [GitHub Actions](.github/workflows/release.yml) direto de uma
tag de versão, e o SHA-256 de cada um é publicado em `SHA256SUMS.txt` — dá para
auditar exatamente o que entrou nele, ou compilar do código-fonte abaixo.

## Compilar do código-fonte

```
rustup toolchain install stable-i686-pc-windows-gnu --profile minimal
cargo +stable-i686-pc-windows-gnu build --release --workspace
```

Isso produz `target/i686-pc-windows-gnu/release/foundry32.exe` e
`mcp-console.exe`. Builds locais com GNU também precisam dos binutils do
MinGW-w64 i686 (`as`, `ar`, `windres`) no PATH — o toolchain embutido do rustup
cobre apenas a linkedição. Os binários de release são compilados no CI com
`--target i686-pc-windows-msvc` (x86 — menor em disco e RAM) e não têm esse
requisito.

Modos headless para script/verificação: `foundry32.exe --dump-catalog [arquivo]`
e `--check-update [arquivo]`; `mcp-console.exe --dump [arquivo]`.

## Licença

[MIT](LICENSE) — © Software Imperial.
