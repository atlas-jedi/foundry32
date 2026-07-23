//! Bilingual (pt-BR / en) UI strings. All user-facing text lives here.
//!
//! Manual documents (`manual_doc_*`) use a tiny line-based markup rendered by
//! `gui::manual_dialog`: `# ` heading, `## ` subheading, `!! ` warning,
//! two-space indent for example/code lines, anything else is body text.

// The `Lang` selector and system detection are shared across the workspace.
// Re-exported so the rest of the console keeps referring to `crate::i18n::Lang`.
pub use foundry_common::lang::{detect_system_lang, Lang};

pub struct T {
    pub app_title: &'static str,
    pub col_num: &'static str,
    pub col_name: &'static str,
    pub col_scope: &'static str,
    pub col_reach: &'static str,
    pub col_type: &'static str,
    pub col_target: &'static str,
    pub col_status: &'static str,
    pub scope_account: &'static str,
    pub scope_plugin: &'static str,
    pub scope_user: &'static str,
    pub scope_project: &'static str,
    pub scope_local: &'static str,
    pub scope_unknown: &'static str,
    pub reach_account: &'static str,
    pub reach_machine: &'static str,
    pub reach_repo: &'static str,
    pub reach_unknown: &'static str,
    pub btn_add: &'static str,
    pub btn_edit: &'static str,
    pub btn_remove: &'static str,
    pub btn_refresh: &'static str,
    pub menu_file: &'static str,
    pub menu_file_prefs: &'static str,
    pub menu_file_exit: &'static str,
    pub menu_servers: &'static str,
    pub menu_srv_add: &'static str,
    pub menu_srv_edit: &'static str,
    pub menu_srv_remove: &'static str,
    pub menu_srv_refresh: &'static str,
    pub menu_srv_connectors: &'static str,
    pub menu_help: &'static str,
    pub menu_help_site: &'static str,
    pub menu_help_download: &'static str,
    pub menu_help_download_v: &'static str,
    pub menu_help_manual: &'static str,
    pub menu_help_about: &'static str,
    pub manual_title: &'static str,
    pub manual_nav_scopes: &'static str,
    pub manual_nav_types: &'static str,
    pub manual_close: &'static str,
    pub manual_doc_scopes: &'static str,
    pub manual_doc_types: &'static str,
    pub about_title: &'static str,
    pub about_body: &'static str,
    pub pref_title: &'static str,
    pub pref_section_interface: &'static str,
    pub pref_hint: &'static str,
    pub pref_ok: &'static str,
    pub lang_label: &'static str,
    pub status_left_ready: &'static str,
    pub status_servers: &'static str,
    pub status_update: &'static str,
    pub status_cli_running: &'static str,
    pub status_mutating: &'static str,
    pub status_error: &'static str,
    pub details_placeholder: &'static str,
    pub d_scope: &'static str,
    pub d_reach: &'static str,
    pub d_type: &'static str,
    pub d_target: &'static str,
    pub d_env: &'static str,
    pub d_source: &'static str,
    pub d_status: &'static str,
    pub d_none: &'static str,
    pub detail_reach_account: &'static str,
    pub detail_reach_machine: &'static str,
    pub detail_reach_repo: &'static str,
    pub detail_reach_unknown: &'static str,
    pub confirm_remove_title: &'static str,
    pub confirm_remove_body: &'static str,
    pub op_done: &'static str,
    pub op_error_title: &'static str,
    pub replace_removed_warning: &'static str,
    pub dlg_title_add: &'static str,
    pub dlg_title_edit: &'static str,
    pub dlg_step_identity: &'static str,
    pub dlg_step_scope: &'static str,
    pub dlg_step_connection: &'static str,
    pub dlg_name: &'static str,
    pub dlg_name_hint: &'static str,
    pub dlg_scope_user_radio: &'static str,
    pub dlg_scope_project_radio: &'static str,
    pub dlg_scope_local_radio: &'static str,
    pub dlg_dir: &'static str,
    pub dlg_known_fill: &'static str,
    pub dlg_dir_hint: &'static str,
    pub dlg_transport: &'static str,
    pub dlg_tr_stdio_radio: &'static str,
    pub dlg_tr_http_radio: &'static str,
    pub dlg_tr_sse_radio: &'static str,
    pub dlg_target_cmd: &'static str,
    pub dlg_target_cmd_hint: &'static str,
    pub dlg_target_url: &'static str,
    pub dlg_target_url_hint: &'static str,
    pub dlg_env: &'static str,
    pub dlg_env_hint: &'static str,
    pub dlg_env_edit_note: &'static str,
    pub dlg_headers: &'static str,
    pub dlg_headers_hint: &'static str,
    pub dlg_sec_summary: &'static str,
    pub dlg_sum_need_name: &'static str,
    pub dlg_sum_add: &'static str,
    pub dlg_sum_edit: &'static str,
    pub dlg_sum_scope_user: &'static str,
    pub dlg_sum_scope_project: &'static str,
    pub dlg_sum_scope_project_nodir: &'static str,
    pub dlg_sum_scope_local: &'static str,
    pub dlg_sum_scope_local_nodir: &'static str,
    pub dlg_sum_stdio: &'static str,
    pub dlg_sum_stdio_notarget: &'static str,
    pub dlg_sum_remote: &'static str,
    pub dlg_sum_remote_notarget: &'static str,
    pub dlg_sum_env: &'static str,
    pub dlg_backup_note: &'static str,
    pub dlg_back: &'static str,
    pub dlg_next: &'static str,
    pub dlg_ok: &'static str,
    pub dlg_cancel: &'static str,
    pub dlg_err_title: &'static str,
    pub dlg_err_name: &'static str,
    pub dlg_err_target: &'static str,
    pub dlg_err_dir: &'static str,
    pub dlg_err_env: &'static str,
    pub dlg_err_headers: &'static str,
    pub dlg_env_blank_confirm_title: &'static str,
    pub dlg_env_blank_confirm_body: &'static str,
    pub warn_claude_missing: &'static str,
}

