use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

#[derive(Clone, Copy)]
enum Mode {
    Run,
    CompileFail,
}

fn run_spec(source: &Path) -> datatest_stable::Result<()> {
    check_spec(source, Mode::Run)
}

fn compile_fail_spec(source: &Path) -> datatest_stable::Result<()> {
    check_spec(source, Mode::CompileFail)
}

fn check_spec(source: &Path, mode: Mode) -> datatest_stable::Result<()> {
    let directory = tempfile::tempdir()?;
    let executable = directory.path().join("program");
    let compiler = Command::new(env!("CARGO_BIN_EXE_kal"))
        .arg("build")
        .arg(source)
        .arg("-o")
        .arg(&executable)
        .output()?;

    match mode {
        Mode::Run => {
            compare_process(source, "compiler", &compiler, 0, &[], &[])?;
            assert_amd64_elf(&executable)?;

            let program = Command::new(&executable).output()?;
            compare_process(
                source,
                "program",
                &program,
                expected_exit(source, 0)?,
                &expected_bytes(source, "stdout")?,
                &expected_bytes(source, "stderr")?,
            )?;
        }
        Mode::CompileFail => {
            compare_process(
                source,
                "compiler",
                &compiler,
                expected_exit(source, 1)?,
                &expected_bytes(source, "stdout")?,
                &expected_bytes(source, "stderr")?,
            )?;
            if executable.exists() {
                return Err(error(format!(
                    "{}: failed compilation created {}",
                    source.display(),
                    executable.display()
                )));
            }
        }
    }

    Ok(())
}

fn expected_bytes(source: &Path, extension: &str) -> io::Result<Vec<u8>> {
    let path = source.with_extension(extension);
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn expected_exit(source: &Path, default: i32) -> io::Result<i32> {
    let path = source.with_extension("exit");
    let value = match fs::read_to_string(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(default),
        Err(error) => return Err(error),
    };
    value.trim().parse().map_err(|parse_error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} must contain an integer: {parse_error}", path.display()),
        )
    })
}

fn compare_process(
    source: &Path,
    process: &str,
    output: &Output,
    expected_exit: i32,
    expected_stdout: &[u8],
    expected_stderr: &[u8],
) -> datatest_stable::Result<()> {
    let actual_exit = output.status.code().ok_or_else(|| {
        error(format!(
            "{}: {process} terminated without an exit code",
            source.display()
        ))
    })?;
    if actual_exit != expected_exit {
        return Err(error(format!(
            "{}: {process} exit status\nexpected: {expected_exit}\n  actual: {actual_exit}",
            source.display()
        )));
    }
    compare_bytes(
        source,
        &format!("{process} stdout"),
        expected_stdout,
        &output.stdout,
    )?;
    compare_bytes(
        source,
        &format!("{process} stderr"),
        expected_stderr,
        &output.stderr,
    )
}

fn compare_bytes(
    source: &Path,
    stream: &str,
    expected: &[u8],
    actual: &[u8],
) -> datatest_stable::Result<()> {
    if expected == actual {
        return Ok(());
    }
    Err(error(format!(
        "{}: {stream} mismatch\nexpected: {}\n  actual: {}",
        source.display(),
        display_bytes(expected),
        display_bytes(actual)
    )))
}

fn display_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => format!("{text:?}"),
        Err(_) => format!("{bytes:?}"),
    }
}

fn assert_amd64_elf(executable: &Path) -> datatest_stable::Result<()> {
    let metadata = fs::metadata(executable)?;
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(error(format!(
            "{} has no executable permission bit",
            executable.display()
        )));
    }

    let binary = fs::read(executable)?;
    if binary.len() < 20
        || &binary[..4] != b"\x7fELF"
        || binary[4] != 2
        || binary[5] != 1
        || u16::from_le_bytes([binary[18], binary[19]]) != 62
    {
        return Err(error(format!(
            "{} is not a little-endian amd64 ELF64 executable",
            executable.display()
        )));
    }
    Ok(())
}

fn error(message: impl Into<String>) -> Box<dyn std::error::Error> {
    Box::new(io::Error::other(message.into()))
}

datatest_stable::harness! {
    { test = run_spec, root = "tests/spec/run", pattern = r".*\.kal$" },
    { test = compile_fail_spec, root = "tests/spec/compile-fail", pattern = r".*\.kal$" },
}
