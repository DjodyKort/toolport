//! MCP server catalog.
//!
//! Two layers, so users can add servers without hand-typing commands/URLs:
//!   1. A small hand-verified "popular" seed, bundled and offline.
//!   2. Live search against the official MCP Registry for the long tail.
//!
//! Both produce the same [`CatalogEntry`] shape, which the UI turns into a
//! registry server with one click - the existing auth flow then handles creds.

use crate::registry::{ArgBinding, ArgPart, LaunchConfig, LaunchInput};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) const REGISTRY_URL: &str = "https://registry.modelcontextprotocol.io/v0.1/servers";

/// Connect budget: fast to fail so a down/unreachable registry surfaces quickly.
const REGISTRY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Read budget: the registry's own search is slow on an uncached query (D-101,
/// 5-57s observed server-side), so this has to outlast that rather than the old
/// flat 20s, which failed a connection that was working, just slow.
const REGISTRY_READ_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a cached registry answer stays fresh before a repeat of the same
/// query re-hits the network instead of serving it straight back.
const REGISTRY_CACHE_TTL: Duration = Duration::from_secs(10 * 60);
/// Registry answers kept per process and on disk; the oldest query is dropped
/// past this.
const REGISTRY_CACHE_MAX_QUERIES: usize = 50;
/// A cached answer older than this is not worth showing even as an instant
/// placeholder, so it is dropped when the disk cache is loaded or written.
const REGISTRY_CACHE_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const REGISTRY_CACHE_FILE: &str = "catalog-search-cache.json";
const REGISTRY_CACHE_FILE_VERSION: u32 = 1;

/// One addable server: enough to create a registry entry, plus display metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub name: String,
    pub description: String,
    /// "stdio" | "http" | "sse"
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch: Option<LaunchConfig>,
    pub url: Option<String>,
    /// Env-var names the server needs (treated as secrets when added).
    pub env_keys: Vec<String>,
    /// "curated" | "registry"
    pub source: String,
    pub homepage: Option<String>,
    /// Publishing namespace from the official registry id, e.g. `io.github.acme`
    /// for `io.github.acme/widget`. A provenance signal (who published it), not a
    /// cryptographic guarantee. `None` for curated/user entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// Curated grouping for the browse view (e.g. "Databases"). Empty for registry
    /// and user entries, which surface flat in search results.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub category: String,
    /// Direct link to where the user creates this server's credential (e.g. the
    /// provider's API-token page). Powers the guided "go get your creds" step in
    /// Stacks (and the normal add flow). Curated entries only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials_url: Option<String>,
    /// One-line hint on what credential to create (scopes, what to paste).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_hint: Option<String>,
    /// Placeholder text for the URL field when the server is self-hosted or
    /// needs a user-specific endpoint. When present, the catalog UI opens
    /// ServerDialog instead of immediate-add so the user can enter their URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_hint: Option<String>,
}

/// A live MCP Registry call that did not come back with fresh results, carried
/// alongside whatever curated/cached entries [`search`] still has, instead of
/// replacing them. `kind` lets a caller tell a slow-but-reachable registry from
/// a genuine connection failure: "timeout" | "connectionFailed" | "other".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryError {
    pub kind: String,
    pub message: String,
}

impl RegistryError {
    fn new(kind: &str, message: impl Into<String>) -> Self {
        Self {
            kind: kind.to_string(),
            message: message.into(),
        }
    }
}

/// Result of [`search`]: curated/cached hits are never dropped just because the
/// live registry call failed, so `registry_error` is reported alongside them
/// rather than in their place.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSearch {
    pub entries: Vec<CatalogEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_error: Option<RegistryError>,
    /// Set by [`search_instant`] only: whether [`search`] would still call the
    /// live registry, which is when the local index is empty or stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_needed: Option<bool>,
}

/// Browse-view grouping for a curated server, keyed by name. Keeps the verified
/// entry list itself untouched; the UI orders the sections, not the arm order here.
fn category_for(name: &str) -> &'static str {
    match name {
        "GitHub"
        | "Vercel"
        | "Sentry"
        | "Cloudflare Docs"
        | "AWS"
        | "Kubernetes"
        | "Linode"
        | "Vercel (Full API)"
        | "Cloudflare (Full API)"
        | "Clerk (Full API)" => "Code & infrastructure",
        "Supabase" | "Neon" | "PostgreSQL" | "MongoDB" | "Elasticsearch" | "Qdrant" | "Redis" => {
            "Databases"
        }
        "Context7" | "DeepWiki" | "Microsoft Learn" | "Hugging Face" | "OpenRouter"
        | "Parallel Search" | "Brave Search" | "Exa" | "Tavily" | "Perplexity" | "DataForSEO" => {
            "Search & knowledge"
        }
        "Firecrawl" | "Apify" | "Browserbase" => "Web & automation",
        "Stripe" | "Stripe (Full API)" | "Notion" | "Composio" | "Linear" | "Atlassian"
        | "Airtable" | "Todoist" | "Slack" | "Resend" | "Figma" | "Postiz" | "Twilio" | "n8n"
        | "Langfuse" | "Postman" | "Trello" => "Apps & productivity",
        "Filesystem"
        | "Fetch"
        | "Git"
        | "Playwright"
        | "Sequential Thinking"
        | "Memory"
        | "Time"
        | "Chrome DevTools" => "Local tools",
        _ => "",
    }
}

/// Where to create the credential for a curated server, and a one-line hint, for
/// the guided "go get your creds" step in Stacks. Returns `(url, hint)`; an empty
/// `url` means there's no single page (a connection string you supply, OAuth, or
/// no auth) so only the hint shows. `None` = unknown / no guidance.
fn credentials_for(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        // Toolport Full-API overlays: a provider API key the overlay uses locally.
        "Stripe (Full API)" => (
            "https://dashboard.stripe.com/apikeys",
            "Create a secret or restricted API key with the access the agent needs.",
        ),
        "Vercel (Full API)" => (
            "https://vercel.com/account/tokens",
            "Create an access token (Account Settings > Tokens).",
        ),
        "Cloudflare (Full API)" => (
            "https://dash.cloudflare.com/profile/api-tokens",
            "Create an API token scoped to the zones and services the agent should touch.",
        ),
        "Clerk (Full API)" => (
            "https://dashboard.clerk.com/last-active?path=api-keys",
            "Copy your Secret Key (Clerk Dashboard > API Keys).",
        ),
        // Token-based: the agent gets an API key Toolport vaults.
        "Linode" => (
            "https://cloud.linode.com/profile/tokens",
            "Create a Personal Access Token with read/write on the resources you need (Linodes, Volumes, Databases).",
        ),
        "AWS" => (
            "https://console.aws.amazon.com/iam/home#/security_credentials",
            "Create an access key (ID + secret) for an IAM user scoped to what the agent should touch.",
        ),
        "MongoDB" => (
            "https://cloud.mongodb.com",
            "Paste your MongoDB connection string (Atlas: Database > Connect > Drivers).",
        ),
        "Exa" => ("https://dashboard.exa.ai/api-keys", "Create an API key."),
        "Perplexity" => (
            "https://www.perplexity.ai/settings/api",
            "Create an API key (needs a small credit balance).",
        ),
        "DataForSEO" => (
            "https://app.dataforseo.com/api-access",
            "Copy your API login and password from the API Access dashboard.",
        ),
        "OpenRouter" => ("https://openrouter.ai/keys", "Create an API key."),
        "Qdrant" => (
            "https://cloud.qdrant.io",
            "Create a free cluster, then copy its URL and an API key (Cluster > API Keys).",
        ),
        "Redis" => (
            "",
            "Enter a redis:// or rediss:// connection URL in Launch setup. It is vaulted and passed as one argument.",
        ),
        "Hugging Face" => (
            "https://huggingface.co/settings/tokens",
            "Authenticate when prompted, or paste a read token.",
        ),
        "Resend" => ("https://resend.com/api-keys", "Create an API key with send access."),
        "Figma" => (
            "https://www.figma.com/settings",
            "Create a personal access token (Settings > Security > Personal access tokens).",
        ),
        "Slack" => (
            "https://api.slack.com/apps",
            "Create a Slack app, add a bot token (xoxb-...), and grab your team id.",
        ),
        "Twilio" => (
            "https://console.twilio.com",
            "Copy your Account SID, API Key SID, and API Secret from the Twilio Console.",
        ),
        "Postiz" => (
            "https://postiz.pro/settings/developers",
            "Create an API key in Settings > Developers > Public API, then paste it as the server's auth token.",
        ),
        "Langfuse" => (
            "https://langfuse.com/docs/api-and-data-platform/features/mcp-server",
            "Use your instance's /api/public/mcp URL. Paste Basic followed by base64(public-key:secret-key) as the server's auth token.",
        ),
        // Config you supply (no single token page).
        "PostgreSQL" => (
            "",
            "Enter your Postgres connection URL in Launch setup. It is vaulted and passed as one argument.",
        ),
        "Kubernetes" => ("", "Uses your local kubeconfig (~/.kube/config); nothing to paste."),
        "Filesystem" => (
            "",
            "Enter one allowed directory in Launch setup. Add more directories as literal arguments if needed.",
        ),
        // OAuth: authorize in the browser, no manual token.
        "GitHub" | "Vercel" | "Sentry" | "Notion" | "Linear" | "Stripe" | "Postman" | "Trello" => (
            "",
            "OAuth: click Authenticate when prompted; no manual token needed.",
        ),
        // No auth at all.
        "Fetch" | "Context7" | "Microsoft Learn" => ("", "No credential needed."),
        _ => return None,
    })
}

