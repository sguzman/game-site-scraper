use anyhow::Result;
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "game-scraper")]
#[command(about = "Parse saved game release HTML files into JSON metadata.")]
#[command(version)]
#[command(propagate_version = true)]
pub struct Cli {
    #[arg(short, long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    #[arg(long, global = true, default_value = "info", env = "GAME_SCRAPER_LOG")]
    pub log_level: String,

    #[arg(long, global = true, default_value = "auto", value_enum)]
    pub log_format: LogFormat,

    #[arg(long, global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Parse(ParseArgs),
    InitConfig(InitConfigArgs),
    PrintConfig(PrintConfigArgs),
    Completions(CompletionsArgs),
    Meilisearch(MeilisearchArgs),
}

#[derive(Args, Debug)]
pub struct ParseArgs {
    #[arg(value_name = "INPUT", required = true)]
    pub inputs: Vec<PathBuf>,

    #[arg(short, long)]
    pub recursive: bool,

    #[arg(long)]
    pub follow_symlinks: bool,

    #[arg(short, long, value_name = "PATH")]
    pub output: Option<PathBuf>,

    #[arg(long)]
    pub pretty: bool,

    #[arg(long)]
    pub ndjson: bool,
}

#[derive(Args, Debug)]
pub struct InitConfigArgs {
    #[arg(long, value_name = "PATH", default_value = "scrape.toml")]
    pub path: PathBuf,
}

#[derive(Args, Debug)]
pub struct PrintConfigArgs {
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct CompletionsArgs {
    #[arg(value_enum)]
    pub shell: ShellArg,
}

#[derive(Args, Debug)]
pub struct MeilisearchArgs {
    #[arg(value_name = "INPUT", required = true)]
    pub inputs: Vec<PathBuf>,

    #[arg(short, long)]
    pub recursive: bool,

    #[arg(long)]
    pub follow_symlinks: bool,

    #[arg(long, value_enum)]
    pub mode: Option<MeilisearchModeArg>,

    #[arg(long, value_name = "URL")]
    pub host: Option<String>,

    #[arg(long, value_name = "UID")]
    pub index: Option<String>,

    #[arg(long, value_name = "KEY")]
    pub api_key: Option<String>,

    #[arg(long, value_name = "KEY")]
    pub primary_key: Option<String>,

    #[arg(long, value_enum)]
    pub id_strategy: Option<MeilisearchIdStrategyArg>,

    #[arg(long, value_name = "N")]
    pub batch_size: Option<usize>,

    #[arg(long, value_name = "SECS")]
    pub timeout_secs: Option<u64>,

    #[arg(long)]
    pub apply_settings: bool,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long, value_name = "PATH")]
    pub from_json: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    pub from_ndjson: Option<PathBuf>,

    #[arg(long)]
    pub stats_only: bool,

    #[arg(long)]
    pub settings_only: bool,

    #[arg(long, value_name = "N")]
    pub sample: Option<usize>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ShellArg {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Elvish,
}

impl From<ShellArg> for Shell {
    fn from(v: ShellArg) -> Self {
        match v {
            ShellArg::Bash => Shell::Bash,
            ShellArg::Zsh => Shell::Zsh,
            ShellArg::Fish => Shell::Fish,
            ShellArg::PowerShell => Shell::PowerShell,
            ShellArg::Elvish => Shell::Elvish,
        }
    }
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum LogFormat {
    Auto,
    Pretty,
    Json,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum MeilisearchModeArg {
    Upsert,
    CleanInsert,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum MeilisearchIdStrategyArg {
    Sha256,
    CanonicalUrl,
    TitleSlug,
}

pub fn init_tracing(cli: &Cli) -> Result<()> {
    let filter_str = normalize_log_filter(&cli.log_level);
    let filter = EnvFilter::try_new(filter_str).unwrap_or_else(|_| EnvFilter::new("info"));
    let ansi = !cli.no_color;

    match cli.log_format {
        LogFormat::Auto | LogFormat::Pretty => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_ansi(ansi)
                .with_writer(std::io::stderr)
                .compact()
                .init();
        }
        LogFormat::Json => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_ansi(ansi)
                .with_writer(std::io::stderr)
                .json()
                .init();
        }
    }

    Ok(())
}

pub fn print_completions(shell: ShellArg) {
    let mut cmd = Cli::command();
    let shell: Shell = shell.into();
    generate(shell, &mut cmd, "game-scraper", &mut std::io::stdout());
}

fn normalize_log_filter(input: &str) -> String {
    let level = input.trim().to_ascii_lowercase();
    let is_simple_level = matches!(
        level.as_str(),
        "error" | "warn" | "info" | "debug" | "trace"
    );
    let looks_like_filter = input.contains('=') || input.contains(',');

    if is_simple_level && !looks_like_filter {
        format!("warn,game_scraper={level}")
    } else {
        input.to_string()
    }
}
