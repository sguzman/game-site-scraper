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

pub fn init_tracing(cli: &Cli) -> Result<()> {
    let filter =
        EnvFilter::try_new(cli.log_level.clone()).unwrap_or_else(|_| EnvFilter::new("info"));
    let ansi = !cli.no_color;

    match cli.log_format {
        LogFormat::Auto | LogFormat::Pretty => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_ansi(ansi)
                .compact()
                .init();
        }
        LogFormat::Json => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_ansi(ansi)
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
