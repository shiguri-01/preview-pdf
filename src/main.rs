use std::ffi::OsString;
use std::path::PathBuf;

use pvf::app::App;
use pvf::backend::open_default_backend;
use pvf::error::{AppError, AppResult};
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct PerfCliOptions {
    run: PerfRunConfig,
    out: Option<PathBuf>,
}

async fn run() -> AppResult<()> {
    let options = parse_cli(std::env::args_os())?;

    if let Some(perf) = options.perf {
        let report = run_report(&options.pdf_path, perf.run).await?;
        return write_report(&report, perf.out.as_deref());
    }

    let mut pdf = open_default_backend(&options.pdf_path)?;
    let mut app = App::new(PresenterKind::RatatuiImage)?;
    app.run(pdf.as_mut()).await
}

fn parse_cli<I>(args: I) -> AppResult<CliOptions>
where
    I: Iterator<Item = OsString>,
{
    let mut args = args;
    let _program = args.next();

    let mut pdf_path = None;
    let mut perf_scenario = None;
    let mut perf_out = None;

    while let Some(arg) = args.next() {
        match arg.to_string_lossy().as_ref() {
            "--perf-run" => {
                let Some(value) = args.next() else {
                    return Err(AppError::invalid_argument(
                        "usage: pvf <file.pdf> [--perf-run <scenario-id>] [--perf-out <path|->]",
                    ));
                };
                let scenario = value
                    .to_str()
                    .and_then(PerfScenarioId::parse)
                    .ok_or_else(|| {
                        AppError::invalid_argument(format!(
                            "unknown perf scenario: {}",
                            value.to_string_lossy()
                        ))
                    })?;
                perf_scenario = Some(scenario);
            }
            "--perf-out" => {
                let Some(value) = args.next() else {
                    return Err(AppError::invalid_argument(
                        "usage: pvf <file.pdf> [--perf-run <scenario-id>] [--perf-out <path|->]",
                    ));
                };
                if value != "-" {
                    perf_out = Some(PathBuf::from(value));
                }
            }
            value if value.starts_with('-') => {
                return Err(AppError::invalid_argument(format!(
                    "unknown option: {value}"
                )));
            }
            _ => {
                if pdf_path.is_some() {
                    return Err(AppError::invalid_argument(
                        "usage: pvf <file.pdf> [--perf-run <scenario-id>] [--perf-out <path|->]",
                    ));
                }
                pdf_path = Some(PathBuf::from(arg));
            }
        }
    }

    let Some(pdf_path) = pdf_path else {
        return Err(AppError::invalid_argument(
            "usage: pvf <file.pdf> [--perf-run <scenario-id>] [--perf-out <path|->]",
        ));
    };

    if perf_out.is_some() && perf_scenario.is_none() {
        return Err(AppError::invalid_argument("--perf-out requires --perf-run"));
    }

    Ok(CliOptions {
        pdf_path,
        perf: perf_scenario.map(|scenario| PerfCliOptions {
            run: PerfRunConfig {
                scenario,
                ..PerfRunConfig::default()
            },
            out: perf_out,
        }),
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use pvf::perf::PerfScenarioId;

    use super::parse_cli;

    #[test]
    fn parse_cli_accepts_plain_pdf_path() {
        let args = vec![OsString::from("pvf"), OsString::from("sample.pdf")];

        let options = parse_cli(args.into_iter()).expect("single arg should parse");
        assert_eq!(options.pdf_path, PathBuf::from("sample.pdf"));
        assert!(options.perf.is_none());
    }

    #[test]
    fn parse_cli_accepts_perf_options() {
        let args = vec![
            OsString::from("pvf"),
            OsString::from("sample.pdf"),
            OsString::from("--perf-run"),
            OsString::from("page-flip-forward"),
            OsString::from("--perf-out"),
            OsString::from("report.json"),
        ];

        let options = parse_cli(args.into_iter()).expect("perf args should parse");
        let perf = options.perf.expect("perf options should exist");
        assert_eq!(perf.run.scenario, PerfScenarioId::PageFlipForward);
        assert_eq!(perf.out, Some(PathBuf::from("report.json")));
    }

    #[test]
    fn parse_cli_rejects_invalid_combinations() {
        let missing = vec![OsString::from("pvf")];
        assert!(parse_cli(missing.into_iter()).is_err());

        let extra = vec![
            OsString::from("pvf"),
            OsString::from("a.pdf"),
            OsString::from("b.pdf"),
        ];
        assert!(parse_cli(extra.into_iter()).is_err());

        let out_without_run = vec![
            OsString::from("pvf"),
            OsString::from("a.pdf"),
            OsString::from("--perf-out"),
            OsString::from("report.json"),
        ];
        assert!(parse_cli(out_without_run.into_iter()).is_err());
    }
}
