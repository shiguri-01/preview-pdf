use std::path::PathBuf;

use clap::Parser;
use pvf::app::App;
use pvf::backend::open_default_backend;
use pvf::error::AppResult;
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
}

#[derive(Debug, Parser)]
#[command(version, about = "PDF viewer for the terminal")]
struct Cli {
    #[arg(value_name = "FILE")]
    pdf_path: PathBuf,
}

async fn run() -> AppResult<()> {
    let options = parse_cli(Cli::parse());

    let pdf = open_default_backend(&options.pdf_path)?;
    let mut app = App::new(PresenterKind::RatatuiImage)?;
    app.run(pdf).await
}

fn parse_cli(cli: Cli) -> CliOptions {
    CliOptions {
        pdf_path: cli.pdf_path,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::{Cli, parse_cli};

    #[test]
    fn parse_cli_accepts_plain_pdf_path() {
        let cli = Cli::try_parse_from(["pvf", "sample.pdf"]).expect("single arg should parse");
        let options = parse_cli(cli);
        assert_eq!(options.pdf_path, PathBuf::from("sample.pdf"));
    }

    #[test]
    fn parse_cli_rejects_invalid_combinations() {
        assert!(Cli::try_parse_from(["pvf"]).is_err());
        assert!(Cli::try_parse_from(["pvf", "a.pdf", "b.pdf"]).is_err());
        assert!(Cli::try_parse_from(["pvf", "sample.pdf", "--scenario", "unknown",]).is_err());
    }
}
