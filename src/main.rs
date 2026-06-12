use std::path::PathBuf;

use clap::{Parser, ValueEnum};
#[cfg(not(test))]
use pvf::app::App;
use pvf::app::PageLayoutMode;
#[cfg(not(test))]
use pvf::backend::open_default_backend;
use pvf::config::{AppOptions, ConfigFileSelection, ViewOptions, WatchOptions};
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

#[derive(Debug, Clone, PartialEq)]
struct CliOptions {
    pdf_path: PathBuf,
    config: ConfigFileSelection,
    options: AppOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliPageLayout {
    Single,
    Spread,
}

impl From<CliPageLayout> for PageLayoutMode {
    fn from(value: CliPageLayout) -> Self {
        match value {
            CliPageLayout::Single => Self::Single,
            CliPageLayout::Spread => Self::Spread,
        }
    }
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
    #[arg(
        long,
        conflicts_with = "no_watch",
        help = "Reload the displayed PDF when the file changes"
    )]
    watch: bool,
    #[arg(long, help = "Do not watch the input PDF for changes")]
    no_watch: bool,
    #[arg(long, value_name = "N", help = "Open the PDF at page N")]
    page: Option<usize>,
    #[arg(
        long,
        value_name = "RATIO",
        help = "Set the initial zoom ratio relative to fit"
    )]
    zoom: Option<f32>,
    #[arg(long, value_enum, help = "Set the initial page layout")]
    layout: Option<CliPageLayout>,
    #[arg(value_name = "FILE")]
    pdf_path: PathBuf,
}

#[cfg(not(test))]
async fn run() -> AppResult<()> {
    let options = parse_cli(Cli::parse());

    let pdf = open_default_backend(&options.pdf_path)?;
    let app_options = options.config.load_options()?.merge(options.options);
    let mut app = App::new_with_options(PresenterKind::RatatuiImage, app_options)?;
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
        config,
        options: AppOptions {
            view: ViewOptions {
                initial_page: cli.page,
                initial_zoom: cli.zoom,
                initial_layout: cli.layout.map(PageLayoutMode::from),
                ..ViewOptions::default()
            },
            watch: WatchOptions {
                enabled: if cli.watch {
                    Some(true)
                } else if cli.no_watch {
                    Some(false)
                } else {
                    None
                },
                ..WatchOptions::default()
            },
            ..AppOptions::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;
    use pvf::app::PageLayoutMode;
    use pvf::config::ConfigFileSelection;

    use super::{Cli, parse_cli};

    #[test]
    fn parse_cli_accepts_plain_pdf_path() {
        let cli = Cli::try_parse_from(["pvf", "sample.pdf"]).expect("single arg should parse");
        let options = parse_cli(cli);
        assert_eq!(options.pdf_path, PathBuf::from("sample.pdf"));
        assert_eq!(options.config, ConfigFileSelection::Default);
        assert_eq!(options.options.watch.enabled, None);
    }

    #[test]
    fn parse_cli_accepts_watch_flag() {
        let cli =
            Cli::try_parse_from(["pvf", "--watch", "sample.pdf"]).expect("watch flag should parse");
        let options = parse_cli(cli);
        assert_eq!(options.pdf_path, PathBuf::from("sample.pdf"));
        assert_eq!(options.config, ConfigFileSelection::Default);
        assert_eq!(options.options.watch.enabled, Some(true));
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
    fn parse_cli_accepts_no_watch_override() {
        let cli = Cli::try_parse_from(["pvf", "--no-watch", "sample.pdf"])
            .expect("no-watch should parse");
        let options = parse_cli(cli);
        assert_eq!(options.options.watch.enabled, Some(false));
    }

    #[test]
    fn parse_cli_accepts_initial_view_overrides() {
        let cli = Cli::try_parse_from([
            "pvf",
            "--page",
            "10",
            "--zoom",
            "1.25",
            "--layout",
            "spread",
            "sample.pdf",
        ])
        .expect("view overrides should parse");
        let options = parse_cli(cli);
        assert_eq!(options.options.view.initial_page, Some(10));
        assert_eq!(options.options.view.initial_zoom, Some(1.25));
        assert_eq!(
            options.options.view.initial_layout,
            Some(PageLayoutMode::Spread)
        );
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
        assert!(Cli::try_parse_from(["pvf", "--watch", "--no-watch", "sample.pdf"]).is_err());
        assert!(Cli::try_parse_from(["pvf", "--layout", "grid", "sample.pdf"]).is_err());
    }
}