static PT: T = T {
    app_title: "MCP Hangar",
    col_num: "#",
    col_name: "Nome",
    col_scope: "Escopo",
    col_reach: "Alcance",
    col_type: "Tipo",
    col_target: "Destino",
    col_status: "Status",
    scope_account: "Conta (claude.ai)",
    scope_plugin: "Plugin",
    scope_user: "Usuário",
    scope_project: "Projeto (.mcp.json)",
    scope_local: "Local (projeto)",
    scope_unknown: "Desconhecido",
    reach_account: "TODAS as máquinas da conta",
    reach_machine: "Só este computador",
    reach_repo: "Time (arquivo no repo)",
    reach_unknown: "—",
    btn_add: "Adicionar…",
    btn_edit: "Editar…",
    btn_remove: "Remover",
    btn_refresh: "Atualizar",
    menu_file: "&Arquivo",
    menu_file_prefs: "&Preferências…",
    menu_file_exit: "&Sair",
    menu_servers: "&Servidores",
    menu_srv_add: "&Adicionar…",
    menu_srv_edit: "&Editar…",
    menu_srv_remove: "&Remover",
    menu_srv_refresh: "Atualizar &lista\tF5",
    menu_srv_connectors: "&Connectors da conta (claude.ai)…",
    menu_help: "Aj&uda",
    menu_help_site: "&Site do projeto (GitHub)",
    menu_help_download: "&Baixar atualização",
    menu_help_download_v: "&Baixar atualização %V…",
    menu_help_manual: "&Manual…",
    menu_help_about: "So&bre o MCP Hangar…",
    manual_title: "Manual — MCP Hangar",
    manual_nav_scopes: "Escopos",
    manual_nav_types: "Tipos",
    manual_close: "Fechar",
    manual_doc_scopes: "\
# Escopos — onde um servidor vale

Um servidor MCP é um programa que dá habilidades extras ao Claude: ler sua agenda, acessar um banco de dados, consultar planilhas, e assim por diante. Cada servidor da lista está configurado em algum lugar — e esse lugar define ONDE ele vale.

Esse \"onde vale\" é o que chamamos de ESCOPO. Antes de adicionar ou remover um servidor, vale a pena entender qual escopo faz sentido — principalmente para não entregar, sem querer, uma ferramenta pessoal para outras pessoas.

## Conta (claude.ai)

São os servidores configurados no site claude.ai (Configurações › Conectores). Eles pertencem à CONTA, e não a um computador: qualquer máquina que fizer login nessa conta recebe o servidor automaticamente — o notebook, o PC de casa, o computador do trabalho.

!! Atenção: se a conta é compartilhada com outras pessoas (por exemplo, uma conta de equipe), todo mundo que usa essa conta enxerga e usa esses servidores.

O MCP Hangar apenas LISTA os conectores da conta — para criar ou remover, use o site (menu Servidores › Connectors da conta).

## Usuário

Vale para você NESTE computador, em todos os projetos e pastas. Fica gravado no arquivo .claude.json, dentro da sua pasta de usuário do Windows.

Não sincroniza com nuvem nenhuma: nenhum outro computador — e nenhuma outra pessoa — recebe nada.

Quando usar: ferramentas que você quer ter sempre à mão, em qualquer projeto. Ex.: um servidor de agenda ou de anotações pessoais.

## Projeto (.mcp.json)

Fica gravado num arquivo chamado .mcp.json DENTRO da pasta de um projeto. Esse arquivo normalmente é versionado junto com o código (git) — ou seja, COLEGAS que baixarem o repositório recebem o servidor também.

Quando usar: ferramentas que o time inteiro precisa naquele projeto. Ex.: o banco de dados de testes do sistema.

!! Nunca coloque senhas ou chaves pessoais no escopo Projeto — o arquivo vai para o repositório e outras pessoas vão ler.

## Local (projeto)

Vale só NESTE computador e só DENTRO de um projeto específico. Também fica no .claude.json da sua pasta de usuário (e não no repositório), portanto não vai para o git e ninguém mais recebe.

Quando usar: experimentos, ou credenciais pessoais que você usa dentro de um projeto compartilhado.

## Plugin

Servidor que veio junto com um plugin do Claude Code instalado nesta máquina. Quem gerencia é o próprio plugin — para removê-lo, desinstale o plugin correspondente.

## Desconhecido

O CLI do Claude listou o servidor, mas o MCP Hangar não encontrou o arquivo de configuração que o define. Normalmente é algo configurado de forma não convencional.

## Resumo rápido

  Conta    →  todos os computadores logados na conta
  Usuário  →  este computador, todos os projetos
  Projeto  →  o time inteiro (arquivo no repositório, via git)
  Local    →  este computador, um único projeto
  Plugin   →  gerenciado pelo plugin que o instalou",
    manual_doc_types: "\
# Tipos — como o Claude se conecta

O TIPO (também chamado de transporte) define COMO o Claude conversa com o servidor MCP: iniciando um programa aqui no seu computador ou acessando um endereço na internet.

A regra prática: quem fornece a ferramenta diz qual tipo usar. Se a instrução é \"rode este comando\", é stdio. Se é \"use esta URL\", é HTTP (ou SSE, se o provedor pedir).

## stdio — comando local

O Claude INICIA um programa no seu próprio computador e conversa com ele diretamente, sem passar pela rede. É o tipo mais comum para ferramentas instaladas na máquina.

No campo \"Comando\" você informa exatamente o que digitaria num terminal para iniciar o programa:

  node D:\\ferramentas\\meu-servidor.js
  npx -y @modelcontextprotocol/server-filesystem C:\\Docs

• O programa precisa estar instalado no computador (ex.: Node.js para comandos node/npx).

• Os dados trafegam só dentro da sua máquina — a não ser que o próprio programa acesse a internet.

• Variáveis de ambiente (CHAVE=VALOR) são a forma comum de passar chaves e senhas para o programa.

## HTTP — servidor remoto

O servidor roda em outro lugar (na internet ou na rede da empresa) e o Claude acessa por uma URL, como se fosse um site. Nada é instalado na sua máquina.

  https://mcp.exemplo.com/mcp

Normalmente exige autenticação, feita por cabeçalhos HTTP — o provedor do serviço informa a URL e a chave:

  Authorization: Bearer sua-chave-aqui

É o formato moderno para servidores remotos.

## SSE — servidor remoto (legado)

Também é remoto e acessado por URL, mas usa uma tecnologia mais antiga (Server-Sent Events). Muitos serviços estão trocando SSE por HTTP.

Use apenas se a documentação do serviço pedir SSE explicitamente.

## E as variáveis de ambiente e os cabeçalhos?

Variáveis de ambiente (CHAVE=VALOR) valem principalmente para servidores stdio: o programa as lê ao iniciar. É onde entram chaves de API e senhas.

Cabeçalhos HTTP (Chave: Valor) valem para HTTP e SSE: acompanham cada chamada à URL, normalmente para autenticação.

Nos dois casos, o MCP Hangar nunca mostra os valores na listagem — só os nomes.",
    about_title: "Sobre o MCP Hangar",
    about_body: "MCP Hangar %V\r\nGerenciador de servidores MCP do Claude Code.\r\n\r\ngithub.com/atlas-jedi/mcp-hangar\r\nLicença MIT — Software Imperial",
    pref_title: "Preferências",
    pref_section_interface: "Interface",
    pref_hint: "A alteração é aplicada imediatamente.",
    pref_ok: "OK",
    lang_label: "Idioma:",
    status_left_ready: "Pronto",
    status_servers: "%N servidores",
    status_update: "Atualização %V disponível",
    status_cli_running: "Consultando o Claude Code (claude mcp list)…",
    status_mutating: "Aplicando alteração via claude CLI…",
    status_error: "Erro: %E",
    details_placeholder: "Selecione um servidor para ver os detalhes.",
    d_scope: "Escopo",
    d_reach: "Alcance",
    d_type: "Tipo",
    d_target: "Destino",
    d_env: "Variáveis de ambiente (nomes)",
    d_source: "Definido em",
    d_status: "Status",
    d_none: "(nenhuma)",
    detail_reach_account: "ATENÇÃO — nível de CONTA: alterações feitas em claude.ai/settings/connectors refletem em TODOS os computadores logados nesta conta claude.ai. Este app não edita connectors — use Servidores › Connectors da conta para abri-los no navegador.",
    detail_reach_machine: "Nível de MÁQUINA: alterações afetam apenas este computador. Nenhum outro computador logado na conta é afetado.",
    detail_reach_repo: "Nível de PROJETO: definido em .mcp.json e compartilhado com quem usa o repositório (via git). Não sincroniza pela conta claude.ai.",
    detail_reach_unknown: "Origem não identificada — listado pelo CLI, mas não encontrado nos arquivos de configuração conhecidos.",
    confirm_remove_title: "Remover servidor",
    confirm_remove_body: "Remover \"%S\"? Um backup da configuração será salvo em ~\\.claude\\backups\\mcp-hangar antes.",
    op_done: "Alteração aplicada.",
    op_error_title: "Falha na operação",
    replace_removed_warning: "Atenção: a entrada original já tinha sido removida antes da falha — ela não existe mais. Backup da configuração em ~\\.claude\\backups\\mcp-hangar.",
    dlg_title_add: "Adicionar servidor MCP",
    dlg_title_edit: "Editar servidor MCP",
    dlg_step_identity: "1 · Identificação",
    dlg_step_scope: "2 · Onde vai valer",
    dlg_step_connection: "3 · Como conectar",
    dlg_name: "Nome do servidor:",
    dlg_name_hint: "Só letras, números, hífen e sublinhado. Ex.: google-calendar",
    dlg_scope_user_radio: "Usuário — em todos os projetos, mas só neste computador",
    dlg_scope_project_radio: "Projeto — vai para o .mcp.json do repositório (o time recebe via git)",
    dlg_scope_local_radio: "Local — só neste computador e só no projeto escolhido",
    dlg_dir: "Pasta do projeto:",
    dlg_known_fill: "Preencher com um projeto conhecido:",
    dlg_dir_hint: "Obrigatória para Projeto e Local — a pasta precisa existir no disco.",
    dlg_transport: "Tipo de conexão:",
    dlg_tr_stdio_radio: "Comando local (stdio)",
    dlg_tr_http_radio: "HTTP (remoto)",
    dlg_tr_sse_radio: "SSE (remoto, legado)",
    dlg_target_cmd: "Comando que inicia o servidor (programa + argumentos):",
    dlg_target_cmd_hint: "O mesmo que você digitaria num terminal. Ex.: node D:\\ferramentas\\meu-servidor.js",
    dlg_target_url: "URL do servidor:",
    dlg_target_url_hint: "Endereço fornecido pelo provedor. Ex.: https://mcp.exemplo.com/mcp",
    dlg_env: "Variáveis de ambiente (opcional) — uma por linha, CHAVE=VALOR:",
    dlg_env_hint: "Para chaves e senhas que o servidor precisa. Os valores nunca aparecem na listagem.",
    dlg_env_edit_note: "Atenção: por segurança, os valores atuais não são relidos ao editar — informe-os novamente.",
    dlg_headers: "Cabeçalhos HTTP (opcional) — um por linha, Chave: Valor:",
    dlg_headers_hint: "Normalmente para autenticação. Ex.: Authorization: Bearer sua-chave",
    dlg_sec_summary: "O que acontece ao salvar",
    dlg_sum_need_name: "Preencha o nome do servidor para ver o resumo.",
    dlg_sum_add: "O servidor \"%S\" será adicionado.",
    dlg_sum_edit: "O servidor \"%S\" será atualizado.",
    dlg_sum_scope_user: "Vale para todos os projetos deste computador; nenhum outro computador é afetado.",
    dlg_sum_scope_project: "Será gravado em %D\\.mcp.json — quem usa o repositório também recebe (via git).",
    dlg_sum_scope_project_nodir: "Será gravado no .mcp.json do projeto (escolha a pasta acima).",
    dlg_sum_scope_local: "Vale só neste computador, dentro do projeto %D.",
    dlg_sum_scope_local_nodir: "Vale só neste computador, dentro do projeto escolhido acima.",
    dlg_sum_stdio: "O Claude vai iniciar: %T",
    dlg_sum_stdio_notarget: "Falta informar o comando que inicia o servidor.",
    dlg_sum_remote: "O Claude vai conectar em: %T",
    dlg_sum_remote_notarget: "Falta informar a URL do servidor.",
    dlg_sum_env: "Variáveis de ambiente: %N.",
    dlg_backup_note: "Antes de qualquer alteração, um backup automático da configuração é salvo em ~\\.claude\\backups\\mcp-hangar.",
    dlg_back: "< Voltar",
    dlg_next: "Avançar >",
    dlg_ok: "Salvar",
    dlg_cancel: "Cancelar",
    dlg_err_title: "Dados inválidos",
    dlg_err_name: "Informe um nome (letras, números, - e _).",
    dlg_err_target: "Informe o comando ou a URL.",
    dlg_err_dir: "A pasta do projeto informada não existe.",
    dlg_err_env: "Linha de env inválida — use CHAVE=VALOR.",
    dlg_err_headers: "Linha de header inválida — use Chave: Valor.",
    dlg_env_blank_confirm_title: "Valores de env vazios",
    dlg_env_blank_confirm_body: "Estas variáveis serão salvas com valor VAZIO (valores não são lidos ao editar): %K\r\n\r\nContinuar mesmo assim?",
    warn_claude_missing: "CLI do Claude Code não encontrado — connectors da conta (claude.ai) não puderam ser listados.",
};