/// The hand-verified popular set. Hosted (URL) servers are favored here because
/// their endpoints are far more stable than package names. The live registry
/// covers the long tail; this set is what most people reach for first.
pub fn curated() -> Vec<CatalogEntry> {
    // Remote servers, keyed by transport. Most hosted MCPs are streamable-http;
    // a few still use SSE endpoints.
    let http = |name: &str, desc: &str, url: &str, home: &str| CatalogEntry {
        name: name.to_string(),
        description: desc.to_string(),
        transport: "http".to_string(),
        command: None,
        args: vec![],
        launch: None,
        url: Some(url.to_string()),
        env_keys: vec![],
        source: "curated".to_string(),
        homepage: Some(home.to_string()),
        publisher: None,
        category: String::new(),
        credentials_url: None,
        setup_hint: None,
        url_hint: None,
    };
    // Self-hosted server: the user supplies the URL (shown as placeholder).
    // transport is http because the server speaks MCP over HTTP, but the URL
    // is None — the catalog UI opens ServerDialog so the user enters their
    // instance endpoint before the server is created.
    let self_hosted = |name: &str, desc: &str, url_hint: &str, home: &str| CatalogEntry {
        name: name.to_string(),
        description: desc.to_string(),
        transport: "http".to_string(),
        command: None,
        args: vec![],
        launch: None,
        url: None,
        env_keys: vec![],
        source: "curated".to_string(),
        homepage: Some(home.to_string()),
        publisher: None,
        category: String::new(),
        credentials_url: None,
        setup_hint: None,
        url_hint: Some(url_hint.to_string()),
    };
    // Local (stdio) servers: `command` + args, with any required secret env keys.
    let cmd = |name: &str, desc: &str, command: &str, args: &[&str], env: &[&str], home: &str| {
        CatalogEntry {
            name: name.to_string(),
            description: desc.to_string(),
            transport: "stdio".to_string(),
            command: Some(command.to_string()),
            args: args.iter().map(|s| s.to_string()).collect(),
            launch: None,
            url: None,
            env_keys: env.iter().map(|s| s.to_string()).collect(),
            source: "curated".to_string(),
            homepage: Some(home.to_string()),
            publisher: None,
            category: String::new(),
            credentials_url: None,
            setup_hint: None,
            url_hint: None,
        }
    };

    let mut list = vec![
        // --- Payments & commerce ---
        http("Stripe", "Payments, customers, charges, and balances.", "https://mcp.stripe.com", "https://docs.stripe.com/mcp"),
        cmd("Stripe (Full API)", "Toolport overlay: all 587 Stripe endpoints as intent-named tools, with the full write coverage the official MCP lacks (your API key, runs locally).", "npx", &["-y", "toolport-mcp-servers", "stripe"], &["STRIPE_API_KEY"], "https://github.com/btsouth/toolport-mcp-servers"),
        // --- Code, deploy & infra ---
        http("GitHub", "Repos, issues, PRs, and code search.", "https://api.githubcopilot.com/mcp/", "https://github.com/github/github-mcp-server"),
        http("Vercel", "Projects, deployments, and logs on Vercel.", "https://mcp.vercel.com", "https://vercel.com/docs/mcp/vercel-mcp"),
        cmd("Vercel (Full API)", "Toolport overlay: 333 Vercel endpoints including the writes the official MCP omits (env vars, domains/DNS, the deploy lifecycle).", "npx", &["-y", "toolport-mcp-servers", "vercel"], &["VERCEL_TOKEN"], "https://github.com/btsouth/toolport-mcp-servers"),
        http("Sentry", "Errors, issues, and releases from Sentry.", "https://mcp.sentry.dev/mcp", "https://docs.sentry.io"),
        http("Cloudflare Docs", "Search Cloudflare's documentation.", "https://docs.mcp.cloudflare.com/mcp", "https://developers.cloudflare.com/agents/model-context-protocol/"),
        cmd("Cloudflare (Full API)", "Toolport overlay: 357 Cloudflare control-plane endpoints as named tools (DNS, email routing, zones, WAF, SSL, cache, R2, Access) for per-tool approval.", "npx", &["-y", "toolport-mcp-servers", "cloudflare"], &["CLOUDFLARE_API_TOKEN"], "https://github.com/btsouth/toolport-mcp-servers"),
        cmd("Clerk (Full API)", "Toolport overlay: 224 Clerk Backend API endpoints (users, orgs, sessions, invitations), vs the official 2-tool docs server.", "npx", &["-y", "toolport-mcp-servers", "clerk"], &["CLERK_SECRET_KEY"], "https://github.com/btsouth/toolport-mcp-servers"),
        cmd("AWS", "AWS service APIs through the AWS Labs API MCP server.", "uvx", &["awslabs.aws-api-mcp-server@latest"], &["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"], "https://github.com/awslabs/mcp"),
        cmd("Kubernetes", "Inspect and manage Kubernetes clusters via your kubeconfig.", "npx", &["-y", "mcp-server-kubernetes"], &[], "https://github.com/Flux159/mcp-server-kubernetes"),
        cmd("Linode", "Manage Linode (Akamai) cloud: instances, volumes, NodeBalancers, databases, and networking.", "npx", &["-y", "@takashito/linode-mcp-server"], &["LINODE_API_TOKEN"], "https://github.com/takashito/linode-mcp-server"),
        cmd("Chrome DevTools", "Control and inspect a live Chrome browser: traces, screenshots, network, console.", "npx", &["-y", "chrome-devtools-mcp@latest"], &[], "https://github.com/ChromeDevTools/chrome-devtools-mcp"),
        // --- Databases ---
        http("Supabase", "Query and manage your Supabase projects.", "https://mcp.supabase.com/mcp", "https://supabase.com/docs/guides/getting-started/mcp"),
        http("Neon", "Serverless Postgres: branches, queries, projects.", "https://mcp.neon.tech/mcp", "https://neon.tech/docs/ai/neon-mcp-server"),
        cmd("PostgreSQL", "Query a Postgres database (requires a connection URL).", "npx", &["-y", "@modelcontextprotocol/server-postgres", "<launch-input>"], &[], "https://github.com/modelcontextprotocol/servers-archived"),
        cmd("MongoDB", "Query and manage MongoDB databases.", "npx", &["-y", "mongodb-mcp-server"], &["MDB_MCP_CONNECTION_STRING"], "https://github.com/mongodb-js/mongodb-mcp-server"),
        cmd("Elasticsearch", "Search and analytics over your Elasticsearch cluster.", "npx", &["-y", "@elastic/mcp-server-elasticsearch"], &["ES_URL", "ES_API_KEY"], "https://github.com/elastic/mcp-server-elasticsearch"),
        cmd("Qdrant", "Vector search and memory for RAG: store and query embeddings in Qdrant.", "uvx", &["mcp-server-qdrant"], &["QDRANT_URL", "QDRANT_API_KEY", "COLLECTION_NAME"], "https://github.com/qdrant/mcp-server-qdrant"),
        cmd("Redis", "Inspect and manage a Redis database.", "uvx", &["--from", "redis-mcp-server@latest", "redis-mcp-server", "--url", "<launch-input>"], &[], "https://github.com/redis/mcp-redis"),
        // --- Project management & docs ---
        http("Notion", "Search and edit Notion pages and databases.", "https://mcp.notion.com/mcp", "https://developers.notion.com"),
        http("Postman", "Manage Postman workspaces, collections, and environments.", "https://mcp.postman.com/minimal", "https://github.com/postmanlabs/postman-mcp-server"),
        http("Composio", "Connect AI agents to 1,000+ apps (Gmail, Slack, GitHub, Notion, Linear, and more).", "https://connect.composio.dev/mcp", "https://composio.dev"),
        http("Trello", "Boards, lists, cards, checklists, and workspace search.", "https://mcp.trello.com/v1", "https://github.com/atlassian/trello-mcp-server"),
        http("Linear", "Issues, projects, and cycles in Linear.", "https://mcp.linear.app/mcp", "https://linear.app/docs"),
        http("Atlassian", "Jira issues and Confluence pages.", "https://mcp.atlassian.com/v2/mcp?tools=all", "https://support.atlassian.com/atlassian-ai-gateway/docs/get-started-with-the-atlassian-remote-mcp-server/"),
        cmd("Airtable", "Read and write records in your Airtable bases.", "npx", &["-y", "airtable-mcp-server"], &["AIRTABLE_API_KEY"], "https://github.com/domdomegg/airtable-mcp-server"),
        cmd("Todoist", "Manage Todoist tasks and projects.", "npx", &["-y", "@abhiz123/todoist-mcp-server"], &["TODOIST_API_TOKEN"], "https://github.com/abhiz123/todoist-mcp-server"),
        // --- Communication ---
        cmd("Slack", "Read and send Slack messages and manage channels.", "npx", &["-y", "@modelcontextprotocol/server-slack"], &["SLACK_BOT_TOKEN", "SLACK_TEAM_ID"], "https://github.com/modelcontextprotocol/servers"),
        cmd("Twilio", "Send SMS, make calls, and manage Twilio messaging and voice.", "npx", &["-y", "@twilio-alpha/mcp", "<launch-input>"], &[], "https://github.com/twilio-labs/mcp"),
        http("Postiz", "Schedule and publish social media posts across platforms.", "https://mcp.postiz.com/mcp", "https://docs.postiz.com/mcp/setup"),
        // --- Knowledge & search ---
        http("Context7", "Up-to-date docs and code examples for libraries.", "https://mcp.context7.com/mcp", "https://github.com/upstash/context7"),
        http("DeepWiki", "Ask questions about any public GitHub repo. No auth.", "https://mcp.deepwiki.com/mcp", "https://deepwiki.com"),
        http("Microsoft Learn", "Search official Microsoft and Azure documentation and code samples. No auth.", "https://learn.microsoft.com/api/mcp", "https://learn.microsoft.com/en-us/training/support/mcp"),
        http("Hugging Face", "Models, datasets, and Spaces on Hugging Face.", "https://huggingface.co/mcp", "https://huggingface.co/settings/mcp"),
        http("OpenRouter", "Live model intelligence: list and compare models, prices, and your credits.", "https://mcp.openrouter.ai/mcp", "https://openrouter.ai/blog/announcements/openrouter-mcp-server/"),
        http("Parallel Search", "Live web search and clean content from URLs. No account or API key required.", "https://search.parallel.ai/mcp", "https://docs.parallel.ai/integrations/mcp/search-mcp"),
        cmd("Brave Search", "Web search via the Brave Search API.", "npx", &["-y", "@brave/brave-search-mcp-server"], &["BRAVE_API_KEY"], "https://github.com/brave/brave-search-mcp-server"),
        cmd("Exa", "AI-native web search built for agents.", "npx", &["-y", "exa-mcp-server"], &["EXA_API_KEY"], "https://github.com/exa-labs/exa-mcp-server"),
        cmd("Tavily", "Web search and content extraction built for LLMs.", "npx", &["-y", "tavily-mcp"], &["TAVILY_API_KEY"], "https://github.com/tavily-ai/tavily-mcp"),
        cmd("Perplexity", "Ask Perplexity for cited, up-to-date answers.", "npx", &["-y", "@perplexity-ai/mcp-server"], &["PERPLEXITY_API_KEY"], "https://github.com/perplexityai/modelcontextprotocol"),
        cmd("DataForSEO", "SEO data: SERP tracking, keyword research, and competitor analysis.", "npx", &["-y", "dataforseo-mcp-server"], &["DATAFORSEO_USERNAME", "DATAFORSEO_PASSWORD"], "https://dataforseo.com"),
        cmd("Firecrawl", "Web scraping and data extraction from websites.", "npx", &["-y", "firecrawl-mcp"], &["FIRECRAWL_API_KEY"], "https://github.com/firecrawl/firecrawl-mcp-server"),
        cmd("Apify", "Run Apify actors for web scraping and automation.", "npx", &["-y", "@apify/actors-mcp-server"], &["APIFY_TOKEN"], "https://github.com/apify/actors-mcp-server"),
        cmd("Browserbase", "Cloud headless browsers agents can drive.", "npx", &["-y", "@browserbasehq/mcp"], &["BROWSERBASE_API_KEY", "BROWSERBASE_PROJECT_ID", "GEMINI_API_KEY"], "https://github.com/browserbase/mcp-server-browserbase"),
        // --- Email & comms already above; Design ---
        cmd("Figma", "Turn Figma designs into code (Framelink).", "npx", &["-y", "figma-developer-mcp", "--stdio"], &["FIGMA_API_KEY"], "https://github.com/GLips/Figma-Context-MCP"),
        // --- Email ---
        cmd("Resend", "Send transactional email through Resend.", "npx", &["-y", "resend-mcp"], &["RESEND_API_KEY"], "https://resend.com/docs"),
        // --- Self-hosted (user supplies URL) ---
        self_hosted("n8n", "Trigger, manage, and edit n8n workflows via MCP.", "https://your-instance.com/mcp-server/http", "https://n8n.io"),
        self_hosted("Langfuse", "Prompt management and observability. Use /api/public/mcp with Basic auth; see setup docs.", "https://your-langfuse.com/api/public/mcp", "https://langfuse.com/docs/api-and-data-platform/features/mcp-server"),
        // --- Local utilities (no account needed) ---
        cmd("Filesystem", "Read and write files in directories you allow.", "npx", &["-y", "@modelcontextprotocol/server-filesystem", "<launch-input>"], &[], "https://github.com/modelcontextprotocol/servers"),
        cmd("Fetch", "Fetch a URL and return its content as markdown.", "uvx", &["mcp-server-fetch"], &[], "https://github.com/modelcontextprotocol/servers"),
        cmd("Git", "Read, search, and manipulate a local Git repo.", "uvx", &["mcp-server-git"], &[], "https://github.com/modelcontextprotocol/servers"),
        cmd("Playwright", "Drive a real browser for testing and scraping.", "npx", &["-y", "@playwright/mcp@latest"], &[], "https://github.com/microsoft/playwright-mcp"),
        cmd("Sequential Thinking", "Structured step-by-step reasoning for hard problems.", "npx", &["-y", "@modelcontextprotocol/server-sequential-thinking"], &[], "https://github.com/modelcontextprotocol/servers"),
        cmd("Memory", "A knowledge graph the agent reads and writes across sessions.", "npx", &["-y", "@modelcontextprotocol/server-memory"], &[], "https://github.com/modelcontextprotocol/servers"),
        cmd("Time", "Current time and timezone conversions.", "uvx", &["mcp-server-time"], &[], "https://github.com/modelcontextprotocol/servers"),
    ];
    for e in &mut list {
        let template = e
            .name
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>();
        let mut launch = LaunchConfig {
            template: Some(template),
            revision: Some(1),
            ..Default::default()
        };
        let mut add_input = |key: &str, label: &str, secret: bool| {
            launch.inputs.push(LaunchInput {
                key: key.into(),
                label: label.into(),
                secret,
                required: true,
                value: None,
            });
        };
        match e.name.as_str() {
            "Twilio" => {
                add_input("TWILIO_ACCOUNT_SID", "Account SID", false);
                add_input("TWILIO_API_KEY", "API Key SID", true);
                add_input("TWILIO_API_SECRET", "API Secret", true);
                launch.bindings.push(ArgBinding {
                    index: 2,
                    parts: vec![
                        ArgPart::Input {
                            key: "TWILIO_ACCOUNT_SID".into(),
                        },
                        ArgPart::Literal { value: "/".into() },
                        ArgPart::Input {
                            key: "TWILIO_API_KEY".into(),
                        },
                        ArgPart::Literal { value: ":".into() },
                        ArgPart::Input {
                            key: "TWILIO_API_SECRET".into(),
                        },
                    ],
                });
                launch.revision = Some(2);
            }
            "PostgreSQL" => {
                add_input("POSTGRES_URL", "Postgres connection URL", true);
                launch.bindings.push(ArgBinding {
                    index: 2,
                    parts: vec![ArgPart::Input {
                        key: "POSTGRES_URL".into(),
                    }],
                });
                launch.revision = Some(2);
            }
            "Redis" => {
                add_input("REDIS_URL", "Redis connection URL", true);
                launch.bindings.push(ArgBinding {
                    index: 4,
                    parts: vec![ArgPart::Input {
                        key: "REDIS_URL".into(),
                    }],
                });
            }
            "Filesystem" => {
                add_input("ALLOWED_DIRECTORY", "Allowed directory", false);
                launch.bindings.push(ArgBinding {
                    index: 2,
                    parts: vec![ArgPart::Input {
                        key: "ALLOWED_DIRECTORY".into(),
                    }],
                });
                launch.revision = Some(2);
            }
            "Browserbase" => {
                launch.required_env = vec![
                    "BROWSERBASE_API_KEY".into(),
                    "BROWSERBASE_PROJECT_ID".into(),
                    "GEMINI_API_KEY".into(),
                ];
                launch.revision = Some(2);
            }
            "Qdrant" => {
                // The tools accept a collection name per call when no default
                // is configured. QDRANT_API_KEY is also optional for local or
                // unsecured clusters; this preset uses a URL connection.
                launch.required_env = vec!["QDRANT_URL".into()];
                launch.revision = Some(2);
            }
            "AWS" => launch.revision = Some(2),
            "Atlassian" => launch.revision = Some(2),
            "Perplexity" | "Brave Search" => launch.revision = Some(2),
            _ => {}
        }
        // These stdio packages cannot authenticate or complete setup without
        // their declared keys. Optional/alternative credentials (AWS's provider
        // chain, MongoDB's connect-later flow, Qdrant's API key) stay optional.
        if matches!(
            e.name.as_str(),
            "Stripe (Full API)"
                | "Vercel (Full API)"
                | "Cloudflare (Full API)"
                | "Clerk (Full API)"
                | "Linode"
                | "Elasticsearch"
                | "Airtable"
                | "Todoist"
                | "Slack"
                | "Brave Search"
                | "Exa"
                | "Tavily"
                | "Perplexity"
                | "DataForSEO"
                | "Firecrawl"
                | "Apify"
                | "Figma"
                | "Resend"
        ) {
            launch.required_env = e.env_keys.clone();
        }
        e.launch = Some(launch);
        e.category = category_for(&e.name).to_string();
        if let Some((url, hint)) = credentials_for(&e.name) {
            e.credentials_url = (!url.is_empty()).then(|| url.to_string());
            e.setup_hint = Some(hint.to_string());
        }
    }
    list
}

/// The popular set shown by default: the curated catalog.
pub fn popular() -> Vec<CatalogEntry> {
    curated()
}

/// Filter a catalog list by a query (name, description, or category). Substring
/// match, plus an all-terms fallback so multi-word queries still hit. Empty query
/// = all.
fn filter_catalog(list: Vec<CatalogEntry>, query: &str) -> Vec<CatalogEntry> {
    let matches = catalog_matcher(query);
    list.into_iter().filter(|e| matches(e)).collect()
}

/// The predicate behind [`filter_catalog`], so a large list can be filtered by
/// reference and only the hits cloned.
pub(crate) fn catalog_matcher(query: &str) -> impl Fn(&CatalogEntry) -> bool {
    let q = query.trim().to_lowercase();
    move |e| {
        if q.is_empty() {
            return true;
        }
        let hay = format!(
            "{} {} {}",
            e.name.to_lowercase(),
            e.description.to_lowercase(),
            e.category.to_lowercase()
        );
        hay.contains(&q) || q.split_whitespace().all(|t| hay.contains(t))
    }
}

