//! Domain model: an MCP server entry and where it is configured.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// claude.ai connector — managed on the account, present on EVERY machine
    /// logged into the same claude.ai account.
    Account,
    /// Provided by a Claude Code plugin installed on this machine.
    Plugin,
    /// `mcpServers` at the top level of `~/.claude.json` — this machine, all projects.
    User,
    /// `.mcp.json` inside a project — shared with the team through git, not the account.
    Project { project_dir: String },
    /// `projects.<dir>.mcpServers` inside `~/.claude.json` — this machine, one project.
    Local { project_dir: String },
    /// Listed by the CLI but not found in any known config source.
    Unknown,
}

impl Scope {
    /// Entries this app can add/edit/remove through the `claude mcp` CLI.
    pub fn is_editable(&self) -> bool {
        matches!(self, Scope::User | Scope::Project { .. } | Scope::Local { .. })
    }

    /// Scope flag accepted by `claude mcp add/remove --scope <flag>`.
    pub fn cli_flag(&self) -> Option<&'static str> {
        match self {
            Scope::User => Some("user"),
            Scope::Project { .. } => Some("project"),
            Scope::Local { .. } => Some("local"),
            _ => None,
        }
    }

    /// Project directory this scope is bound to, when any.
    pub fn project_dir(&self) -> Option<&str> {
        match self {
            Scope::Project { project_dir } | Scope::Local { project_dir } => Some(project_dir),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Stdio,
    Http,
    Sse,
}

impl Transport {
    pub fn label(&self) -> &'static str {
        match self {
            Transport::Stdio => "stdio",
            Transport::Http => "HTTP",
            Transport::Sse => "SSE",
        }
    }

    pub fn cli_flag(&self) -> &'static str {
        match self {
            Transport::Stdio => "stdio",
            Transport::Http => "http",
            Transport::Sse => "sse",
        }
    }
}

#[derive(Debug, Clone)]
pub struct McpServer {
    pub name: String,
    pub scope: Scope,
    pub transport: Transport,
    /// Command line (stdio) or URL (http/sse).
    pub target: String,
    /// Environment variable NAMES only — values are secrets and are never read.
    pub env_keys: Vec<String>,
    /// Health status reported by `claude mcp list`, when available (raw CLI text).
    pub status: Option<String>,
    /// File that defines this entry, when file-based.
    pub source_file: Option<String>,
}
