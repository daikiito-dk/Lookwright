//! `lookwright` — Headless Perception & 6DoF Motion Test Framework for Smart
//! Glasses & Spatial AI. CLI contract: SPEC.md §10 (exit codes 0/1/2).

mod assert;
mod cmd_golden;
mod cmd_perception;
mod cmd_validate;
mod cmd_worlds;
mod config;
mod junit;
mod report;
mod runner;

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use lookwright_core::Issue;

#[derive(Parser)]
#[command(
    name = "lookwright",
    version,
    about = "Headless Perception & 6DoF Motion Test Framework for Smart Glasses & Spatial AI"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Arguments shared by `run` and `test` (SPEC.md §10).
#[derive(clap::Args)]
pub(crate) struct RunArgs {
    /// Test config path (JSON, SPEC.md §6)
    #[arg(long)]
    pub(crate) config: PathBuf,
    /// Write a JSON report (SPEC.md §11)
    #[arg(long)]
    pub(crate) report: Option<PathBuf>,
    /// Write a JUnit XML report (SPEC.md §11)
    #[arg(long)]
    pub(crate) junit: Option<PathBuf>,
    /// No-op: Lookwright is always headless
    #[arg(long)]
    pub(crate) headless: bool,
    /// Device profile recorded in the report (ray-ban-meta|orion|horizon-os|generic)
    #[arg(long)]
    pub(crate) target: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Run a spatial test config end to end (assertions + golden images).
    Run {
        #[command(flatten)]
        args: RunArgs,
    },
    /// Alias of `run`.
    Test {
        #[command(flatten)]
        args: RunArgs,
    },
    /// Regenerate golden frame images for a test config.
    Golden {
        #[command(subcommand)]
        cmd: GoldenCmd,
    },
    /// Dump synthetic perception frames (mock input for contextual AI).
    Perception {
        /// Preset world id or world file path (JSON/glTF)
        #[arg(long)]
        world: String,
        /// Trajectory path (JSON or YAML, SPEC.md §2)
        #[arg(long)]
        trajectory: PathBuf,
        /// Capture a single frame at this time (seconds); default: every frame
        /// at 30 hz
        #[arg(long, value_name = "T_SEC")]
        at_sec: Option<f32>,
        /// Feed format
        #[arg(long, value_enum, default_value_t = Format::Json)]
        format: Format,
        /// Output path (default stdout)
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Built-in world utilities.
    Worlds {
        #[command(subcommand)]
        cmd: WorldsCmd,
    },
    /// Validate documents without running them.
    Validate {
        #[arg(long)]
        trajectory: Option<PathBuf>,
        #[arg(long)]
        world: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum GoldenCmd {
    /// Re-render every golden frame of a config (SPEC.md §12; never hand-edit).
    Update {
        #[arg(long)]
        config: PathBuf,
    },
}

#[derive(Subcommand)]
enum WorldsCmd {
    /// List preset world ids.
    List,
    /// Show a preset world's objects as JSON.
    Show { id: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Format {
    Json,
    Jsonld,
}

fn main() {
    let cli = Cli::parse();
    let code = match dispatch(cli) {
        Ok(code) => code,
        Err(issue) => {
            eprintln!("error: {issue}");
            2
        }
    };
    std::process::exit(code);
}

fn dispatch(cli: Cli) -> Result<i32, Issue> {
    match cli.command {
        Command::Run { args } | Command::Test { args } => runner::run(&args),
        Command::Golden { cmd } => match cmd {
            GoldenCmd::Update { config } => cmd_golden::update(&config),
        },
        Command::Perception {
            world,
            trajectory,
            at_sec,
            format,
            out,
        } => cmd_perception::run(
            &world,
            &trajectory,
            at_sec,
            format == Format::Jsonld,
            out.as_deref(),
        ),
        Command::Worlds { cmd } => match cmd {
            WorldsCmd::List => cmd_worlds::list(),
            WorldsCmd::Show { id } => cmd_worlds::show(&id),
        },
        Command::Validate {
            trajectory,
            world,
            config,
        } => cmd_validate::run(trajectory.as_deref(), world.as_deref(), config.as_deref()),
    }
}