/// Popular entries (user + curated) matching a query.
pub fn search_curated(query: &str) -> Vec<CatalogEntry> {
    filter_catalog(popular(), query)
}

/// Search the catalog: the user's picks + curated matches first (highest
/// quality), then the local registry index and, only while that index is empty
/// or stale, live MCP Registry results for the long tail, de-duplicated by
/// name. This is why popular picks like Vercel always surface even when the
/// registry's own search doesn't return them.
///
/// A registry failure (slow or unreachable) never drops the curated matches
/// already found in this same call (D-101): it is reported as `registry_error`
/// alongside whatever entries are available instead of in their place.
pub fn search(query: &str) -> CatalogSearch {
    search_with(
        REGISTRY_URL,
        query,
        REGISTRY_CONNECT_TIMEOUT,
        REGISTRY_READ_TIMEOUT,
        REGISTRY_CACHE_TTL,
    )
}

/// [`search`] without the network: curated matches plus the local registry
/// index and whatever the registry cache (memory or disk, however stale) already
/// holds for this query. Lets a caller paint results at once and then let
/// [`search`] refresh them.
pub fn search_instant(query: &str) -> CatalogSearch {
    search_instant_with(
        REGISTRY_URL,
        query,
        persists_to_disk(REGISTRY_URL),
        index_path_for(REGISTRY_URL).as_deref(),
    )
}

fn search_instant_with(
    base_url: &str,
    query: &str,
    persist: bool,
    index_path: Option<&Path>,
) -> CatalogSearch {
    let (mut extra, live_needed) = index_lookup(index_path, query);
    if !query.trim().is_empty() {
        extra.extend(
            cache_lookup(&cache_key(base_url, query), persist)
                .map(|(entries, _)| entries)
                .unwrap_or_default(),
        );
    }
    CatalogSearch {
        live_needed: Some(live_needed),
        ..merge_search_results(query, extra)
    }
}

/// The index is only consulted for the live registry: a mock endpoint's tests
/// must not read the user's data dir.
fn index_path_for(base_url: &str) -> Option<PathBuf> {
    persists_to_disk(base_url)
        .then(crate::catalog_index::default_path)
        .flatten()
}

/// The index hits for `query` and whether a live registry call is still needed.
fn index_lookup(index_path: Option<&Path>, query: &str) -> (Vec<CatalogEntry>, bool) {
    let Some(path) = index_path else {
        return (Vec::new(), true);
    };
    let index = crate::catalog_index::load(path);
    let hits = if query.trim().is_empty() {
        Vec::new()
    } else {
        index.search(query)
    };
    (hits, index.needs_live())
}

/// [`search`] parameterized over the registry endpoint, timeouts and cache TTL,
/// so tests can point it at a local mock server with budgets measured in
/// milliseconds instead of minutes.
fn search_with(
    base_url: &str,
    query: &str,
    connect_timeout: Duration,
    read_timeout: Duration,
    cache_ttl: Duration,
) -> CatalogSearch {
    search_with_index(
        base_url,
        query,
        connect_timeout,
        read_timeout,
        cache_ttl,
        index_path_for(base_url).as_deref(),
    )
}

fn search_with_index(
    base_url: &str,
    query: &str,
    connect_timeout: Duration,
    read_timeout: Duration,
    cache_ttl: Duration,
    index_path: Option<&Path>,
) -> CatalogSearch {
    if query.trim().is_empty() {
        return merge_search_results(query, Vec::new());
    }
    let (mut registry_entries, live_needed) = index_lookup(index_path, query);
    if !live_needed {
        return merge_search_results(query, registry_entries);
    }
    let (live_entries, registry_error) = registry_search_cached_from(
        base_url,
        query,
        connect_timeout,
        read_timeout,
        cache_ttl,
        persists_to_disk(base_url),
    );
    if let (Some(path), None) = (index_path, &registry_error) {
        crate::catalog_index::merge_live(path, &live_entries);
    }
    registry_entries.extend(live_entries);
    CatalogSearch {
        registry_error,
        ..merge_search_results(query, registry_entries)
    }
}

/// Curated matches first, then registry hits de-duplicated by name, ranked.
fn merge_search_results(query: &str, registry_entries: Vec<CatalogEntry>) -> CatalogSearch {
    let mut out = search_curated(query);
    let mut seen: std::collections::HashSet<String> =
        out.iter().map(|e| e.name.to_lowercase()).collect();
    for e in registry_entries {
        if seen.insert(e.name.to_lowercase()) {
            out.push(e);
        }
    }
    rank_search_results(&mut out, query);
    CatalogSearch {
        entries: out,
        registry_error: None,
        live_needed: None,
    }
}

pub(crate) fn rank_search_results(entries: &mut [CatalogEntry], query: &str) {
    let query = query.trim().to_lowercase();
    entries.sort_by(|a, b| {
        catalog_match_score(b, &query)
            .cmp(&catalog_match_score(a, &query))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

fn catalog_match_score(entry: &CatalogEntry, query: &str) -> i32 {
    let name = entry.name.to_lowercase();
    let description = entry.description.to_lowercase();
    let category = entry.category.to_lowercase();
    let mut score = if entry.source == "curated" { 10_000 } else { 0 };
    if name == query {
        score += 5_000;
    } else if name.starts_with(query) {
        score += 3_000;
    } else if name.contains(query) {
        score += 2_000;
    } else if query.split_whitespace().all(|term| name.contains(term)) {
        score += 1_500;
    } else if category.contains(query) {
        score += 800;
    } else if description.contains(query) {
        score += 500;
    }
    if entry.url.is_some() {
        score += 100;
    }
    if entry.homepage.is_some() {
        score += 25;
    }
    score
}

/// npm/yarn/pnpm remote specs that `npx -y` will fetch outside the registry.
/// `:` itself stays allowed so docker `host:port/image` and OCI `image:tag` work.
fn is_remote_package_spec(spec: &str) -> bool {
    let lower = spec.to_ascii_lowercase();
    lower.contains("://")
        || lower.starts_with("github:")
        || lower.starts_with("gitlab:")
        || lower.starts_with("bitbucket:")
        || lower.starts_with("gist:")
        || lower.starts_with("git+")
        || lower.starts_with("npm:")
        || lower.starts_with("jsr:")
        || lower.starts_with("file:")
        || lower.starts_with("http:")
        || lower.starts_with("https:")
}

/// npm-only: specs that make `npx -y` fetch code from somewhere other than the named
/// registry, in the spellings npm accepts BEYOND a leading protocol.
///
/// [`is_remote_package_spec`] only inspects the start of the combined
/// `identifier@version` string, so every form here reached `npx -y` from a malicious or
/// MITM'd registry entry: `attacker/payload` (npm reads an unscoped slash name as
/// GitHub shorthand), `lodash@github:attacker/payload` (a hosted-git alias as the
/// version range), `github.com/attacker/payload`, and `git@github.com:attacker/x.git`.
///
/// Applied on the npm path only. `user/image:tag` is a normal Docker reference, so the
/// slash rule cannot be global without dropping valid OCI entries.
fn npm_spec_escapes_registry(spec: &str) -> bool {
    let lower = spec.to_ascii_lowercase();
    // SCP-style git, which carries no `://` for is_remote_package_spec to catch.
    if lower.starts_with("git@") {
        return true;
    }
    // A hosted-git alias ANYWHERE, not just as a prefix: npm resolves it from the
    // version half too.
    if ["github:", "gitlab:", "bitbucket:", "gist:", "git+"]
        .iter()
        .any(|alias| lower.contains(alias))
    {
        return true;
    }
    // Hosted URLs with the scheme left off.
    if ["github.com/", "gitlab.com/", "bitbucket.org/"]
        .iter()
        .any(|host| lower.starts_with(host) || lower.contains(&format!("@{host}")))
    {
        return true;
    }
    // An unscoped `user/repo` is GitHub shorthand to npm, while `@scope/name` is a
    // real package. Judge the NAME half: a scoped name's own `@` must not be mistaken
    // for the version separator.
    let name = match lower.strip_prefix('@') {
        Some(rest) => match rest.find('@') {
            Some(i) => &lower[..=i],
            None => lower.as_str(),
        },
        None => lower.split('@').next().unwrap_or(&lower),
    };
    !name.starts_with('@') && name.contains('/')
}

/// A registry package spec safe to pass as an npx/uvx/docker argument: non-empty, no
/// leading dash (flag injection), bounded length, and only the characters real
/// package names use. Nothing that could become a separate flag, a shell token,
/// or a github:/URL install that bypasses the named registry.
fn is_safe_package_id(spec: &str) -> bool {
    !spec.is_empty()
        && !spec.starts_with('-')
        && spec.len() <= 200
        && spec.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '@' | '/' | '-' | '_' | '.' | '+' | ':')
        })
        && !is_remote_package_spec(spec)
}

/// Friendly title when present; fall back to the namespaced id.
fn registry_display_name(server: &Value) -> String {
    let id = server.get("name").and_then(|v| v.as_str()).unwrap_or("");
    server
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(id)
        .to_string()
}