static EN: T = T {
    app_title: "MCP Hangar",
    col_num: "#",
    col_name: "Name",
    col_scope: "Scope",
    col_reach: "Reach",
    col_type: "Type",
    col_target: "Target",
    col_status: "Status",
    scope_account: "Account (claude.ai)",
    scope_plugin: "Plugin",
    scope_user: "User",
    scope_project: "Project (.mcp.json)",
    scope_local: "Local (project)",
    scope_unknown: "Unknown",
    reach_account: "ALL machines on the account",
    reach_machine: "This computer only",
    reach_repo: "Team (file in repo)",
    reach_unknown: "—",
    btn_add: "Add…",
    btn_edit: "Edit…",
    btn_remove: "Remove",
    btn_refresh: "Refresh",
    menu_file: "&File",
    menu_file_prefs: "&Preferences…",
    menu_file_exit: "E&xit",
    menu_servers: "&Servers",
    menu_srv_add: "&Add…",
    menu_srv_edit: "&Edit…",
    menu_srv_remove: "&Remove",
    menu_srv_refresh: "Refresh &list\tF5",
    menu_srv_connectors: "Account &connectors (claude.ai)…",
    menu_help: "&Help",
    menu_help_site: "Project &site (GitHub)",
    menu_help_download: "&Download update",
    menu_help_download_v: "&Download update %V…",
    menu_help_manual: "&Manual…",
    menu_help_about: "&About MCP Hangar…",
    manual_title: "MCP Hangar Manual",
    manual_nav_scopes: "Scopes",
    manual_nav_types: "Types",
    manual_close: "Close",
    manual_doc_scopes: "\
# Scopes — where a server applies

An MCP server is a program that gives Claude extra abilities: reading your calendar, querying a database, browsing spreadsheets, and so on. Every server in the list is configured somewhere — and that somewhere defines WHERE it applies.

That \"where\" is what we call the SCOPE. Before adding or removing a server, it is worth understanding which scope makes sense — mainly so you never hand a personal tool to other people by accident.

## Account (claude.ai)

Servers configured on the claude.ai website (Settings › Connectors). They belong to the ACCOUNT, not to a computer: every machine that logs into that account receives the server automatically — the laptop, the home PC, the work computer.

!! Careful: if the account is shared with other people (a team account, for example), everyone using that account sees and uses these servers.

MCP Hangar only LISTS account connectors — to create or remove them, use the website (Servers › Account connectors menu).

## User

Applies to you on THIS computer, in every project and folder. Stored in the .claude.json file inside your Windows user folder.

It does not sync to any cloud: no other computer — and no other person — receives anything.

When to use it: tools you want at hand everywhere, in any project. E.g.: a personal calendar or note-taking server.

## Project (.mcp.json)

Stored in a file called .mcp.json INSIDE a project folder. That file is usually versioned together with the code (git) — meaning COLLEAGUES who clone the repository receive the server too.

When to use it: tools the whole team needs in that project. E.g.: the system's test database.

!! Never put personal secrets or keys in the Project scope — the file goes into the repository and other people will read it.

## Local (project)

Applies only on THIS computer and only INSIDE one specific project. It also lives in the .claude.json of your user folder (not in the repository), so it never reaches git and nobody else receives it.

When to use it: experiments, or personal credentials you use inside a shared project.

## Plugin

A server that came bundled with a Claude Code plugin installed on this machine. The plugin manages it — to remove it, uninstall the corresponding plugin.

## Unknown

The Claude CLI listed the server, but MCP Hangar could not find the configuration file that defines it. Usually something configured in an unconventional way.

## Quick recap

  Account  →  every computer logged into the account
  User     →  this computer, every project
  Project  →  the whole team (file in the repository, via git)
  Local    →  this computer, a single project
  Plugin   →  managed by the plugin that installed it",
    manual_doc_types: "\
# Types — how Claude connects

The TYPE (also called transport) defines HOW Claude talks to the MCP server: by launching a program on your own computer or by reaching an address on the internet.

The practical rule: whoever provides the tool tells you which type to use. If the instruction is \"run this command\", it is stdio. If it is \"use this URL\", it is HTTP (or SSE, if the provider says so).

## stdio — local command

Claude LAUNCHES a program on your own computer and talks to it directly, without going through the network. It is the most common type for tools installed on the machine.

In the \"Command\" field you enter exactly what you would type in a terminal to start the program:

  node D:\\tools\\my-server.js
  npx -y @modelcontextprotocol/server-filesystem C:\\Docs

• The program must be installed on the computer (e.g.: Node.js for node/npx commands).

• Data stays inside your machine — unless the program itself reaches the internet.

• Environment variables (KEY=VALUE) are the usual way to hand keys and secrets to the program.

## HTTP — remote server

The server runs somewhere else (on the internet or on the company network) and Claude reaches it through a URL, like a website. Nothing is installed on your machine.

  https://mcp.example.com/mcp

It usually requires authentication, done through HTTP headers — the service provider gives you the URL and the key:

  Authorization: Bearer your-key-here

It is the modern format for remote servers.

## SSE — remote server (legacy)

Also remote and reached through a URL, but using an older technology (Server-Sent Events). Many services are replacing SSE with HTTP.

Use it only if the service documentation explicitly asks for SSE.

## What about environment variables and headers?

Environment variables (KEY=VALUE) matter mostly for stdio servers: the program reads them at startup. This is where API keys and secrets go.

HTTP headers (Key: Value) matter for HTTP and SSE: they travel with every call to the URL, usually for authentication.

In both cases, MCP Hangar never shows the values in the listing — only the names.",
    about_title: "About MCP Hangar",
    about_body: "MCP Hangar %V\r\nClaude Code MCP server manager.\r\n\r\ngithub.com/atlas-jedi/mcp-hangar\r\nMIT License — Software Imperial",
    pref_title: "Preferences",
    pref_section_interface: "Interface",
    pref_hint: "The change is applied immediately.",
    pref_ok: "OK",
    lang_label: "Language:",
    status_left_ready: "Ready",
    status_servers: "%N servers",
    status_update: "Update %V available",
    status_cli_running: "Querying Claude Code (claude mcp list)…",
    status_mutating: "Applying change via claude CLI…",
    status_error: "Error: %E",
    details_placeholder: "Select a server to see its details.",
    d_scope: "Scope",
    d_reach: "Reach",
    d_type: "Type",
    d_target: "Target",
    d_env: "Environment variables (names)",
    d_source: "Defined in",
    d_status: "Status",
    d_none: "(none)",
    detail_reach_account: "WARNING — ACCOUNT level: changes made at claude.ai/settings/connectors propagate to EVERY computer logged into this claude.ai account. This app does not edit connectors — use Servers › Account connectors to open them in your browser.",
    detail_reach_machine: "MACHINE level: changes affect this computer only. No other computer logged into the account is affected.",
    detail_reach_repo: "PROJECT level: defined in .mcp.json and shared with everyone using the repository (via git). It does not sync through the claude.ai account.",
    detail_reach_unknown: "Unidentified origin — listed by the CLI but not found in any known configuration file.",
    confirm_remove_title: "Remove server",
    confirm_remove_body: "Remove \"%S\"? A configuration backup will be saved to ~\\.claude\\backups\\mcp-hangar first.",
    op_done: "Change applied.",
    op_error_title: "Operation failed",
    replace_removed_warning: "Warning: the original entry had already been removed before the failure — it no longer exists. Configuration backup at ~\\.claude\\backups\\mcp-hangar.",
    dlg_title_add: "Add MCP server",
    dlg_title_edit: "Edit MCP server",
    dlg_step_identity: "1 · Identity",
    dlg_step_scope: "2 · Where it applies",
    dlg_step_connection: "3 · How to connect",
    dlg_name: "Server name:",
    dlg_name_hint: "Letters, digits, hyphen and underscore only. E.g.: google-calendar",
    dlg_scope_user_radio: "User — every project, but only on this computer",
    dlg_scope_project_radio: "Project — goes into the repo's .mcp.json (the team gets it via git)",
    dlg_scope_local_radio: "Local — this computer only, inside the chosen project",
    dlg_dir: "Project folder:",
    dlg_known_fill: "Fill from a known project:",
    dlg_dir_hint: "Required for Project and Local — the folder must exist on disk.",
    dlg_transport: "Connection type:",
    dlg_tr_stdio_radio: "Local command (stdio)",
    dlg_tr_http_radio: "HTTP (remote)",
    dlg_tr_sse_radio: "SSE (remote, legacy)",
    dlg_target_cmd: "Command that starts the server (program + arguments):",
    dlg_target_cmd_hint: "Exactly what you would type in a terminal. E.g.: node D:\\tools\\my-server.js",
    dlg_target_url: "Server URL:",
    dlg_target_url_hint: "Address given by the provider. E.g.: https://mcp.example.com/mcp",
    dlg_env: "Environment variables (optional) — one per line, KEY=VALUE:",
    dlg_env_hint: "For keys and secrets the server needs. Values never show up in the listing.",
    dlg_env_edit_note: "Careful: for safety, current values are not read back when editing — enter them again.",
    dlg_headers: "HTTP headers (optional) — one per line, Key: Value:",
    dlg_headers_hint: "Usually for authentication. E.g.: Authorization: Bearer your-key",
    dlg_sec_summary: "What happens when you save",
    dlg_sum_need_name: "Enter the server name to see the summary.",
    dlg_sum_add: "Server \"%S\" will be added.",
    dlg_sum_edit: "Server \"%S\" will be updated.",
    dlg_sum_scope_user: "Applies to every project on this computer; no other computer is affected.",
    dlg_sum_scope_project: "Stored in %D\\.mcp.json — everyone using the repository gets it too (via git).",
    dlg_sum_scope_project_nodir: "Stored in the project's .mcp.json (choose the folder above).",
    dlg_sum_scope_local: "Applies only on this computer, inside project %D.",
    dlg_sum_scope_local_nodir: "Applies only on this computer, inside the project chosen above.",
    dlg_sum_stdio: "Claude will launch: %T",
    dlg_sum_stdio_notarget: "The command that starts the server is still missing.",
    dlg_sum_remote: "Claude will connect to: %T",
    dlg_sum_remote_notarget: "The server URL is still missing.",
    dlg_sum_env: "Environment variables: %N.",
    dlg_backup_note: "Before any change, an automatic configuration backup is saved to ~\\.claude\\backups\\mcp-hangar.",
    dlg_back: "< Back",
    dlg_next: "Next >",
    dlg_ok: "Save",
    dlg_cancel: "Cancel",
    dlg_err_title: "Invalid data",
    dlg_err_name: "Enter a name (letters, digits, - and _).",
    dlg_err_target: "Enter the command or the URL.",
    dlg_err_dir: "The given project folder does not exist.",
    dlg_err_env: "Invalid env line — use KEY=VALUE.",
    dlg_err_headers: "Invalid header line — use Key: Value.",
    dlg_env_blank_confirm_title: "Empty env values",
    dlg_env_blank_confirm_body: "These variables will be saved with an EMPTY value (values are not read when editing): %K\r\n\r\nContinue anyway?",
    warn_claude_missing: "Claude Code CLI not found — account connectors (claude.ai) could not be listed.",
};

pub fn t(lang: Lang) -> &'static T {
    match lang {
        Lang::PtBr => &PT,
        Lang::En => &EN,
    }
}
