use eyre::{eyre, Context, Result};
use relative_path::RelativePathBuf;
use std::{
    env,
    fmt::Display,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(feature = "deny-net-fetch")]
use eyre::bail;

// update this whenever you change the subtree pointer
const CAPNP_VERSION: &str = "2.0-fs";

enum CapstoneAcquired {
    Locally(PathBuf),
    OnSystem(PathBuf),
}

impl Display for CapstoneAcquired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CapstoneAcquired::Locally(e) => write!(f, "{}", e.display()),
            CapstoneAcquired::OnSystem(e) => write!(f, "{}", e.display()),
        }
    }
}

fn main() -> Result<()> {
    // we're making the assumption that the executable is always accessible.
    // if we can't make this assumption, we can just include_bytes!() it and then unpack it at runtime.

    println!("cargo:rerun-if-changed=capstone");

    let out_dir = PathBuf::from(
        env::var("OUT_DIR").context("Cargo did not set $OUT_DIR. this should be impossible.")?,
    );

    // updated with the final path of the capnp binary if it's ever found, to be recorded
    // and consumed by capnp_import!()
    let mut capnp_path: Option<CapstoneAcquired> = None;

    // only build if it can't be detected in the $PATH
    // check if there is a capnp binary in the path that meets the version requirement
    let existing_capnp: Result<PathBuf> = (|| {
        let bin = which::which("capnp").context("could not find a system capnp binary")?;
        let version = get_version(&bin).context(
            "could not obtain version of found binary, system capnp may be inaccessible",
        )?;

        println!("found capnp '{version}'");

        if version.trim() == format!("Cap'n Proto version {}", CAPNP_VERSION) {
            capnp_path = Some(CapstoneAcquired::OnSystem(bin.clone()));
            Ok(bin)
        } else {
            println!("cargo:warning=System version of capnp found ({}) does not meet version requirement {CAPNP_VERSION}.", &version);
            Err(eyre!(
                "version of system capnp does not meet version requirements"
            ))?
        }
    })();

    // no capstone here, proceed to build
    if let Err(e) = existing_capnp {
        #[cfg(feature = "deny-net-fetch")]
        bail!("Couldn't find a local capnp: {}\n refusing to build", e);

        println!("Couldn't find a local capnp: {}", e);
        println!("building...");

        let built_bin = build_with_cmake(&out_dir)?;

        capnp_path = Some(built_bin);
    }

    fs::write(
        out_dir.join("extract_bin.rs"),
        format!(
            "
#[allow(dead_code)]
fn commandhandle() -> eyre::Result<tempfile::TempDir> {{
    use std::io::Write;
    #[cfg(any(target_os = \"linux\", target_os = \"macos\"))]
    use std::os::unix::fs::OpenOptionsExt;
    use tempfile::tempdir;

    let file_contents = include_bytes!(r#\"{}\"#);

    let tempdir = tempdir()?;

    #[cfg(any(target_os = \"linux\", target_os = \"macos\"))]
    let mut handle = 
        std::fs::OpenOptions::new()
        .write(true)
        .mode(0o770)
        .create(true)
        .open(tempdir.path().join(\"capnp\"))?;

    #[cfg(target_os = \"windows\")]
    let mut handle = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(tempdir.path().join(\"capnp\"))?;

    #[cfg(not(any(target_os = \"linux\", target_os = \"macos\", target_os = \"windows\")))]
    compile_error!(\"capnp-import does not support your operating system!\");

    handle.write_all(file_contents)?;

    Ok(tempdir)
}}",
            capnp_path.unwrap(),
        ),
    )?;

    Ok(())
}

fn get_version(executable: &Path) -> Result<String> {
    let version = String::from_utf8(Command::new(executable).arg("--version").output()?.stdout)?;
    Ok(version)
}

// build capstone with cmake, configured for windows and linux envs
fn build_with_cmake(out_dir: &PathBuf) -> Result<CapstoneAcquired> {
    // is dst consistent? might need to write this down somewhere if it isn't
    let mut dst = cmake::Config::new("capstone");

    if which::which("ninja").is_ok() {
        dst.generator("Ninja");
    }

    // g++ doesn't work, so we try to use clang if available
    #[cfg(not(target_os = "windows"))]
    if which::which("clang++").is_ok() {
        dst.define("CMAKE_CXX_COMPILER", "clang++");
    }

    // it would be nice to be able to use mold

    #[cfg(target_os = "windows")]
    dst.cxxflag("/EHsc");

    let dst = dst.define("BUILD_TESTING", "OFF").build();

    assert_eq!(*out_dir, dst);

    // place the capstone binary in $OUT_DIR, next to where binary_decision.rs
    // is intended to go (it's still called capnp.exe for compatibility)
    if cfg!(target_os = "windows") {
        Ok(CapstoneAcquired::Locally(
            RelativePathBuf::from("bin/capnp.exe").to_path(out_dir),
        ))
    } else if cfg!(target_os = "linux") || cfg!(target_os = "macos") {
        Ok(CapstoneAcquired::Locally(
            RelativePathBuf::from("bin/capnp").to_path(out_dir),
        ))
    } else {
        panic!("Sorry, capnp-import does not support your operating system.");
    }
}
