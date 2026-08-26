use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kal: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: impl Iterator<Item = std::ffi::OsString>) -> Result<(), kal::Error> {
    let mut arguments = arguments;
    let command = arguments
        .next()
        .ok_or_else(|| kal::Error::new("usage: kal build <input> [-o <output>]"))?;
    if command != "build" {
        return Err(kal::Error::new(format!(
            "unknown command {:?}; expected `build`",
            command
        )));
    }

    let input = PathBuf::from(
        arguments
            .next()
            .ok_or_else(|| kal::Error::new("build requires an input file"))?,
    );
    let mut output = None;

    while let Some(argument) = arguments.next() {
        if argument != "-o" {
            return Err(kal::Error::new(format!(
                "unexpected argument {:?}",
                argument
            )));
        }
        if output.is_some() {
            return Err(kal::Error::new("`-o` may only be supplied once"));
        }
        output =
            Some(PathBuf::from(arguments.next().ok_or_else(|| {
                kal::Error::new("`-o` requires an output path")
            })?));
    }

    let output = output.unwrap_or(kal::default_output_path(&input)?);
    kal::build(&input, &output)
}
