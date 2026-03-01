use std::ffi::OsString;

use pvf::app::App;
use pvf::backend::open_default_backend;
use pvf::error::{AppError, AppResult};
use pvf::presenter::PresenterKind;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

async fn run() -> AppResult<()> {
    let pdf_path = parse_cli_path(std::env::args_os())?;

    let mut pdf = open_default_backend(&pdf_path)?;
    let mut app = App::new(PresenterKind::RatatuiImage)?;

    app.run(pdf.as_mut()).await
}

fn parse_cli_path<I>(mut args: I) -> AppResult<OsString>
where
    I: Iterator<Item = OsString>,
{
    let _program = args.next();
    let Some(path) = args.next() else {
        return Err(AppError::invalid_argument("usage: pvf <file.pdf>"));
    };

    if args.next().is_some() {
        return Err(AppError::invalid_argument(
            "usage: pvf <file.pdf> (exactly one path argument is required)",
        ));
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::parse_cli_path;

    #[test]
    fn parse_cli_path_accepts_single_pdf_arg() {
        let args = vec![OsString::from("pvf"), OsString::from("sample.pdf")];

        let path = parse_cli_path(args.into_iter()).expect("single arg should parse");
        assert_eq!(path, OsString::from("sample.pdf"));
    }

    #[test]
    fn parse_cli_path_rejects_missing_or_extra_args() {
        let missing = vec![OsString::from("pvf")];
        assert!(parse_cli_path(missing.into_iter()).is_err());

        let extra = vec![
            OsString::from("pvf"),
            OsString::from("a.pdf"),
            OsString::from("b.pdf"),
        ];
        assert!(parse_cli_path(extra.into_iter()).is_err());
    }
}
