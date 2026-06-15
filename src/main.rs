mod cli;

#[cfg(not(test))]
use pvf::app::App;
#[cfg(not(test))]
use pvf::backend::open_default_backend;
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

#[cfg(not(test))]
async fn run() -> AppResult<()> {
    let options = cli::parse();

    let pdf = open_default_backend(&options.pdf_path)?;
    let app_options = options.config.load_options()?.merge(options.options);
    let mut app = App::new_with_options(PresenterKind::RatatuiImage, app_options)?;
    app.run(pdf).await
}
