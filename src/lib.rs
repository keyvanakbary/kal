mod codegen;
mod semantics;
mod syntax;

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::NamedTempFile;

pub fn build(input: &Path, output: &Path) -> Result<(), Error> {
    let source = fs::read_to_string(input)
        .map_err(|error| Error::new(format!("could not read {}: {error}", input.display())))?;
    let expression = syntax::parse(&source)?;
    let program = semantics::analyze(expression)?;
    let object = codegen::emit_object(&program)?;

    link(&object, output)
}

fn link(object: &[u8], output: &Path) -> Result<(), Error> {
    let output_parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    let mut object_file = NamedTempFile::new_in(output_parent).map_err(|error| {
        Error::new(format!(
            "could not create a temporary object file in {}: {error}",
            output_parent.display()
        ))
    })?;
    std::io::Write::write_all(&mut object_file, object)
        .map_err(|error| Error::new(format!("could not write object file: {error}")))?;

    let executable_file = NamedTempFile::new_in(output_parent).map_err(|error| {
        Error::new(format!(
            "could not create a temporary executable in {}: {error}",
            output_parent.display()
        ))
    })?;
    let executable_path = executable_file.path().to_owned();

    let linker = Command::new("cc")
        .arg(object_file.path())
        .arg("-o")
        .arg(&executable_path)
        .output()
        .map_err(|error| Error::new(format!("could not run the system linker: {error}")))?;

    if !linker.status.success() {
        return Err(Error::new(format!(
            "system linker failed with {}:\n{}",
            linker.status,
            String::from_utf8_lossy(&linker.stderr)
        )));
    }

    executable_file.persist(output).map_err(|error| {
        Error::new(format!(
            "could not move the linked executable to {}: {}",
            output.display(),
            error.error
        ))
    })?;

    Ok(())
}

#[derive(Debug)]
pub struct Error {
    message: String,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for Error {}

pub fn default_output_path(input: &Path) -> Result<PathBuf, Error> {
    let stem = input
        .file_stem()
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| Error::new(format!("{} has no file name", input.display())))?;
    Ok(PathBuf::from(stem))
}
