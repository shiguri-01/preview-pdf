use std::path::PathBuf;

use clap::Parser;
#[cfg(not(test))]
use pvf::app::App;
#[cfg(not(test))]
use pvf::backend::open_default_backend;
use pvf::config::ConfigFileSelection;
#[cfg(not(test))]
use pvf::error::AppResult;
#[cfg(not(test))]
use pvf::presenter::PresenterKind;

#[cfg(not(test))]
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

#[cfg(test)]
fn main() {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CliOptions {
    pdf_path: PathBuf,
    watch: bool,
    config: ConfigFileSelection,
}

#[derive(Debug, Parser)]
#[command(version, about = "PDF viewer for the terminal")]
struct Cli {
    #[arg(
        long,
        value_name = "PATH",
        conflicts_with = "no_config",
        help = "Read configuration from PATH"
    )]
    config: Option<PathBuf>,
    #[arg(long, help = "Do not read a configuration file")]
    no_config: bool,
    #[arg(long, help = "Reload the displayed PDF when the file changes")]
    watch: bool,
    #[arg(value_name = "FILE")]
    pdf_path: PathBuf,
}

#[cfg(not(test))]
async fn run() -> AppResult<()> {
    let options = parse_cli(Cli::parse());

    let pdf = open_default_backend(&options.pdf_path)?;
    let app_options = options.config.load_options()?;
    let mut app = App::new_with_options(PresenterKind::RatatuiImage, app_options)?;
    app.set_watch(options.watch);
    app.run(pdf).await
}

fn parse_cli(cli: Cli) -> CliOptions {
    let config = if cli.no_config {
        ConfigFileSelection::Disabled
    } else if let Some(path) = cli.config {
        ConfigFileSelection::Path(path)
    } else {
        ConfigFileSelection::Default
    };
    CliOptions {
        pdf_path: cli.pdf_path,
        watch: cli.watch,
        config,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;
    use pvf::config::ConfigFileSelection;

    use super::{Cli, parse_cli};

    #[test]
    fn parse_cli_accepts_plain_pdf_path() {
        let cli = Cli::try_parse_from(["pvf", "sample.pdf"]).expect("single arg should parse");
        let options = parse_cli(cli);
        assert_eq!(options.pdf_path, PathBuf::from("sample.pdf"));
        assert!(!options.watch);
        assert_eq!(options.config, ConfigFileSelection::Default);
    }

    #[test]
    fn parse_cli_accepts_watch_flag() {
        let cli =
            Cli::try_parse_from(["pvf", "--watch", "sample.pdf"]).expect("watch flag should parse");
        let options = parse_cli(cli);
        assert_eq!(options.pdf_path, PathBuf::from("sample.pdf"));
        assert!(options.watch);
        assert_eq!(options.config, ConfigFileSelection::Default);
    }

    #[test]
    fn parse_cli_accepts_explicit_config_path() {
        let cli = Cli::try_parse_from(["pvf", "--config", "pvf.toml", "sample.pdf"])
            .expect("config path should parse");
        let options = parse_cli(cli);
        assert_eq!(
            options.config,
            ConfigFileSelection::Path(PathBuf::from("pvf.toml"))
        );
    }

    #[test]
    fn parse_cli_accepts_no_config() {
        let cli = Cli::try_parse_from(["pvf", "--no-config", "sample.pdf"])
            .expect("no-config should parse");
        let options = parse_cli(cli);
        assert_eq!(options.config, ConfigFileSelection::Disabled);
    }

    #[test]
    fn parse_cli_rejects_invalid_combinations() {
        assert!(Cli::try_parse_from(["pvf"]).is_err());
        assert!(Cli::try_parse_from(["pvf", "a.pdf", "b.pdf"]).is_err());
        assert!(Cli::try_parse_from(["pvf", "sample.pdf", "--scenario", "unknown",]).is_err());
        assert!(
            Cli::try_parse_from(["pvf", "--config", "pvf.toml", "--no-config", "sample.pdf"])
                .is_err()
        );
    }
}
