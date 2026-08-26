use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

#[test]
fn builds_a_native_hello_world_program() {
    let test_dir = TestDir::new("hello-world");
    let source_path = test_dir.path().join("program.kal");
    let executable_path = test_dir.path().join("program");

    fs::write(&source_path, hello_world_source()).expect("write Kal source file");

    let compiler = compile(&source_path, &executable_path);
    assert_success("compiler", &compiler);
    assert!(
        compiler.stdout.is_empty(),
        "compiler wrote unexpected stdout: {}",
        String::from_utf8_lossy(&compiler.stdout)
    );
    assert!(
        compiler.stderr.is_empty(),
        "compiler wrote unexpected stderr: {}",
        String::from_utf8_lossy(&compiler.stderr)
    );

    let metadata = fs::metadata(&executable_path).expect("compiler should create an executable");
    assert_ne!(
        metadata.permissions().mode() & 0o111,
        0,
        "compiler output should have an executable permission bit"
    );
    assert_amd64_elf(&fs::read(&executable_path).expect("read compiler output"));

    let program = Command::new(&executable_path)
        .output()
        .expect("run the compiled Kal program");

    assert_success("compiled program", &program);
    assert_eq!(program.stdout, b"Hello, world!");
    assert!(
        program.stderr.is_empty(),
        "compiled program wrote unexpected stderr: {}",
        String::from_utf8_lossy(&program.stderr)
    );
}

#[test]
fn reports_an_invalid_program_without_creating_an_executable() {
    let test_dir = TestDir::new("invalid-program");
    let source_path = test_dir.path().join("invalid.kal");
    let executable_path = test_dir.path().join("program");

    fs::write(
        &source_path,
        r#"(defn main ((args (Array String))) -> String
  "not a valid main")
"#,
    )
    .expect("write invalid Kal source file");

    let compiler = compile(&source_path, &executable_path);

    assert!(!compiler.status.success());
    assert!(compiler.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&compiler.stderr),
        "kal: main must return `Int`\n"
    );
    assert!(
        !executable_path.exists(),
        "failed compilation must not create an executable"
    );
}

fn hello_world_source() -> &'static str {
    r#"(defn main ((args (Array String))) -> Int
  (do
    (print "Hello, world!")
    0))
"#
}

fn compile(source_path: &std::path::Path, executable_path: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kal"))
        .args(["build"])
        .arg(source_path)
        .arg("-o")
        .arg(executable_path)
        .output()
        .expect("run the Kal compiler")
}

fn assert_amd64_elf(binary: &[u8]) {
    assert!(
        binary.len() >= 20,
        "compiler output is too short to be an ELF header"
    );
    assert_eq!(&binary[..4], b"\x7fELF", "compiler output should be ELF");
    assert_eq!(binary[4], 2, "compiler output should be a 64-bit ELF");
    assert_eq!(binary[5], 1, "compiler output should be little-endian");
    assert_eq!(
        u16::from_le_bytes([binary[18], binary[19]]),
        62,
        "compiler output should target amd64"
    );
}

fn assert_success(process: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{process} failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("kal-{name}-{}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).expect("remove stale test directory");
        }
        fs::create_dir(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
