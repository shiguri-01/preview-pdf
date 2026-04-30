use std::path::PathBuf;
use std::str::FromStr;

use clap::Parser;
use pvf::error::{AppError, AppResult};
use pvf::perf::{PerfScenarioId, PerfSuiteConfig, run_suite, write_report};

#[derive(Debug, Parser)]
#[command(version, about = "Headless pvf performance diagnostics")]
struct BenchArgs {
    #[arg(long, value_name = "PATH")]
    pdf: PathBuf,

    #[arg(long, value_name = "ID|all")]
    scenario: Vec<String>,

    #[arg(long, default_value_t = 1)]
    warmup: usize,

    #[arg(long, default_value_t = 5)]
    iterations: usize,

    #[arg(long, default_value_t = 8)]
    page_steps: usize,

    #[arg(long, default_value_t = 250)]
    idle_ms: u64,

    #[arg(long, value_name = "PATH")]
    out: Option<PathBuf>,

    #[arg(long, hide = true)]
    bench: bool,
}

impl BenchArgs {
    fn suite_config(&self) -> AppResult<PerfSuiteConfig> {
        let scenarios = parse_scenarios(&self.scenario)?;
        Ok(PerfSuiteConfig {
            pdf_path: self.pdf.clone(),
            scenarios,
            warmup_iterations: self.warmup,
            measured_iterations: self.iterations,
            page_steps: self.page_steps,
            idle_ms: self.idle_ms,
        })
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args = BenchArgs::parse();
    let config = match args.suite_config() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    };

    match run_suite(config)
        .await
        .and_then(|report| write_report(&report, args.out.as_deref()))
    {
        Ok(()) => {}
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}

fn parse_scenarios(values: &[String]) -> AppResult<Vec<PerfScenarioId>> {
    if values.is_empty() || values.iter().any(|value| value == "all") {
        return Ok(PerfScenarioId::all().to_vec());
    }

    values
        .iter()
        .map(|value| PerfScenarioId::from_str(value))
        .collect::<Result<Vec<_>, AppError>>()
}