/// Turn one registry `server` object into a catalog entry. Prefers a hosted
/// remote (simplest to connect), else the first installable package.
fn map_server(server: &Value) -> Option<CatalogEntry> {
    let id = server.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let name = registry_display_name(server);
    if name.is_empty() {
        return None;
    }
    let description = server
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let homepage = server
        .get("repository")
        .and_then(|r| r.get("url"))
        .and_then(|v| v.as_str())
        // SECURITY: this flows to the OS opener in the UI. A registry entry could set
        // it to file:// (Windows SMB -> NTLM-hash leak) or a custom-scheme handler
        // URI; only hand real web links onward. Mirrors the remotes[].url guard below.
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        .map(String::from);
    // Registry ids are namespaced (`io.github.acme/widget`); the namespace tells
    // you who published it - a provenance signal we surface in the catalog.
    let publisher = id.split_once('/').map(|(ns, _)| ns.to_string());

    if let Some(remote) = server
        .get("remotes")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
    {
        if let Some(url) = remote.get("url").and_then(|v| v.as_str()) {
            // SECURITY: only real HTTP(S) endpoints from a registry entry; a
            // javascript:/file:/data: URL has no business here.
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return None;
            }
            let ty = remote.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let transport = if ty.contains("sse") { "sse" } else { "http" };
            return Some(CatalogEntry {
                name,
                description,
                transport: transport.to_string(),
                command: None,
                args: vec![],
                launch: None,
                url: Some(url.to_string()),
                env_keys: vec![],
                source: "registry".to_string(),
                homepage,
                publisher,
                category: String::new(),
                credentials_url: None,
                setup_hint: None,
                url_hint: None,
            });
        }
    }

    if let Some(pkg) = server
        .get("packages")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
    {
        let registry_type = pkg
            .get("registryType")
            .or_else(|| pkg.get("registry_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("npm");
        let identifier = pkg.get("identifier").and_then(|v| v.as_str()).unwrap_or("");
        if identifier.is_empty() {
            return None;
        }
        let version = pkg.get("version").and_then(|v| v.as_str());
        let spec = match version {
            Some(v) if !v.is_empty() && v != "latest" => {
                // npm/PyPI pin with `@`. Docker treats `@` as a content digest
                // (`image@sha256:…`), so OCI tags must use `identifier:version` (SBS-784).
                if matches!(registry_type, "oci" | "docker") {
                    if v.contains(':') {
                        format!("{identifier}@{v}")
                    } else {
                        format!("{identifier}:{v}")
                    }
                } else {
                    format!("{identifier}@{v}")
                }
            }
            _ => identifier.to_string(),
        };
        // SECURITY: this spec becomes an argument to npx/uvx/docker for a one-click
        // install. Reject anything that isn't a plain package spec (a leading '-'
        // reads as a flag; shell metacharacters have no business in a package name).
        // Args are passed without a shell, but this is cheap defense in depth against
        // a malicious or MITM'd registry entry.
        if !is_safe_package_id(&spec) {
            return None;
        }
        let (command, args) = match registry_type {
            "pypi" => ("uvx".to_string(), vec![spec]),
            "oci" | "docker" => (
                "docker".to_string(),
                vec![
                    "run".to_string(),
                    "-i".to_string(),
                    "--rm".to_string(),
                    spec,
                ],
            ),
            _ => {
                // npm-only, and deliberately here rather than in is_safe_package_id:
                // `user/image:tag` is an ordinary Docker reference, so the slash rule
                // below would drop legitimate OCI entries if applied globally.
                if npm_spec_escapes_registry(&spec) {
                    return None;
                }
                ("npx".to_string(), vec!["-y".to_string(), spec])
            }
        };
        let env_keys = pkg
            .get("environmentVariables")
            .or_else(|| pkg.get("environment_variables"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        return Some(CatalogEntry {
            name,
            description,
            transport: "stdio".to_string(),
            command: Some(command),
            args,
            launch: None,
            url: None,
            env_keys,
            source: "registry".to_string(),
            homepage,
            publisher,
            category: String::new(),
            credentials_url: None,
            setup_hint: None,
            url_hint: None,
        });
    }

    None
}

/// True if a registry list item is the latest published version of its server
/// (the API returns one item per version, so we dedupe on this).
fn is_latest(item: &Value) -> bool {
    item.get("_meta")
        .and_then(|m| m.get("io.modelcontextprotocol.registry/official"))
        .and_then(|o| o.get("isLatest"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

fn is_active(item: &Value) -> bool {
    item.get("_meta")
        .and_then(|m| m.get("io.modelcontextprotocol.registry/official"))
        .and_then(|o| o.get("status"))
        .and_then(|v| v.as_str())
        .is_none_or(|status| status == "active")
}

fn registry_search_url(base_url: &str, query: &str) -> String {
    let q = query.trim();
    if q.is_empty() {
        format!("{base_url}?limit=50&version=latest")
    } else {
        format!(
            "{base_url}?limit=50&version=latest&search={}",
            urlencoding::encode(q)
        )
    }
}

/// Classify a transport-level ureq failure: the body-read half of a budget
/// surfaces as `ErrorKind::Io` with a "timed out" message (ureq normalizes
/// `WouldBlock`/`TimedOut` this way, see `stream.rs`), while DNS and connect
/// failures - including a connect-phase timeout - come back as
/// `ConnectionFailed`. Anything else (bad URL, bad status line, proxy) is
/// `"other"`: real, but not one the UI needs a dedicated message for.
fn classify_ureq_error(error: &ureq::Error) -> RegistryError {
    let message = error.to_string();
    let kind = match error {
        ureq::Error::Status(_, _) => "other",
        ureq::Error::Transport(transport) => match transport.kind() {
            ureq::ErrorKind::ConnectionFailed
            | ureq::ErrorKind::Dns
            | ureq::ErrorKind::ProxyConnect
            | ureq::ErrorKind::ProxyUnauthorized
            | ureq::ErrorKind::InvalidProxyUrl => "connectionFailed",
            ureq::ErrorKind::Io if message.to_ascii_lowercase().contains("timed out") => {
                "timeout"
            }
            _ => "other",
        },
    };
    RegistryError::new(kind, message)
}

/// Classify a plain `io::Error` from reading the response body (the read
/// timeout also applies here, past the point a [`ureq::Error`] would be seen).
fn classify_io_error(error: &std::io::Error) -> RegistryError {
    let kind = if error.kind() == std::io::ErrorKind::TimedOut {
        "timeout"
    } else {
        "connectionFailed"
    };
    RegistryError::new(kind, error.to_string())
}

fn read_registry_body(agent: &ureq::Agent, url: &str) -> Result<Value, RegistryError> {
    use std::io::Read;
    let resp = agent.get(url).call().map_err(|e| classify_ureq_error(&e))?;
    // Cap the registry response (defense in depth against a huge or MITM'd body).
    let mut buf = Vec::new();
    resp.into_reader()
        .take(8 * 1024 * 1024)
        .read_to_end(&mut buf)
        .map_err(|e| classify_io_error(&e))?;
    serde_json::from_slice(&buf).map_err(|e| RegistryError::new("other", e.to_string()))
}

fn registry_items(body: &Value) -> &[Value] {
    body.get("servers")
        .and_then(|v| v.as_array())
        .map_or(&[], Vec::as_slice)
}

fn fetch_registry(agent: &ureq::Agent, url: &str) -> Result<Vec<CatalogEntry>, RegistryError> {
    let body = read_registry_body(agent, url)?;
    Ok(registry_items(&body)
        .iter()
        .filter(|item| is_latest(item) && is_active(item))
        .filter_map(|item| map_server(item.get("server").unwrap_or(item)))
        .collect())
}

pub(crate) struct RegistryPage {
    pub entries: Vec<CatalogEntry>,
    pub retired: Vec<String>,
    pub next_cursor: Option<String>,
}

pub(crate) fn fetch_registry_page(
    agent: &ureq::Agent,
    url: &str,
) -> Result<RegistryPage, RegistryError> {
    let body = read_registry_body(agent, url)?;
    let latest = registry_items(&body).iter().filter(|item| is_latest(item));
    let (active, inactive): (Vec<&Value>, Vec<&Value>) = latest.partition(|item| is_active(item));
    let next_cursor = body
        .get("metadata")
        .and_then(|m| m.get("nextCursor").or_else(|| m.get("next_cursor")))
        .and_then(|v| v.as_str())
        .filter(|cursor| !cursor.is_empty())
        .map(String::from);
    Ok(RegistryPage {
        entries: active
            .into_iter()
            .filter_map(|item| map_server(item.get("server").unwrap_or(item)))
            .collect(),
        retired: inactive
            .into_iter()
            .map(|item| registry_display_name(item.get("server").unwrap_or(item)))
            .filter(|name| !name.is_empty())
            .collect(),
        next_cursor,
    })
}

pub(crate) fn registry_page_url(
    base_url: &str,
    cursor: Option<&str>,
    updated_since: Option<&str>,
) -> String {
    let mut url = format!("{base_url}?limit=100&version=latest");
    if let Some(cursor) = cursor {
        url.push_str(&format!("&cursor={}", urlencoding::encode(cursor)));
    }
    if let Some(since) = updated_since {
        url.push_str(&format!("&updated_since={}", urlencoding::encode(since)));
    }
    url
}

/// Search the official MCP Registry. Empty query lists popular/recent servers.
/// Connect and read get separate budgets (D-101): the registry's own search is
/// slow, not down, on an uncached query, so this must outlast that instead of
/// giving up on a connection that is still working.
pub fn search_registry(query: &str) -> Result<Vec<CatalogEntry>, RegistryError> {
    search_registry_from(REGISTRY_URL, query, REGISTRY_CONNECT_TIMEOUT, REGISTRY_READ_TIMEOUT)
}

fn search_registry_from(
    base_url: &str,
    query: &str,
    connect_timeout: Duration,
    read_timeout: Duration,
) -> Result<Vec<CatalogEntry>, RegistryError> {
    let url = registry_search_url(base_url, query);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(connect_timeout)
        .timeout_read(read_timeout)
        .build();
    fetch_registry(&agent, &url)
}

type CacheKey = (String, String);

/// Keyed by (base url, normalized query) so tests pointed at a local mock
/// server never share an entry with the live registry or with each other.
fn cache_key(base_url: &str, query: &str) -> CacheKey {
    (base_url.to_string(), query.trim().to_lowercase())
}

/// Only the live registry is cached on disk: a mock endpoint's answers must not
/// land in the user's data dir.
fn persists_to_disk(base_url: &str) -> bool {
    base_url == REGISTRY_URL
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

/// A timestamp in the future (clock set back) counts as infinitely old, so it
/// is refetched and pruned instead of staying fresh until the clock catches up.
fn age_since(fetched_at_ms: u64, now_ms: u64) -> Duration {
    now_ms
        .checked_sub(fetched_at_ms)
        .map_or(Duration::MAX, Duration::from_millis)
}

struct CachedRegistrySearch {
    entries: Vec<CatalogEntry>,
    fetched_at_ms: u64,
    last_used_ms: u64,
    ticket: u64,
}

#[derive(Default)]
struct RegistryCache {
    by_query: HashMap<CacheKey, CachedRegistrySearch>,
    loaded_from: Option<PathBuf>,
    revision: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryCacheFile {
    version: u32,
    queries: Vec<RegistryCacheRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryCacheRecord {
    base_url: String,
    query: String,
    fetched_at_ms: u64,
    #[serde(default)]
    last_used_ms: u64,
    entries: Vec<CatalogEntry>,
}

impl RegistryCacheRecord {
    fn used_at_ms(&self) -> u64 {
        self.last_used_ms.max(self.fetched_at_ms)
    }
}

/// The most recently used [`REGISTRY_CACHE_MAX_QUERIES`] usable records in the
/// file. A missing, unreadable, corrupt or foreign-version file is just an
/// empty cache.
fn read_cache_file(path: &Path, now_ms: u64) -> Vec<RegistryCacheRecord> {
    let Some(file) = std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<RegistryCacheFile>(&raw).ok())
        .filter(|file| file.version == REGISTRY_CACHE_FILE_VERSION)
    else {
        return Vec::new();
    };
    let mut records: Vec<_> = file
        .queries
        .into_iter()
        .filter(|r| age_since(r.fetched_at_ms, now_ms) <= REGISTRY_CACHE_MAX_AGE)
        .collect();
    records.sort_by_key(|r| std::cmp::Reverse(r.used_at_ms()));
    records.truncate(REGISTRY_CACHE_MAX_QUERIES);
    records
}

/// Best-effort: an unwritable cache file leaves the in-memory cache working.
fn write_cache_records(path: &Path, queries: Vec<RegistryCacheRecord>) {
    let file = RegistryCacheFile {
        version: REGISTRY_CACHE_FILE_VERSION,
        queries,
    };
    if let Ok(json) = serde_json::to_string(&file) {
        let _ = crate::registry::atomic_write(path, &json);
    }
}

impl RegistryCache {
    fn get(&self, key: &CacheKey, now_ms: u64) -> Option<(Vec<CatalogEntry>, Duration)> {
        self.by_query
            .get(key)
            .map(|cached| (cached.entries.clone(), age_since(cached.fetched_at_ms, now_ms)))
    }

    fn touch(&mut self, key: &CacheKey, now_ms: u64) {
        if let Some(cached) = self.by_query.get_mut(key) {
            cached.last_used_ms = cached.last_used_ms.max(now_ms);
        }
    }

    fn put(&mut self, key: CacheKey, cached: CachedRegistrySearch) {
        self.by_query.insert(key, cached);
        self.revision += 1;
        while self.by_query.len() > REGISTRY_CACHE_MAX_QUERIES {
            let least_recent = self
                .by_query
                .iter()
                .min_by_key(|(_, cached)| cached.last_used_ms)
                .map(|(key, _)| key.clone());
            match least_recent {
                Some(key) => self.by_query.remove(&key),
                None => break,
            };
        }
    }

    /// A fetch that started before the one whose answer is already stored must
    /// not replace it: the later query is the one the user is waiting on.
    fn insert_unless_superseded(
        &mut self,
        key: CacheKey,
        entries: Vec<CatalogEntry>,
        fetched_at_ms: u64,
        ticket: u64,
    ) -> bool {
        if self.by_query.get(&key).is_some_and(|have| have.ticket > ticket) {
            return false;
        }
        self.put(
            key,
            CachedRegistrySearch {
                entries,
                fetched_at_ms,
                last_used_ms: fetched_at_ms,
                ticket,
            },
        );
        true
    }

    #[cfg(test)]
    fn insert(&mut self, key: CacheKey, entries: Vec<CatalogEntry>, fetched_at_ms: u64) {
        self.insert_unless_superseded(key, entries, fetched_at_ms, 0);
    }

    /// Merge records read from `path` in; whatever is already in memory wins
    /// when it is at least as new.
    fn merge_loaded(&mut self, path: &Path, records: Vec<RegistryCacheRecord>) {
        for record in records {
            let used_at_ms = record.used_at_ms();
            let key = (record.base_url, record.query);
            let newer = self
                .by_query
                .get(&key)
                .is_none_or(|have| have.fetched_at_ms < record.fetched_at_ms);
            if newer {
                self.put(
                    key,
                    CachedRegistrySearch {
                        entries: record.entries,
                        fetched_at_ms: record.fetched_at_ms,
                        last_used_ms: used_at_ms,
                        ticket: 0,
                    },
                );
            }
        }
        self.loaded_from = Some(path.to_path_buf());
    }

    /// The records worth writing and the revision they were taken at.
    fn snapshot(&self, now_ms: u64) -> (u64, Vec<RegistryCacheRecord>) {
        let mut queries: Vec<RegistryCacheRecord> = self
            .by_query
            .iter()
            .filter(|(_, cached)| {
                age_since(cached.fetched_at_ms, now_ms) <= REGISTRY_CACHE_MAX_AGE
            })
            .map(|((base_url, query), cached)| RegistryCacheRecord {
                base_url: base_url.clone(),
                query: query.clone(),
                fetched_at_ms: cached.fetched_at_ms,
                last_used_ms: cached.last_used_ms,
                entries: cached.entries.clone(),
            })
            .collect();
        queries.sort_by(|a, b| {
            b.used_at_ms()
                .cmp(&a.used_at_ms())
                .then_with(|| a.query.cmp(&b.query))
        });
        queries.truncate(REGISTRY_CACHE_MAX_QUERIES);
        (self.revision, queries)
    }
}

static REGISTRY_CACHE: OnceLock<Mutex<RegistryCache>> = OnceLock::new();
static CACHE_REVISION_WRITTEN: Mutex<u64> = Mutex::new(0);
static FETCH_TICKETS: AtomicU64 = AtomicU64::new(0);

fn registry_cache() -> &'static Mutex<RegistryCache> {
    REGISTRY_CACHE.get_or_init(|| Mutex::new(RegistryCache::default()))
}

fn registry_cache_path() -> Option<PathBuf> {
    crate::registry::conduit_dir().map(|dir| dir.join(REGISTRY_CACHE_FILE))
}

/// Merge the disk cache in once per file path. The file is read with the cache
/// unlocked so a lookup never waits on disk.
fn ensure_cache_loaded(path: &Path, now_ms: u64) {
    let loaded = registry_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .loaded_from
        .as_deref()
        == Some(path);
    if loaded {
        return;
    }
    let records = read_cache_file(path, now_ms);
    registry_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .merge_loaded(path, records);
}

/// Writes a snapshot unless a newer one already reached the disk: two stores
/// can finish their writes in the opposite order to their snapshots.
fn write_cache_snapshot(
    last_written: &Mutex<u64>,
    path: &Path,
    revision: u64,
    records: Vec<RegistryCacheRecord>,
) {
    let mut written = last_written.lock().unwrap_or_else(|e| e.into_inner());
    if revision <= *written {
        return;
    }
    write_cache_records(path, records);
    *written = revision;
}

/// The cached answer for `key` and its age, however stale; a hit counts as a
/// use. With `persist`, the disk cache is merged in first (once per process).
fn cache_lookup(key: &CacheKey, persist: bool) -> Option<(Vec<CatalogEntry>, Duration)> {
    let now_ms = unix_ms_now();
    if let Some(path) = persist.then(registry_cache_path).flatten() {
        ensure_cache_loaded(&path, now_ms);
    }
    let mut cache = registry_cache().lock().unwrap_or_else(|e| e.into_inner());
    cache.touch(key, now_ms);
    cache.get(key, now_ms)
}

/// `ticket` is the [`FETCH_TICKETS`] value taken before the network call.
fn cache_store(key: CacheKey, entries: Vec<CatalogEntry>, persist: bool, ticket: u64) {
    let now_ms = unix_ms_now();
    let path = persist.then(registry_cache_path).flatten();
    if let Some(path) = &path {
        // Load before writing so the file is never replaced by this process's
        // new answers alone.
        ensure_cache_loaded(path, now_ms);
    }
    let snapshot = {
        let mut cache = registry_cache().lock().unwrap_or_else(|e| e.into_inner());
        if !cache.insert_unless_superseded(key, entries, now_ms, ticket) {
            return;
        }
        path.is_some().then(|| cache.snapshot(now_ms))
    };
    if let (Some(path), Some((revision, records))) = (path, snapshot) {
        write_cache_snapshot(&CACHE_REVISION_WRITTEN, &path, revision, records);
    }
}

/// [`search_registry_from`] in front of a per-query cache: a call within
/// `cache_ttl` of the last success is served from the cache with no network
/// call, and a failed call falls back to the last good answer for the same
/// query (however stale) rather than losing results the user already saw. The
/// real error is always logged, and always returned alongside whatever entries
/// are available, so a caller can decide what to show. With `persist`, answers
/// also survive a restart (see [`REGISTRY_CACHE_FILE`]).
fn registry_search_cached_from(
    base_url: &str,
    query: &str,
    connect_timeout: Duration,
    read_timeout: Duration,
    cache_ttl: Duration,
    persist: bool,
) -> (Vec<CatalogEntry>, Option<RegistryError>) {
    let key = cache_key(base_url, query);
    if let Some((entries, age)) = cache_lookup(&key, persist) {
        if age < cache_ttl {
            return (entries, None);
        }
    }
    let ticket = FETCH_TICKETS.fetch_add(1, Ordering::Relaxed) + 1;
    match search_registry_from(base_url, query, connect_timeout, read_timeout) {
        Ok(entries) => {
            cache_store(key, entries.clone(), persist, ticket);
            (entries, None)
        }
        Err(error) => {
            crate::gatewaylog::append(&format!(
                "toolport: registry search failed ({}): {}",
                error.kind, error.message
            ));
            let stale = cache_lookup(&key, persist).map(|(entries, _)| entries);
            (stale.unwrap_or_default(), Some(error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plus::testutil::DataDirFx;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Read a request off a raw mock-server socket up to the end of its headers
    /// (there is no body on a registry GET), so the response write that follows
    /// isn't racing the client's own read.
    fn read_request_headers(stream: &mut std::net::TcpStream) {
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        while stream.read(&mut byte).unwrap_or(0) > 0 {
            buf.push(byte[0]);
            if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
                break;
            }
        }
    }

    fn write_json_response(stream: &mut std::net::TcpStream, body: &str) {
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.flush();
    }

    /// One registry entry, matching the shape [`map_server`] expects, with a
    /// dummy remote so it maps to a valid `CatalogEntry`.
    fn mock_registry_body(name: &str) -> String {
        json!({
            "servers": [{
                "name": format!("io.example/{name}"),
                "title": name,
                "remotes": [{"url": "https://example.com/mcp", "type": "sse"}],
            }]
        })
        .to_string()
    }

    #[test]
    fn curated_remote_setup_matches_supported_publisher_auth() {
        let entries = curated();
        // Asana v2 requires a preregistered authorization-code client, which
        // Toolport's CIMD/DCR flow cannot yet supply. Keep it out of curated add.
        assert!(!entries.iter().any(|e| e.name == "Asana"));
        let langfuse = entries.iter().find(|e| e.name == "Langfuse").unwrap();
        assert_eq!(
            langfuse.url_hint.as_deref(),
            Some("https://your-langfuse.com/api/public/mcp")
        );
        assert!(langfuse.setup_hint.as_deref().unwrap().contains("Basic"));
        let postiz = entries.iter().find(|e| e.name == "Postiz").unwrap();
        assert_eq!(postiz.url.as_deref(), Some("https://mcp.postiz.com/mcp"));
        assert!(postiz.setup_hint.as_deref().unwrap().contains("token"));
    }

    #[test]
    fn registry_search_requests_only_current_versions() {
        assert_eq!(
            registry_search_url(REGISTRY_URL, ""),
            "https://registry.modelcontextprotocol.io/v0.1/servers?limit=50&version=latest"
        );
        assert_eq!(
            registry_search_url(REGISTRY_URL, " git hub "),
            "https://registry.modelcontextprotocol.io/v0.1/servers?limit=50&version=latest&search=git%20hub"
        );
    }

    #[test]
    fn curated_is_nonempty_and_well_formed() {
        let c = curated();
        assert!(c.len() >= 20);
        for e in &c {
            assert!(!e.name.is_empty());
            // Each entry has a target: a URL, a command, or a url_hint
            // (self-hosted servers where the user supplies the URL at add time).
            assert!(
                e.url.is_some() || e.command.is_some() || e.url_hint.is_some(),
                "{} has no target",
                e.name
            );
            // Every curated entry must land in a browse-view category.
            assert!(!e.category.is_empty(), "{} has no category", e.name);
        }
    }

    #[test]
    fn curated_search_finds_popular_picks() {
        // The reported bug: searching a curated vendor must still surface it,
        // even though the live registry wouldn't return it.
        let vercel = filter_catalog(curated(), "vercel");
        // The official Vercel and the Toolport (Full API) overlay both surface, official first.
        assert_eq!(vercel.len(), 2);
        assert_eq!(vercel[0].name, "Vercel");
        assert_eq!(vercel[1].name, "Vercel (Full API)");
        // Description matches too (Postgres -> Neon/Supabase).
        assert!(filter_catalog(curated(), "postgres")
            .iter()
            .any(|e| e.name == "Neon"));
        // Empty query returns the full set.
        assert_eq!(filter_catalog(curated(), "").len(), curated().len());
    }

    #[test]
    fn search_ranking_prefers_curated_and_exact_name_matches() {
        let stripe = curated()
            .into_iter()
            .find(|entry| entry.name == "Stripe")
            .unwrap();
        let mut registry_prefix = stripe.clone();
        registry_prefix.name = "Stripe helper".into();
        registry_prefix.source = "registry".into();
        let mut results = vec![registry_prefix, stripe];
        rank_search_results(&mut results, "stripe");
        assert_eq!(results[0].name, "Stripe");
    }

    #[test]
    fn registry_lifecycle_filter_excludes_inactive_entries() {
        let item = |status| {
            json!({
                "_meta": {
                    "io.modelcontextprotocol.registry/official": {
                        "status": status,
                        "isLatest": true
                    }
                }
            })
        };
        assert!(is_active(&item("active")));
        assert!(!is_active(&item("deprecated")));
        assert!(!is_active(&item("deleted")));
    }

    #[test]
    fn trello_is_a_hosted_oauth_catalog_entry() {
        let entries = curated();
        let trello = entries.iter().find(|entry| entry.name == "Trello").unwrap();
        assert_eq!(trello.transport, "http");
        assert_eq!(trello.url.as_deref(), Some("https://mcp.trello.com/v1"));
        assert!(trello.command.is_none());
        assert!(trello.env_keys.is_empty());
        assert_eq!(trello.category, "Apps & productivity");
        assert!(trello.setup_hint.as_deref().unwrap().contains("OAuth"));
        assert!(trello.credentials_url.is_none());
        assert_eq!(filter_catalog(entries, "trello").len(), 1);
    }

    #[test]
    fn atlassian_uses_the_current_v2_gateway_endpoint() {
        let atlassian = curated()
            .into_iter()
            .find(|entry| entry.name == "Atlassian")
            .expect("Atlassian must remain in the curated catalog");
        assert_eq!(
            atlassian.url.as_deref(),
            Some("https://mcp.atlassian.com/v2/mcp?tools=all")
        );
    }

    #[test]
    fn filter_catalog_matches_category_headings() {
        let c = curated();
        let databases = filter_catalog(c.clone(), "databases");
        for name in [
            "Supabase",
            "Neon",
            "Elasticsearch",
            "Qdrant",
            "MongoDB",
            "PostgreSQL",
            "Redis",
        ] {
            assert!(
                databases.iter().any(|e| e.name == name),
                "searching 'databases' should include {name}"
            );
        }

        let productivity = filter_catalog(c.clone(), "productivity");
        assert!(
            productivity.iter().any(|e| e.name == "Notion"),
            "searching 'productivity' should include Apps & productivity members"
        );
        assert!(
            productivity.iter().any(|e| e.name == "Linear"),
            "searching 'productivity' should include Apps & productivity members"
        );
        assert!(productivity.iter().any(|e| e.name == "Postman"));

        let infrastructure = filter_catalog(c, "infrastructure");
        assert!(
            infrastructure.iter().any(|e| e.name == "GitHub"),
            "searching 'infrastructure' should include Code & infrastructure members"
        );
        assert!(
            infrastructure.iter().any(|e| e.name == "AWS"),
            "searching 'infrastructure' should include Code & infrastructure members"
        );
    }

    #[test]
    fn filter_catalog_empty_category_does_not_match_everything() {
        let entry = CatalogEntry {
            name: "Registry Only".into(),
            description: "A live registry result with no browse category.".into(),
            transport: "http".into(),
            command: None,
            args: vec![],
            launch: None,
            url: Some("https://example.com/mcp".into()),
            env_keys: vec![],
            source: "registry".into(),
            homepage: None,
            publisher: None,
            category: String::new(),
            credentials_url: None,
            setup_hint: None,
            url_hint: None,
        };
        assert!(filter_catalog(vec![entry], "databases").is_empty());
    }

    #[test]
    fn filter_catalog_multiword_uses_all_terms_fallback() {
        let c = curated();
        // "serverless postgres" is not a contiguous substring of Neon's text
        // ("Serverless Postgres: ..."), but both terms appear, so the all-terms
        // fallback must still surface it.
        let hits = filter_catalog(c.clone(), "serverless postgres");
        assert!(
            hits.iter().any(|e| e.name == "Neon"),
            "all-terms fallback should match Neon"
        );
        // A term present nowhere yields nothing.
        assert!(filter_catalog(c, "zzzznotacatalogword").is_empty());
    }

    #[test]
    fn maps_a_remote_server() {
        // Shape taken from a real registry.modelcontextprotocol.io response.
        let server = json!({
            "name": "ac.inference.sh/mcp",
            "title": "inference.sh",
            "description": "Run 150+ AI apps.",
            "remotes": [{ "type": "streamable-http", "url": "https://api.inference.sh/mcp" }]
        });
        let e = map_server(&server).unwrap();
        assert_eq!(e.name, "inference.sh");
        assert_eq!(e.transport, "http");
        assert_eq!(e.url.as_deref(), Some("https://api.inference.sh/mcp"));
        assert_eq!(e.source, "registry");
    }

    #[test]
    fn maps_an_npm_package_with_env() {
        let server = json!({
            "name": "io.github.acme/widget",
            "description": "A widget server.",
            "packages": [{
                "registryType": "npm",
                "identifier": "@acme/widget-mcp",
                "version": "1.2.3",
                "environmentVariables": [{ "name": "ACME_API_KEY" }]
            }]
        });
        let e = map_server(&server).unwrap();
        // No title -> falls back to the namespaced id.
        assert_eq!(e.name, "io.github.acme/widget");
        assert_eq!(e.transport, "stdio");
        assert_eq!(e.command.as_deref(), Some("npx"));
        assert_eq!(e.args, vec!["-y", "@acme/widget-mcp@1.2.3"]);
        assert_eq!(e.env_keys, vec!["ACME_API_KEY"]);
    }

    #[test]
    fn maps_a_pypi_package_to_uvx() {
        let server = json!({
            "name": "io.github.acme/pytool",
            "description": "A python server.",
            "packages": [{
                "registryType": "pypi",
                "identifier": "acme-mcp",
                "version": "0.4.2"
            }]
        });
        let e = map_server(&server).unwrap();
        assert_eq!(e.transport, "stdio");
        assert_eq!(e.command.as_deref(), Some("uvx"));
        // uvx takes the spec alone — no `-y`-style prefix.
        assert_eq!(e.args, vec!["acme-mcp@0.4.2"]);

        // `latest` is a dist-tag, not a version, so the bare identifier is used.
        let latest = json!({ "name": "io.github.acme/pytool", "title": "Pytool",
            "packages": [{ "registryType": "pypi", "identifier": "acme-mcp", "version": "latest" }] });
        let e = map_server(&latest).unwrap();
        assert_eq!(e.command.as_deref(), Some("uvx"));
        assert_eq!(e.args, vec!["acme-mcp"]);

        // Same for a missing version.
        let bare = json!({ "name": "io.github.acme/pytool", "title": "Pytool",
            "packages": [{ "registryType": "pypi", "identifier": "acme-mcp" }] });
        assert_eq!(map_server(&bare).unwrap().args, vec!["acme-mcp"]);
    }

    #[test]
    fn maps_container_packages_to_a_docker_run() {
        // "oci" and "docker" are the two spellings the registry uses for the same thing.
        for registry_type in ["oci", "docker"] {
            let server = json!({
                "name": "io.github.acme/boxed",
                "description": "A containerized server.",
                "packages": [{
                    "registryType": registry_type,
                    "identifier": "ghcr.io/acme/boxed-mcp",
                    "version": "1.2.3",
                    "environmentVariables": [{ "name": "ACME_API_KEY" }]
                }]
            });
            let e = map_server(&server).unwrap();
            assert_eq!(e.transport, "stdio");
            assert_eq!(e.command.as_deref(), Some("docker"), "{registry_type}");
            // stdio needs the container's stdin held open and the container reaped,
            // so `-i --rm` must precede the image spec.
            assert_eq!(
                e.args,
                vec!["run", "-i", "--rm", "ghcr.io/acme/boxed-mcp:1.2.3"],
                "{registry_type}"
            );
            assert_eq!(e.env_keys, vec!["ACME_API_KEY"], "{registry_type}");
        }

        // No version: the image spec is the bare identifier.
        let bare = json!({ "name": "io.github.acme/boxed", "title": "Boxed",
            "packages": [{ "registryType": "oci", "identifier": "ghcr.io/acme/boxed-mcp" }] });
        assert_eq!(
            map_server(&bare).unwrap().args,
            vec!["run", "-i", "--rm", "ghcr.io/acme/boxed-mcp"]
        );

        // OCI digests use @; a colon is only the separator for a tag.
        let digest = json!({ "name": "io.github.acme/boxed", "title": "Boxed",
            "packages": [{
                "registryType": "oci",
                "identifier": "ghcr.io/acme/boxed-mcp",
                "version": "sha256:abcdef0123456789"
            }] });
        assert_eq!(
            map_server(&digest).unwrap().args,
            vec![
                "run",
                "-i",
                "--rm",
                "ghcr.io/acme/boxed-mcp@sha256:abcdef0123456789"
            ]
        );
    }

    #[test]
    fn map_server_rejects_unsafe_specs_for_every_registry_type() {
        // The safety gate runs before the registry_type match, so uvx and docker get
        // the same treatment npx does.
        for registry_type in ["pypi", "oci", "docker"] {
            // Leading-dash identifier (flag injection into uvx/docker) is dropped.
            let flag = json!({ "name": "io.x/y", "title": "Y",
                "packages": [{ "registryType": registry_type, "identifier": "--unsafe-flag" }] });
            assert!(
                map_server(&flag).is_none(),
                "{registry_type}: flag-injection identifier must be dropped"
            );
            // Shell metacharacters in the version are dropped.
            let meta = json!({ "name": "io.x/z", "title": "Z",
                "packages": [{ "registryType": registry_type, "identifier": "pkg", "version": "1; rm -rf /" }] });
            assert!(
                map_server(&meta).is_none(),
                "{registry_type}: shell metachars must be dropped"
            );
            // An empty identifier has nothing to install.
            let empty = json!({ "name": "io.x/w", "title": "W",
                "packages": [{ "registryType": registry_type, "identifier": "" }] });
            assert!(
                map_server(&empty).is_none(),
                "{registry_type}: empty identifier must be dropped"
            );
        }
    }

    /// npm accepts several spellings of "fetch this from a git host" that carry no
    /// `://` and no leading alias, so a prefix-only check let a malicious or MITM'd
    /// registry entry one-click-install code from outside the named registry.
    #[test]
    fn map_server_rejects_npm_specs_that_escape_the_registry() {
        let npm = |identifier: &str, version: Option<&str>| {
            let mut pkg = json!({ "registryType": "npm", "identifier": identifier });
            if let Some(v) = version {
                pkg["version"] = json!(v);
            }
            json!({ "name": "io.x/y", "title": "Y", "packages": [pkg] })
        };
        for (identifier, version) in [
            // Unscoped slash name: GitHub shorthand to npm.
            ("attacker/payload", None),
            // Hosted-git alias smuggled in as the version range.
            ("lodash", Some("github:attacker/payload")),
            ("lodash", Some("gitlab:attacker/payload")),
            // Hosted URL with the scheme left off.
            ("github.com/attacker/payload", None),
            // SCP-style git, which has no `://` to catch.
            ("git@github.com:attacker/payload.git", None),
        ] {
            assert!(
                map_server(&npm(identifier, version)).is_none(),
                "npm spec {identifier}@{version:?} must not reach npx -y"
            );
        }

        // Real npm packages still install, including scoped names whose own `@` must
        // not be read as the version separator.
        for (identifier, version) in [
            ("@scope/name", None),
            ("@scope/name", Some("1.2.3")),
            ("express", Some("4.18.0")),
        ] {
            let entry = map_server(&npm(identifier, version));
            assert!(
                entry.is_some(),
                "npm spec {identifier}@{version:?} is legitimate and must still map"
            );
        }

        // The slash rule is npm-only: a Docker reference is not GitHub shorthand.
        let docker = json!({ "name": "io.x/d", "title": "D", "packages": [
            { "registryType": "oci", "identifier": "user/image", "version": "tag" }] });
        assert!(
            map_server(&docker).is_some(),
            "a docker user/image:tag reference must still map"
        );
    }

    #[test]
    fn skips_isnt_latest() {
        let old = json!({ "_meta": { "io.modelcontextprotocol.registry/official": { "isLatest": false } } });
        let cur = json!({ "_meta": { "io.modelcontextprotocol.registry/official": { "isLatest": true } } });
        assert!(!is_latest(&old));
        assert!(is_latest(&cur));
    }

    #[test]
    fn map_server_rejects_unsafe_specs() {
        // Leading-dash identifier (flag injection into npx) is dropped.
        let flag = json!({ "name": "io.x/y", "title": "Y",
            "packages": [{ "registryType": "npm", "identifier": "--unsafe-flag" }] });
        assert!(
            map_server(&flag).is_none(),
            "flag-injection identifier must be dropped"
        );
        // Shell metacharacters in the version are dropped.
        let meta = json!({ "name": "io.x/z", "title": "Z",
            "packages": [{ "registryType": "npm", "identifier": "pkg", "version": "1; rm -rf /" }] });
        assert!(
            map_server(&meta).is_none(),
            "shell metachars must be dropped"
        );
        // A non-http(s) remote URL is dropped.
        let scheme = json!({ "name": "io.x/w", "title": "W",
            "remotes": [{ "type": "streamable-http", "url": "file:///etc/passwd" }] });
        assert!(
            map_server(&scheme).is_none(),
            "non-http remote must be dropped"
        );
        // github:/URL/git+ specs would make npx fetch outside npm. Colon stays
        // allowed for docker host:port and OCI tags (see maps_container_packages).
        for identifier in [
            "github:attacker/payload",
            "https://evil.example/pkg.tgz",
            "http://evil.example/pkg.tgz",
            "git+https://github.com/attacker/payload.git",
            "npm:evil",
        ] {
            let remote = json!({ "name": "io.x/r", "title": "R",
                "packages": [{ "registryType": "npm", "identifier": identifier }] });
            assert!(
                map_server(&remote).is_none(),
                "{identifier}: remote package spec must be dropped"
            );
        }
    }

    // Self-hosted catalog coverage (url_hint / setup_hint invariants), contributed by
    // @bradhallett (salvaged from PR #62 onto current main).
    #[test]
    fn self_hosted_entries_have_url_hint_not_url() {
        let c = curated();
        for e in &c {
            if e.url_hint.is_some() {
                // Self-hosted entries must NOT have a fixed URL (user supplies it).
                assert!(e.url.is_none(), "{} has both url and url_hint", e.name);
                // And must have transport http (they speak MCP over HTTP).
                assert_eq!(
                    e.transport, "http",
                    "{} has url_hint but transport is not http",
                    e.name
                );
            }
        }
    }

    #[test]
    fn url_hint_round_trips_through_serialization() {
        let e = curated()
            .into_iter()
            .find(|e| e.url_hint.is_some())
            .unwrap();
        let original_hint = e.url_hint.clone().unwrap();
        let json = serde_json::to_string(&e).unwrap();
        // url_hint is serialized (not skipped — it's present).
        assert!(json.contains("urlHint"), "url_hint should appear in JSON");
        let back: CatalogEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.url_hint.as_deref(), Some(original_hint.as_str()));
    }

    #[test]
    fn n8n_and_langfuse_are_in_catalog() {
        let c = curated();
        assert!(
            c.iter().any(|e| e.name == "n8n"),
            "n8n missing from catalog"
        );
        assert!(
            c.iter().any(|e| e.name == "Langfuse"),
            "Langfuse missing from catalog"
        );
    }

    #[test]
    fn n8n_and_langfuse_have_categories() {
        let c = curated();
        for e in c.iter().filter(|e| e.name == "n8n" || e.name == "Langfuse") {
            assert!(!e.category.is_empty(), "{} has no category", e.name);
        }
    }

    #[test]
    fn self_hosted_entries_have_credential_hints() {
        // n8n and Langfuse should guide the user on their URL + credentials. On current
        // main that guidance lives in url_hint (adapted from Brad's setup_hint check).
        let c = curated();
        for name in ["n8n", "Langfuse"] {
            let e = c.iter().find(|e| e.name == name).unwrap();
            let hint = e.url_hint.as_deref().or(e.setup_hint.as_deref());
            assert!(
                hint.map(|h| !h.is_empty()).unwrap_or(false),
                "{name} should have a credential/setup hint"
            );
        }
    }

    #[test]
    fn registry_entries_have_no_url_hint() {
        // map_server (the MCP registry mapper) should never set url_hint.
        let server = json!({
            "name": "io.test/example",
            "title": "Example",
            "packages": [{ "registryType": "npm", "identifier": "example-mcp" }]
        });
        let entry = map_server(&server).unwrap();
        assert!(
            entry.url_hint.is_none(),
            "registry entries should not have url_hint"
        );
    }

    #[test]
    fn curated_launch_references_are_complete_and_templates_stable() {
        let entries = curated();
        let mut templates = std::collections::HashSet::new();
        for entry in &entries {
            let launch = entry.launch.as_ref().expect("curated template identity");
            assert!(templates.insert(launch.template.as_deref().unwrap()));
            launch.validate(&entry.args, true).unwrap();
            assert!(launch
                .required_env
                .iter()
                .all(|key| entry.env_keys.contains(key)));
        }
        let mut bad = entries.into_iter().find(|e| e.name == "Twilio").unwrap();
        bad.launch.as_mut().unwrap().bindings[0]
            .parts
            .push(ArgPart::Input {
                key: "MISSING".into(),
            });
        assert!(bad.launch.unwrap().validate(&bad.args, true).is_err());
        let mut unused = curated().into_iter().find(|e| e.name == "Twilio").unwrap();
        unused.launch.as_mut().unwrap().inputs.push(LaunchInput {
            key: "UNUSED".into(),
            label: "Unused".into(),
            secret: false,
            required: true,
            value: None,
        });
        assert!(unused.launch.unwrap().validate(&unused.args, true).is_err());
        let mut missing_binding = curated().into_iter().find(|e| e.name == "Twilio").unwrap();
        missing_binding.launch.as_mut().unwrap().bindings.clear();
        assert!(missing_binding
            .launch
            .unwrap()
            .validate(&missing_binding.args, true)
            .is_err());
    }

    #[test]
    fn redis_and_postman_have_documented_launch_shapes() {
        let entries = curated();
        let redis = entries.iter().find(|e| e.name == "Redis").unwrap();
        assert_eq!(redis.category, "Databases");
        assert_eq!(redis.command.as_deref(), Some("uvx"));
        assert_eq!(
            redis.args,
            [
                "--from",
                "redis-mcp-server@latest",
                "redis-mcp-server",
                "--url",
                "<launch-input>",
            ]
        );
        let launch = redis.launch.as_ref().unwrap();
        assert_eq!(launch.template.as_deref(), Some("redis"));
        assert_eq!(launch.revision, Some(1));
        assert_eq!(launch.inputs.len(), 1);
        assert_eq!(launch.inputs[0].key, "REDIS_URL");
        assert!(launch.inputs[0].secret && launch.inputs[0].required);
        assert_eq!(launch.bindings[0].index, 4);

        let postman = entries.iter().find(|e| e.name == "Postman").unwrap();
        assert_eq!(postman.transport, "http");
        assert_eq!(
            postman.url.as_deref(),
            Some("https://mcp.postman.com/minimal")
        );
        assert!(postman.command.is_none());
        assert_eq!(postman.category, "Apps & productivity");
        assert!(postman.setup_hint.as_deref().unwrap().contains("OAuth"));
    }

    #[test]
    fn registry_error_kinds_are_distinct_strings() {
        use std::io;
        let timeout = classify_io_error(&io::Error::new(io::ErrorKind::TimedOut, "slow"));
        assert_eq!(timeout.kind, "timeout");
        let reset = classify_io_error(&io::Error::new(io::ErrorKind::ConnectionReset, "reset"));
        assert_eq!(reset.kind, "connectionFailed");
        assert_ne!(timeout.kind, reset.kind);
    }

    // D-101: a registry that stalls past the read budget must not take the
    // curated matches already found in the same call down with it.
    #[test]
    fn search_registry_timeout_keeps_curated_hits_and_reports_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request_headers(&mut stream);
            // Never respond: the client's own read budget must fire unaided.
            std::thread::sleep(Duration::from_millis(400));
        });
        let base_url = format!("http://127.0.0.1:{port}/v0.1/servers");
        let result = search_with(
            &base_url,
            "slack",
            Duration::from_millis(300),
            Duration::from_millis(150),
            Duration::from_secs(600),
        );
        server.join().unwrap();
        assert!(
            result.entries.iter().any(|e| e.name == "Slack"),
            "a curated hit must survive a stalled registry call"
        );
        let error = result
            .registry_error
            .expect("a stalled registry call must report an error");
        assert_eq!(error.kind, "timeout");
    }

    #[test]
    fn search_registry_connection_failure_is_classified_distinctly() {
        // Bind then drop: the port is free again but nothing is listening, so a
        // connect attempt there is refused rather than stalling.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let base_url = format!("http://127.0.0.1:{port}/v0.1/servers");
        let result = search_with(
            &base_url,
            "zzz-mock-connection-refused",
            Duration::from_millis(300),
            Duration::from_millis(300),
            Duration::from_secs(600),
        );
        let error = result
            .registry_error
            .expect("a refused connection must report an error");
        assert_eq!(error.kind, "connectionFailed");
    }

    #[test]
    fn registry_cache_serves_a_repeat_query_without_a_new_network_call() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_server = Arc::clone(&hits);
        let server = std::thread::spawn(move || {
            // Exactly one connection is ever served: a second real network
            // attempt against this address is refused, which is how the test
            // proves the second call below never reached the network.
            let (mut stream, _) = listener.accept().unwrap();
            read_request_headers(&mut stream);
            hits_server.fetch_add(1, Ordering::SeqCst);
            write_json_response(&mut stream, &mock_registry_body("Widget"));
        });
        let base_url = format!("http://127.0.0.1:{port}/v0.1/servers");
        let ttl = Duration::from_secs(600);
        let budget = Duration::from_millis(300);
        let first = search_with(&base_url, "zzz-mock-cache", budget, budget, ttl);
        server.join().unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert!(first.registry_error.is_none());
        assert!(first.entries.iter().any(|e| e.name == "Widget"));

        let second = search_with(&base_url, "zzz-mock-cache", budget, budget, ttl);
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "a cache hit must not reach the network again"
        );
        assert!(second.registry_error.is_none());
        assert_eq!(second.entries, first.entries);
    }

    #[test]
    fn registry_cache_expires_and_refetches_after_ttl() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_server = Arc::clone(&hits);
        let server = std::thread::spawn(move || {
            for stream in listener.incoming().take(2) {
                let mut stream = stream.unwrap();
                read_request_headers(&mut stream);
                hits_server.fetch_add(1, Ordering::SeqCst);
                write_json_response(&mut stream, &mock_registry_body("Widget"));
            }
        });
        let base_url = format!("http://127.0.0.1:{port}/v0.1/servers");
        let budget = Duration::from_millis(300);
        let short_ttl = Duration::from_millis(150);
        let first = search_with(&base_url, "zzz-mock-ttl", budget, budget, short_ttl);
        assert!(first.registry_error.is_none());
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        // Generous margin over the TTL so ordinary scheduling jitter can't
        // make this flaky.
        std::thread::sleep(Duration::from_millis(500));
        let second = search_with(&base_url, "zzz-mock-ttl", budget, budget, short_ttl);
        server.join().unwrap();
        assert_eq!(
            hits.load(Ordering::SeqCst),
            2,
            "an expired cache entry must refetch instead of serving stale data forever"
        );
        assert!(second.registry_error.is_none());
    }

    #[test]
    fn registry_falls_back_to_last_good_answer_when_a_stale_refetch_fails() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            // Serves exactly one request, then the listener is gone: the
            // refetch after the cache goes stale is a genuine failure, not
            // another good answer.
            let (mut stream, _) = listener.accept().unwrap();
            read_request_headers(&mut stream);
            write_json_response(&mut stream, &mock_registry_body("Widget"));
        });
        let base_url = format!("http://127.0.0.1:{port}/v0.1/servers");
        let budget = Duration::from_millis(300);
        let short_ttl = Duration::from_millis(150);
        let first = search_with(&base_url, "zzz-mock-stale", budget, budget, short_ttl);
        server.join().unwrap();
        assert!(first.registry_error.is_none());
        assert!(first.entries.iter().any(|e| e.name == "Widget"));

        std::thread::sleep(Duration::from_millis(500));
        let second = search_with(&base_url, "zzz-mock-stale", budget, budget, short_ttl);
        let error = second
            .registry_error
            .expect("a failed refetch must still report the error");
        assert_eq!(error.kind, "connectionFailed");
        assert!(
            second.entries.iter().any(|e| e.name == "Widget"),
            "the last good answer must survive a failed refetch"
        );
    }

    const NOW_MS: u64 = 1_800_000_000_000;
    const DAY_MS: u64 = 24 * 60 * 60 * 1000;

    fn widget(name: &str) -> CatalogEntry {
        map_server(&json!({
            "name": format!("io.example/{name}"),
            "title": name,
            "remotes": [{"url": "https://example.com/mcp", "type": "sse"}],
        }))
        .expect("the mock server object maps to an entry")
    }

    /// Serves one canned response per body, in order, then stops listening.
    fn spawn_mock_registry(
        bodies: Vec<String>,
    ) -> (String, Arc<AtomicUsize>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_server = Arc::clone(&hits);
        let server = std::thread::spawn(move || {
            for body in bodies {
                let (mut stream, _) = listener.accept().unwrap();
                read_request_headers(&mut stream);
                hits_server.fetch_add(1, Ordering::SeqCst);
                write_json_response(&mut stream, &body);
            }
        });
        (format!("http://127.0.0.1:{port}/v0.1/servers"), hits, server)
    }

    /// Drops this process's copy of one endpoint's answers, as a restart would.
    fn forget_process_cache(base_url: &str) {
        let mut cache = registry_cache().lock().unwrap_or_else(|e| e.into_inner());
        cache.by_query.retain(|(base, _), _| base != base_url);
        cache.loaded_from = None;
    }

    fn scratch_cache(tag: &str) -> (DataDirFx, PathBuf) {
        let fx = DataDirFx::new("toolport-catalog-cache", tag);
        let path = fx.dir.join(REGISTRY_CACHE_FILE);
        (fx, path)
    }

    fn record(query: &str, fetched_at_ms: u64) -> RegistryCacheRecord {
        RegistryCacheRecord {
            base_url: REGISTRY_URL.to_string(),
            query: query.to_string(),
            fetched_at_ms,
            last_used_ms: 0,
            entries: vec![widget("W")],
        }
    }

    fn write_cache_file(path: &Path, queries: Vec<RegistryCacheRecord>) {
        let file = RegistryCacheFile {
            version: REGISTRY_CACHE_FILE_VERSION,
            queries,
        };
        std::fs::write(path, serde_json::to_string(&file).unwrap()).unwrap();
    }

    fn persist(cache: &RegistryCache, path: &Path, now_ms: u64) {
        write_cache_records(path, cache.snapshot(now_ms).1);
    }

    fn load(cache: &mut RegistryCache, path: &Path, now_ms: u64) {
        cache.merge_loaded(path, read_cache_file(path, now_ms));
    }

    fn loaded_cache(path: &Path) -> RegistryCache {
        let mut cache = RegistryCache::default();
        load(&mut cache, path, NOW_MS);
        cache
    }

    #[test]
    fn instant_search_returns_curated_hits_without_a_network_call() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}/v0.1/servers");

        let result = search_instant_with(&base_url, "slack", false, None);

        assert!(result.entries.iter().any(|e| e.name == "Slack"));
        assert!(result.registry_error.is_none());
        assert!(
            listener.accept().is_err(),
            "instant search must not open a connection to the registry"
        );
    }

    #[test]
    fn instant_search_includes_a_stale_cached_registry_answer() {
        let base_url = "http://registry.invalid/stale-instant";
        let query = "zzz-instant-stale";
        registry_cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                cache_key(base_url, query),
                vec![widget("StaleWidget")],
                unix_ms_now() - 2 * DAY_MS,
            );

        let result = search_instant_with(base_url, query, false, None);

        assert!(result.entries.iter().any(|e| e.name == "StaleWidget"));
        assert!(result.registry_error.is_none());
    }

    #[test]
    fn merged_results_put_curated_first_and_drop_duplicate_names() {
        let mut duplicate = widget("slack");
        duplicate.description = "A registry copy of the curated Slack entry.".into();

        let result = merge_search_results("slack", vec![duplicate, widget("SlackExtras")]);

        let named_slack: Vec<_> = result
            .entries
            .iter()
            .filter(|e| e.name.eq_ignore_ascii_case("slack"))
            .collect();
        assert_eq!(named_slack.len(), 1);
        assert_eq!(named_slack[0].source, "curated");
        let first_registry = result
            .entries
            .iter()
            .position(|e| e.source == "registry")
            .expect("the non-duplicate registry hit is kept");
        assert!(result.entries[..first_registry]
            .iter()
            .all(|e| e.source == "curated"));
        assert_eq!(result.entries[first_registry].name, "SlackExtras");
    }

    #[test]
    fn disk_cache_round_trips_into_a_fresh_cache() {
        let (_fx, path) = scratch_cache("round-trip");
        let slack = cache_key(REGISTRY_URL, "Slack ");
        let git = cache_key(REGISTRY_URL, "git");
        let mut written = RegistryCache::default();
        written.insert(slack.clone(), vec![widget("A")], NOW_MS - 1_000);
        written.insert(git.clone(), vec![widget("B"), widget("C")], NOW_MS - 2_000);
        persist(&written, &path, NOW_MS);

        let restarted = loaded_cache(&path);

        assert_eq!(
            restarted.get(&slack, NOW_MS),
            Some((vec![widget("A")], Duration::from_millis(1_000)))
        );
        assert_eq!(
            restarted.get(&git, NOW_MS),
            Some((vec![widget("B"), widget("C")], Duration::from_millis(2_000)))
        );
    }

    #[test]
    fn disk_cache_keeps_only_the_newest_fifty_queries() {
        let (_fx, path) = scratch_cache("cap");
        let total = REGISTRY_CACHE_MAX_QUERIES + 10;
        let key_of = |i: usize| cache_key(REGISTRY_URL, &format!("query-{i:02}"));
        // A higher index was fetched later.
        let fetched_at = |i: usize| NOW_MS - (total - i) as u64 * 1_000;
        let mut written = RegistryCache::default();
        for i in 0..total {
            written.insert(key_of(i), vec![widget("W")], fetched_at(i));
        }
        assert_eq!(written.by_query.len(), REGISTRY_CACHE_MAX_QUERIES);
        persist(&written, &path, NOW_MS);
        let on_disk: RegistryCacheFile =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk.queries.len(), REGISTRY_CACHE_MAX_QUERIES);

        let restarted = loaded_cache(&path);

        assert_eq!(restarted.by_query.len(), REGISTRY_CACHE_MAX_QUERIES);
        assert!(restarted.get(&key_of(total - 1), NOW_MS).is_some());
        assert!(restarted.get(&key_of(10), NOW_MS).is_some());
        assert!(restarted.get(&key_of(9), NOW_MS).is_none());
    }

    #[test]
    fn loading_an_oversized_cache_file_keeps_only_the_newest_fifty() {
        let (_fx, path) = scratch_cache("oversized");
        let total = REGISTRY_CACHE_MAX_QUERIES + 10;
        let records = (0..total)
            .map(|i| record(&format!("query-{i:02}"), NOW_MS - (total - i) as u64 * 1_000))
            .collect();
        write_cache_file(&path, records);

        let restarted = loaded_cache(&path);

        assert_eq!(restarted.by_query.len(), REGISTRY_CACHE_MAX_QUERIES);
        let has = |query: &str| {
            restarted
                .get(&cache_key(REGISTRY_URL, query), NOW_MS)
                .is_some()
        };
        assert!(has("query-59"));
        assert!(has("query-10"));
        assert!(!has("query-09"));
    }

    #[test]
    fn disk_cache_ignores_a_missing_or_corrupt_file() {
        let (_fx, path) = scratch_cache("corrupt");
        assert!(loaded_cache(&path).by_query.is_empty());

        let foreign_version = json!({
            "version": REGISTRY_CACHE_FILE_VERSION + 1,
            "queries": [record("slack", NOW_MS)],
        })
        .to_string();
        for junk in [
            "",
            "{not json",
            "[]",
            r#"{"version":1}"#,
            r#"{"version":1,"queries":[{"query":"slack"}]}"#,
            foreign_version.as_str(),
        ] {
            std::fs::write(&path, junk).unwrap();
            assert!(loaded_cache(&path).by_query.is_empty(), "{junk}");
        }
    }

    #[test]
    fn disk_cache_write_failure_leaves_the_memory_cache_working() {
        let (fx, _) = scratch_cache("unwritable");
        let blocker = fx.dir.join("blocker");
        std::fs::write(&blocker, "a file where the cache directory should be").unwrap();
        let path = blocker.join(REGISTRY_CACHE_FILE);
        let key = cache_key(REGISTRY_URL, "slack");
        let mut cache = RegistryCache::default();
        cache.insert(key.clone(), vec![widget("A")], NOW_MS);

        persist(&cache, &path, NOW_MS);

        assert!(!path.exists());
        assert!(cache.get(&key, NOW_MS).is_some());
    }

    #[test]
    fn disk_cache_drops_answers_older_than_seven_days_on_load() {
        let (_fx, path) = scratch_cache("max-age-load");
        write_cache_file(
            &path,
            vec![
                record("fresh", NOW_MS - 6 * DAY_MS),
                record("expired", NOW_MS - 8 * DAY_MS),
                record("future", NOW_MS + DAY_MS),
            ],
        );

        let restarted = loaded_cache(&path);

        let has = |query: &str| {
            restarted
                .get(&cache_key(REGISTRY_URL, query), NOW_MS)
                .is_some()
        };
        assert!(has("fresh"));
        assert!(!has("expired"));
        assert!(!has("future"));
    }

    #[test]
    fn disk_cache_drops_answers_older_than_seven_days_on_write() {
        let (_fx, path) = scratch_cache("max-age-write");
        let mut cache = RegistryCache::default();
        cache.insert(cache_key(REGISTRY_URL, "fresh"), vec![widget("W")], NOW_MS);
        cache.insert(
            cache_key(REGISTRY_URL, "expired"),
            vec![widget("W")],
            NOW_MS - 8 * DAY_MS,
        );

        persist(&cache, &path, NOW_MS);

        let on_disk: RegistryCacheFile =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let queries: Vec<_> = on_disk.queries.iter().map(|r| r.query.as_str()).collect();
        assert_eq!(queries, ["fresh"]);
    }

    #[test]
    fn loading_the_disk_cache_keeps_a_newer_in_memory_answer() {
        let (_fx, path) = scratch_cache("newer-memory");
        let key = cache_key(REGISTRY_URL, "slack");
        let mut on_disk = record("slack", NOW_MS - 1_000);
        on_disk.entries = vec![widget("Disk")];
        write_cache_file(&path, vec![on_disk]);

        let mut newer_in_memory = RegistryCache::default();
        newer_in_memory.insert(key.clone(), vec![widget("Memory")], NOW_MS - 100);
        load(&mut newer_in_memory, &path, NOW_MS);
        assert_eq!(
            newer_in_memory.get(&key, NOW_MS).unwrap().0,
            vec![widget("Memory")]
        );

        let mut older_in_memory = RegistryCache::default();
        older_in_memory.insert(key.clone(), vec![widget("Memory")], NOW_MS - 5_000);
        load(&mut older_in_memory, &path, NOW_MS);
        assert_eq!(
            older_in_memory.get(&key, NOW_MS).unwrap().0,
            vec![widget("Disk")]
        );
    }

    #[test]
    fn a_fetched_answer_is_written_to_disk_and_served_after_a_restart() {
        let (_fx, path) = scratch_cache("restart");
        let (base_url, hits, server) = spawn_mock_registry(vec![mock_registry_body("Widget")]);
        let budget = Duration::from_millis(300);
        let ttl = Duration::from_secs(600);
        let query = "zzz-restart";
        let (first, error) =
            registry_search_cached_from(&base_url, query, budget, budget, ttl, true);
        server.join().unwrap();
        assert!(error.is_none());
        assert!(path.exists(), "a successful fetch must write the disk cache");

        forget_process_cache(&base_url);
        let instant = search_instant_with(&base_url, query, true, None);
        assert!(instant.entries.iter().any(|e| e.name == "Widget"));

        forget_process_cache(&base_url);
        let (again, error) =
            registry_search_cached_from(&base_url, query, budget, budget, ttl, true);
        assert!(error.is_none());
        assert_eq!(again, first);
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "a fresh disk answer must not reach the network"
        );
    }

    #[test]
    fn a_stale_disk_answer_shows_instantly_but_is_still_refetched() {
        let (_fx, path) = scratch_cache("stale-disk");
        let (base_url, hits, server) =
            spawn_mock_registry(vec![mock_registry_body("NewWidget")]);
        let query = "zzz-stale-disk";
        let now = unix_ms_now();
        let mut seed = RegistryCache::default();
        seed.insert(
            cache_key(&base_url, query),
            vec![widget("OldWidget")],
            now - 60 * 60 * 1000,
        );
        persist(&seed, &path, now);
        forget_process_cache(&base_url);

        let instant = search_instant_with(&base_url, query, true, None);
        assert!(instant.entries.iter().any(|e| e.name == "OldWidget"));
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        let budget = Duration::from_millis(300);
        let (entries, error) = registry_search_cached_from(
            &base_url,
            query,
            budget,
            budget,
            REGISTRY_CACHE_TTL,
            true,
        );
        server.join().unwrap();
        assert!(error.is_none());
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert!(entries.iter().any(|e| e.name == "NewWidget"));
        assert!(!entries.iter().any(|e| e.name == "OldWidget"));
    }

    #[test]
    fn a_failed_refetch_falls_back_to_the_stale_disk_answer() {
        let (_fx, path) = scratch_cache("stale-disk-down");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let base_url = format!("http://127.0.0.1:{port}/v0.1/servers");
        let query = "zzz-stale-disk-down";
        let now = unix_ms_now();
        let mut seed = RegistryCache::default();
        seed.insert(
            cache_key(&base_url, query),
            vec![widget("OldWidget")],
            now - 60 * 60 * 1000,
        );
        persist(&seed, &path, now);
        forget_process_cache(&base_url);

        let budget = Duration::from_millis(300);
        let (entries, error) = registry_search_cached_from(
            &base_url,
            query,
            budget,
            budget,
            REGISTRY_CACHE_TTL,
            true,
        );

        assert_eq!(error.expect("the refetch must fail").kind, "connectionFailed");
        assert!(entries.iter().any(|e| e.name == "OldWidget"));
    }
    #[test]
    fn eviction_drops_the_least_recently_used_query_not_the_oldest_fetched() {
        let key_of = |i: usize| cache_key(REGISTRY_URL, &format!("query-{i:02}"));
        let mut cache = RegistryCache::default();
        for i in 0..REGISTRY_CACHE_MAX_QUERIES {
            cache.insert(key_of(i), vec![widget("W")], NOW_MS + i as u64);
        }
        cache.touch(&key_of(0), NOW_MS + 1_000);

        cache.insert(
            key_of(REGISTRY_CACHE_MAX_QUERIES),
            vec![widget("W")],
            NOW_MS + 2_000,
        );

        assert_eq!(cache.by_query.len(), REGISTRY_CACHE_MAX_QUERIES);
        assert!(cache.get(&key_of(0), NOW_MS).is_some(), "a used query stays");
        assert!(cache.get(&key_of(1), NOW_MS).is_none(), "the idlest query goes");
    }

    #[test]
    fn a_cache_hit_counts_as_a_use() {
        let base_url = "http://registry.invalid/lru-lookup";
        let key = cache_key(base_url, "zzz-lru-lookup");
        let fetched_at = unix_ms_now() - DAY_MS;
        registry_cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.clone(), vec![widget("W")], fetched_at);

        assert!(cache_lookup(&key, false).is_some());

        let cache = registry_cache().lock().unwrap_or_else(|e| e.into_inner());
        assert!(cache.by_query[&key].last_used_ms > fetched_at);
        assert_eq!(cache.by_query[&key].fetched_at_ms, fetched_at);
    }

    #[test]
    fn the_disk_cache_keeps_the_recently_used_queries_not_the_recently_fetched() {
        let (_fx, path) = scratch_cache("lru-disk");
        let key_of = |i: usize| cache_key(REGISTRY_URL, &format!("query-{i:02}"));
        let mut cache = RegistryCache::default();
        for i in 0..REGISTRY_CACHE_MAX_QUERIES {
            cache.insert(key_of(i), vec![widget("W")], NOW_MS - 10_000 + i as u64);
        }
        cache.touch(&key_of(0), NOW_MS - 1_000);
        cache.insert(key_of(REGISTRY_CACHE_MAX_QUERIES), vec![widget("W")], NOW_MS);
        persist(&cache, &path, NOW_MS);

        let restarted = loaded_cache(&path);

        assert!(restarted.get(&key_of(0), NOW_MS).is_some());
        assert!(restarted.get(&key_of(1), NOW_MS).is_none());
    }

    #[test]
    fn a_cache_file_without_last_use_times_still_loads() {
        let (_fx, path) = scratch_cache("no-last-use");
        let old_format = json!({
            "version": REGISTRY_CACHE_FILE_VERSION,
            "queries": [{
                "baseUrl": REGISTRY_URL,
                "query": "slack",
                "fetchedAtMs": NOW_MS - 1_000,
                "entries": [widget("W")],
            }],
        });
        std::fs::write(&path, old_format.to_string()).unwrap();

        let restarted = loaded_cache(&path);

        assert!(restarted
            .get(&cache_key(REGISTRY_URL, "slack"), NOW_MS)
            .is_some());
    }

    #[test]
    fn a_fetch_that_started_earlier_does_not_replace_a_later_answer() {
        let key = cache_key(REGISTRY_URL, "slack");
        let mut cache = RegistryCache::default();
        assert!(cache.insert_unless_superseded(key.clone(), vec![widget("Later")], NOW_MS, 2));

        let replaced =
            cache.insert_unless_superseded(key.clone(), vec![widget("Earlier")], NOW_MS + 1, 1);

        assert!(!replaced);
        assert_eq!(cache.get(&key, NOW_MS).unwrap().0, vec![widget("Later")]);
        assert!(cache.insert_unless_superseded(key.clone(), vec![widget("Newest")], NOW_MS, 3));
        assert_eq!(cache.get(&key, NOW_MS).unwrap().0, vec![widget("Newest")]);
    }

    #[test]
    fn a_slow_earlier_search_cannot_overwrite_the_answer_of_a_later_one() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let mut handlers = Vec::new();
            for (n, stream) in listener.incoming().take(2).enumerate() {
                let mut stream = stream.unwrap();
                handlers.push(std::thread::spawn(move || {
                    read_request_headers(&mut stream);
                    let (delay, name) = if n == 0 { (600, "Earlier") } else { (0, "Later") };
                    std::thread::sleep(Duration::from_millis(delay));
                    write_json_response(&mut stream, &mock_registry_body(name));
                }));
            }
            for handler in handlers {
                handler.join().unwrap();
            }
        });
        let base_url = format!("http://127.0.0.1:{port}/v0.1/servers");
        let query = "zzz-superseded";
        let budget = Duration::from_secs(5);
        let ttl = Duration::from_millis(1);
        let slow = {
            let base_url = base_url.clone();
            std::thread::spawn(move || {
                registry_search_cached_from(&base_url, query, budget, budget, ttl, false)
            })
        };
        std::thread::sleep(Duration::from_millis(200));
        let (later, _) = registry_search_cached_from(&base_url, query, budget, budget, ttl, false);
        let (earlier, _) = slow.join().unwrap();
        server.join().unwrap();

        assert!(earlier.iter().any(|e| e.name == "Earlier"));
        assert!(later.iter().any(|e| e.name == "Later"));
        let cached = cache_lookup(&cache_key(&base_url, query), false).unwrap().0;
        assert!(cached.iter().any(|e| e.name == "Later"), "{cached:?}");
        assert!(!cached.iter().any(|e| e.name == "Earlier"), "{cached:?}");
    }

    #[test]
    fn a_snapshot_older_than_the_one_on_disk_is_not_written() {
        let (_fx, path) = scratch_cache("snapshot-order");
        let last_written = Mutex::new(0);

        write_cache_snapshot(&last_written, &path, 6, vec![record("newer", NOW_MS)]);
        write_cache_snapshot(&last_written, &path, 5, vec![record("older", NOW_MS)]);
        let on_disk = |path: &Path| -> Vec<String> {
            read_cache_file(path, NOW_MS)
                .into_iter()
                .map(|r| r.query)
                .collect()
        };
        assert_eq!(on_disk(&path), ["newer"]);

        write_cache_snapshot(&last_written, &path, 7, vec![record("newest", NOW_MS)]);
        assert_eq!(on_disk(&path), ["newest"]);
    }

    #[cfg(unix)]
    #[test]
    fn loading_the_disk_cache_does_not_hold_the_cache_lock() {
        use std::os::unix::ffi::OsStrExt;
        let (fx, _) = scratch_cache("lock-while-loading");
        let fifo = fx.dir.join(REGISTRY_CACHE_FILE);
        let c_path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        let loader_path = fifo.clone();
        let loader = std::thread::spawn(move || ensure_cache_loaded(&loader_path, NOW_MS));

        std::thread::sleep(Duration::from_millis(150));
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut lock_was_free = false;
        while !lock_was_free && std::time::Instant::now() < deadline {
            lock_was_free = registry_cache().try_lock().is_ok();
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(std::fs::OpenOptions::new().write(true).open(&fifo).unwrap());
        loader.join().unwrap();

        assert!(
            lock_was_free,
            "the cache lock stayed held while the disk cache was being read"
        );
    }
    fn fresh_index(scratch: &crate::catalog_index::testkit::Scratch, names: &[&str]) {
        use crate::catalog_index::testkit::{entry, index_of, write_index};
        let entries: Vec<_> = names.iter().map(|n| entry(n, "An indexed server")).collect();
        let synced = crate::plus::tasks::cron::rfc3339(crate::plus::tasks::cron::now() - 3600);
        write_index(&scratch.index, &index_of(&entries, Some(&synced), true));
    }

    fn stale_index(scratch: &crate::catalog_index::testkit::Scratch, names: &[&str]) {
        use crate::catalog_index::testkit::{entry, index_of, write_index};
        let entries: Vec<_> = names.iter().map(|n| entry(n, "An indexed server")).collect();
        let synced = crate::plus::tasks::cron::rfc3339(
            crate::plus::tasks::cron::now() - 2 * 24 * 3600,
        );
        write_index(&scratch.index, &index_of(&entries, Some(&synced), true));
    }

    #[test]
    fn instant_search_adds_index_hits_and_says_whether_a_live_call_is_still_needed() {
        use crate::catalog_index::testkit::Scratch;
        let scratch = Scratch::new("instant");
        fresh_index(&scratch, &["IdxWidget"]);
        let base_url = "http://registry.invalid/instant-index";

        let with_index = search_instant_with(base_url, "idxwidget", false, Some(&scratch.index));
        let without_index = search_instant_with(base_url, "idxwidget", false, None);

        assert!(with_index.entries.iter().any(|e| e.name == "IdxWidget"));
        assert_eq!(with_index.live_needed, Some(false));
        assert!(without_index.entries.is_empty());
        assert_eq!(without_index.live_needed, Some(true));
    }

    #[test]
    fn instant_search_keeps_curated_hits_first_and_drops_index_duplicates() {
        use crate::catalog_index::testkit::Scratch;
        let scratch = Scratch::new("instant-order");
        fresh_index(&scratch, &["slack", "SlackExtras"]);

        let result = search_instant_with(
            "http://registry.invalid/instant-order",
            "slack",
            false,
            Some(&scratch.index),
        );

        let named_slack: Vec<_> = result
            .entries
            .iter()
            .filter(|e| e.name.eq_ignore_ascii_case("slack"))
            .collect();
        assert_eq!(named_slack.len(), 1);
        assert_eq!(named_slack[0].source, "curated");
        assert!(result.entries.iter().any(|e| e.name == "SlackExtras"));
    }

    #[test]
    fn a_fresh_index_answers_a_search_without_touching_the_network() {
        use crate::catalog_index::testkit::{closed_port_url, Scratch};
        let scratch = Scratch::new("fresh-no-network");
        fresh_index(&scratch, &["IdxWidget"]);
        let budget = Duration::from_millis(300);

        let result = search_with_index(
            &closed_port_url(),
            "idxwidget",
            budget,
            budget,
            Duration::from_secs(600),
            Some(&scratch.index),
        );

        assert!(result.entries.iter().any(|e| e.name == "IdxWidget"));
        assert!(
            result.registry_error.is_none(),
            "a refused connection would be reported"
        );
    }

    #[test]
    fn a_stale_index_still_answers_and_the_live_results_are_merged_into_it() {
        use crate::catalog_index::testkit::{MockRegistry, Scratch};
        let scratch = Scratch::new("stale-live");
        stale_index(&scratch, &["IdxWidget"]);
        let registry = MockRegistry::start(|_| (200, mock_registry_body("LiveWidget")));
        let budget = Duration::from_millis(500);

        let result = search_with_index(
            &registry.base_url,
            "widget",
            budget,
            budget,
            Duration::from_secs(600),
            Some(&scratch.index),
        );

        assert!(result.registry_error.is_none());
        assert!(result.entries.iter().any(|e| e.name == "IdxWidget"));
        assert!(result.entries.iter().any(|e| e.name == "LiveWidget"));
        assert_eq!(registry.requests().len(), 1);
        let indexed = crate::catalog_index::load(&scratch.index);
        assert!(indexed.entries.contains_key("livewidget"));
        assert!(
            std::fs::read_to_string(&scratch.index)
                .unwrap()
                .contains("LiveWidget"),
            "the merged result is persisted"
        );
    }

    #[test]
    fn a_missing_or_corrupt_index_falls_back_to_the_live_path() {
        use crate::catalog_index::testkit::{MockRegistry, Scratch};
        let scratch = Scratch::new("fallback");
        let registry = MockRegistry::start(|_| (200, mock_registry_body("LiveWidget")));
        let budget = Duration::from_millis(500);
        let ttl = Duration::from_secs(600);

        let missing = search_with_index(
            &registry.base_url,
            "zzz-fallback-missing",
            budget,
            budget,
            ttl,
            Some(&scratch.index),
        );
        assert!(missing.entries.iter().any(|e| e.name == "LiveWidget"));

        std::fs::write(&scratch.index, "{not json").unwrap();
        let corrupt = search_with_index(
            &registry.base_url,
            "zzz-fallback-corrupt",
            budget,
            budget,
            ttl,
            Some(&scratch.index),
        );
        assert!(corrupt.entries.iter().any(|e| e.name == "LiveWidget"));
        assert!(corrupt.registry_error.is_none());
    }

    #[test]
    fn a_failed_live_call_keeps_the_index_hits_and_reports_the_error() {
        use crate::catalog_index::testkit::{closed_port_url, Scratch};
        let scratch = Scratch::new("stale-down");
        stale_index(&scratch, &["IdxWidget"]);
        let budget = Duration::from_millis(300);

        let result = search_with_index(
            &closed_port_url(),
            "idxwidget",
            budget,
            budget,
            Duration::from_secs(600),
            Some(&scratch.index),
        );

        assert!(result.entries.iter().any(|e| e.name == "IdxWidget"));
        assert_eq!(
            result.registry_error.expect("the live call failed").kind,
            "connectionFailed"
        );
    }

    #[test]
    fn a_mock_endpoint_never_reads_the_index() {
        assert_eq!(index_path_for("http://127.0.0.1:1/v0.1/servers"), None);
    }
}
