use std::ffi::OsString;

use pvf::app::App;
use pvf::backend::open_default_backend;
use pvf::error::{AppError, AppResult};
use pvf::perf_report::run_fixed_perf_report;
use pvf::presenter::PresenterKind;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

async fn run() -> AppResult<()> {
    match parse_cli_args(std::env::args_os())? {
        CliCommand::Viewer { pdf_path } => {
            let mut pdf = open_default_backend(&pdf_path)?;
            let mut app = App::new(PresenterKind::RatatuiImage)?;
            app.run(pdf.as_mut()).await
        }
        CliCommand::PerfReport { pdf_path } => {
            let report = run_fixed_perf_report(std::path::Path::new(&pdf_path)).await?;
            print!("{report}");
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliCommand {
    Viewer { pdf_path: OsString },
    PerfReport { pdf_path: OsString },
}

fn parse_cli_args<I>(mut args: I) -> AppResult<CliCommand>
where
    I: Iterator<Item = OsString>,
{
    let _program = args.next();
    let mut perf_report = false;
    let mut path = None;

    for arg in args {
        if arg == "--perf-report" {
            perf_report = true;
            continue;
        }
        if arg == "--format" {
            return Err(AppError::invalid_argument(
                "usage: pvf --perf-report <file.pdf>",
            ));
        }
        if path.replace(arg).is_some() {
            return Err(AppError::invalid_argument(
                "usage: pvf <file.pdf> or pvf --perf-report <file.pdf>",
            ));
        }
    }

    let Some(pdf_path) = path else {
        return Err(AppError::invalid_argument(
            "usage: pvf <file.pdf> or pvf --perf-report <file.pdf>",
        ));
    };

    Ok(if perf_report {
        CliCommand::PerfReport { pdf_path }
    } else {
        CliCommand::Viewer { pdf_path }
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{CliCommand, parse_cli_args};

    #[test]
    fn parse_cli_path_accepts_single_pdf_arg() {
        let args = vec![OsString::from("pvf"), OsString::from("sample.pdf")];

        let command = parse_cli_args(args.into_iter()).expect("single arg should parse");
        assert_eq!(
            command,
            CliCommand::Viewer {
                pdf_path: OsString::from("sample.pdf")
            }
        );
    }

    #[test]
    fn parse_cli_path_rejects_missing_or_extra_args() {
        let missing = vec![OsString::from("pvf")];
        assert!(parse_cli_args(missing.into_iter()).is_err());

        let extra = vec![
            OsString::from("pvf"),
            OsString::from("a.pdf"),
            OsString::from("b.pdf"),
        ];
        assert!(parse_cli_args(extra.into_iter()).is_err());
    }

    #[test]
    fn parse_cli_path_supports_perf_report() {
        let args = vec![
            OsString::from("pvf"),
            OsString::from("--perf-report"),
            OsString::from("sample.pdf"),
        ];

        let command = parse_cli_args(args.into_iter()).expect("perf report args should parse");
        assert_eq!(
            command,
            CliCommand::PerfReport {
                pdf_path: OsString::from("sample.pdf"),
            }
        );
    }

    #[test]
    fn parse_cli_path_rejects_perf_report_format_flag() {
        let args = vec![
            OsString::from("pvf"),
            OsString::from("--perf-report"),
            OsString::from("--format"),
            OsString::from("json"),
            OsString::from("sample.pdf"),
        ];

        assert!(parse_cli_args(args.into_iter()).is_err());
    }
}
