use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use pvf::app::App;
use pvf::backend::open_default_backend;
use pvf::error::AppResult;
use pvf::perf::{PerfRunConfig, PerfScenarioId, run_report, write_report};
use pvf::presenter::PresenterKind;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CliOptions {
    pdf_path: PathBuf,
    perf: Option<PerfCliOptions>,
}

#[derive(Debug, Parser)]
#[command(
    version,
    about = "PDF viewer for the terminal",
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(value_name = "FILE", required = true)]
    pdf_path: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Perf(PerfCommand),
}

#[derive(Debug, Args)]
struct PerfCommand {
    #[arg(value_name = "FILE")]
    pdf_path: PathBuf,

    #[arg(long, value_enum)]
    scenario: PerfScenarioId,

    #[arg(long, value_name = "PATH|-")]
    out: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PerfCliOptions {
    run: PerfRunConfig,
    out: Option<PathBuf>,
}

async fn run() -> AppResult<()> {
    let options = parse_cli(Cli::parse());

    if let Some(perf) = options.perf {
        let report = run_report(&options.pdf_path, perf.run).await?;
        return write_report(&report, perf.out.as_deref());
    }

    let mut pdf = open_default_backend(&options.pdf_path)?;
    let mut app = App::new(PresenterKind::RatatuiImage)?;
    app.run(pdf.as_mut()).await
}

fn parse_cli(cli: Cli) -> CliOptions {
    match cli.command {
        Some(Commands::Perf(perf)) => CliOptions {
            pdf_path: perf.pdf_path,
            perf: Some(PerfCliOptions {
                run: PerfRunConfig {
                    scenario: perf.scenario,
                    ..PerfRunConfig::default()
                },
                out: perf.out.and_then(|path| {
                    if path.as_os_str() == "-" {
                        None
                    } else {
                        Some(path)
                    }
                }),
            }),
        },
        None => CliOptions {
            pdf_path: cli
                .pdf_path
                .expect("clap enforces pdf path when no subcommand is present"),
            perf: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;
    use pvf::perf::PerfScenarioId;

    use super::{Cli, parse_cli};

    #[test]
    fn parse_cli_accepts_plain_pdf_path() {
        let cli = Cli::try_parse_from(["pvf", "sample.pdf"]).expect("single arg should parse");
        let options = parse_cli(cli);
        assert_eq!(options.pdf_path, PathBuf::from("sample.pdf"));
        assert!(options.perf.is_none());
    }

    #[test]
    fn parse_cli_accepts_perf_options() {
        let cli = Cli::try_parse_from([
            "pvf",
            "perf",
            "sample.pdf",
            "--scenario",
            "page-flip-forward",
            "--out",
            "report.json",
        ])
        .expect("perf args should parse");
        let options = parse_cli(cli);
        let perf = options.perf.expect("perf options should exist");
        assert_eq!(perf.run.scenario, PerfScenarioId::PageFlipForward);
        assert_eq!(perf.out, Some(PathBuf::from("report.json")));
    }

    #[test]
    fn parse_cli_accepts_stdout_perf_output() {
        let cli = Cli::try_parse_from([
            "pvf",
            "perf",
            "sample.pdf",
            "--scenario",
            "idle-pending-redraw",
            "--out",
            "-",
        ])
        .expect("stdout output should parse");
        let options = parse_cli(cli);
        let perf = options.perf.expect("perf options should exist");
        assert_eq!(perf.run.scenario, PerfScenarioId::IdlePendingRedraw);
        assert_eq!(perf.out, None);
    }

    #[test]
    fn parse_cli_rejects_invalid_combinations() {
        assert!(Cli::try_parse_from(["pvf"]).is_err());
        assert!(Cli::try_parse_from(["pvf", "a.pdf", "b.pdf"]).is_err());
        assert!(Cli::try_parse_from(["pvf", "perf", "a.pdf"]).is_err());
        assert!(
            Cli::try_parse_from(["pvf", "sample.pdf", "--perf-run", "page-flip-forward",]).is_err()
        );
        assert!(
            Cli::try_parse_from(["pvf", "perf", "sample.pdf", "--scenario", "unknown",]).is_err()
        );
    }
}
