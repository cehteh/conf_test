//! Run configuration tests from build.rs and set available features, similar to *autotools*
//! configure scripts.
//!
//!
//! # Description
//!
//! `ConfTest::run()` called from 'build.rs' parses 'Cargo.toml'. Then for each *[feature]*
//! defined, it checks if that feature was not set manually (with `--features`) and a test in
//! 'conf_tests/' exists. This test is then compiled and build. When that succeeds the
//! feature becomes enabled automatically.
//!
//! ## Special case for 'docs.rs'
//!
//! When a packages is build for documentation on 'docs.rs' then conf_test detects and checks
//! if a `docs_rs = []` features is available in 'Cargo.toml', if so, then this becomes
//! enabled. When not, then nothing is probed.
//!
//!
//! # Rationale
//!
//! Compiler versions and Operating Systems have sometimes subtle differences in features and
//! standards conformance. Sometimes non-standard features are added for improved
//! performance. These differences make it hard to write portable software that still offer
//! optimal performance for different Operating Systems. Moreover often a developer doesn't
//! even know what featureset other Operating Systems may provide or this may be changed by
//! kernel or userland version or configuration. Probing the presence of such features at
//! build time can solve these problems.
//!
//! Further it becomes possible to test for rust stdlib functionality such as if nightly
//! features are available or became stabilized.
//!
//!
//! # How To
//!
//! ## Checking OS features
//!
//! When present 'cargo' (builds and) executes a *build.rs* script while building crate. This
//! is the place where *ConfTest* is hooked in at first:
//!
//! ```rust,ignore
//! fn main() {
//!     conf_test::ConfTest::run();
//!     // any other build.rs steps follow below
//! }
//! ```
//!
//! Further one has to define a set of features and dependencies in the 'Cargo.toml'.  Note
//! that 'build.rs' will be run before the '[dependencies]' are build. Thus all dependencies
//! needed for the tests must go into '[build-dependencies]' as well. For Example:
//!
//! ```toml
//! [dependencies]
//! libc = "0.2.34"
//!
//! [build-dependencies]
//! libc = "0.2.34"
//! conf_test = "0.4"
//!
//! [features]
//! default = []
//! o_path = []
//! ```
//!
//! And as final step the crate directory 'conf_tests/' need to be created which contain rust
//! files named after the features to be probed. Containing a single `fn main()` which shall
//! probe one single thing.
//!
//! ```rust,ignore
//! // This goes into conf_tests/o_path.rs
//! extern crate libc;
//!
//! fn main() {
//!     unsafe {
//!         let conf_tests = std::ffi::CString::new("conf_tests").unwrap();
//!         // Compilation will fail when the libc does not define O_PATH
//!         libc::open(conf_tests.as_ptr(), libc::O_PATH);
//!     }
//! }
//! ```
//!
//! Later in the crate implementation source code one uses conditional compilation as usual
//! with `#[cfg(feature = "o_path")]`.
//!
//! ## Test depending on other Features
//!
//! Tests may depend on features that are discovered by other tests or set manually. For
//! simplicity there is no dependency resolver about this but tests are run in sort order of
//! the feature name. Every subsequent test is compiled with the the feature flags already
//! discovered so far. To leverage this functionality one rarely needs to change the feature
//! names. For example when 'bar' depends on 'foo' it is required to enforce the sort order by
//! renaming these features to 'aa_foo' and 'bb_bar'. Only features that get discovered are
//! used for the test compilations features set by printing cargo instructions from the test
//! scripts are not used.
//!
//!
//! # Detailed Control
//!
//! Tests can emit special instructions to cargo on stdout.
//! These become only effective when the test exits successful.
//! See https://doc.rust-lang.org/cargo/reference/build-scripts.html#outputs-of-the-build-script
//!
//! One can control ConfTest by setting the environment variable `CONF_TEST_INHIBIT` to one of
//! the following:
//! * **skip**
//!   Will not execute any conf_tests but proceed with 'build.rs'.
//! * **stop**
//!   Exits 'build.rs' sucessfully, not executing any tests.
//! * **fail**
//!   Exits 'build.rs' with an failure, not executing any tests.
//!
//! Any other value will make the script panic.
//!
//!
//! # Limitations
//!
//! * The tests running on the machine where the software is build, using the
//!   build-dependencies. This will be a problem when Software gets cross-compiled. For cross
//!   compilation set 'CONF_TEST_INHIBIT=skip' and set the desired features manually with the
//!   '--features' option.
//!
//! * Features can only be set, not unset. This is deliberate and not a limitation. Do only
//!   positive tests checking for the presence of a feature.
//!
//!
//! # Good Practices
//!
//! * Only use ConfTest when other things (like factoring out OS specific thing into their own
//!   crates) are not applicable.
//!
//! * Provide a baseline implementation which is portable with no features enabled. This may
//!   not perform as well or lack some special features but should compile nevertheless.
//!

use std::ffi::{OsStr, OsString};
use std::fs::{DirBuilder, File};

use std::io::prelude::*;

use std::env::var_os as env;
use std::path::{Path, PathBuf};
use std::str;

use cargo_metadata::{Edition, Message, MetadataCommand};
use std::process::{Command, Stdio};

use std::collections::{BTreeMap, BTreeSet};

// Empty Type for now, In future this may be extended without breaking existing code.
/// Implements the conf_test API
pub enum ConfTest {}

impl ConfTest {
    /// Run the configuration tests in 'conf_tests/'.
    #[allow(dead_code)]
    pub fn run() {
        if let Some(inhibit) = env("CONF_TEST_INHIBIT") {
            if inhibit == "skip" {
                println!("cargo:warning=Skipping ConfTest via CONF_TEST_INHIBIT");
                return;
            } else if inhibit == "stop" {
                std::process::exit(0);
            } else if inhibit == "fail" {
                println!("cargo:warning=Requested ConfTest failure via CONF_TEST_INHIBIT");
                std::process::exit(1)
            } else {
                // Bail on any unknown value to catch 'undefined' states/typos
                panic!("Unknown CONF_TEST_INHIBIT value: {:?}", inhibit)
            }
        }

        let mut outputs = Vec::new();

        outputs.push(format!(
            "# OUT_DIR is '{:?}'\n",
            env("OUT_DIR").expect("env var OUT_DIR is not set")
        ));

        // make our output dir
        let mut out_dir = PathBuf::new();
        out_dir.push(env("OUT_DIR").unwrap());
        out_dir.push("conf_test");
        DirBuilder::new()
            .recursive(true)
            .create(out_dir)
            .expect("Failed to create output directory");

        let mut logfile = PathBuf::new();
        logfile.push(env("OUT_DIR").unwrap());
        logfile.push("conf_test");
        logfile.push("conf_test.log");
        let mut logfile = File::create(logfile).expect("Failed to create logfile");

        let metadata = MetadataCommand::new()
            .other_options(["--frozen".to_string()])
            .no_deps()
            .exec()
            .expect("Querying cargo metadata failed");

        let mut features = BTreeSet::new();
        let mut dependencies = BTreeSet::new();
        let mut edition: Option<Edition> = None;
        for package in metadata.packages {
            if edition == None {
                // just pick the first edition seen
                edition = Some(package.edition);
            }
            for (feature, _) in package.features {
                features.insert(feature);
            }
            for dep in package.dependencies {
                dependencies.insert(dep.name);
            }
        }

        if env("DOCS_RS").is_some() {
            outputs.push("# running on DOCS.RS\n".to_string());
            if features.contains("docs_rs") {
                outputs.push("cargo:rustc-cfg=feature=\"docs_rs\"\n".to_string());
            }
        } else {
            let edition = edition.unwrap_or_else(|| Edition::E2021);

            let mut lockfile = PathBuf::new();
            lockfile
                .push(env("CARGO_MANIFEST_DIR").expect("env var CARGO_MANIFEST_DIR is not set"));
            lockfile.push("Cargo.lock");
            let lockfile_exists = lockfile.exists();

            outputs.push(format!(
                "# Lockfile '{:?}' present: {}\n",
                lockfile, lockfile_exists
            ));

            let extern_libs = Self::get_extern_libs(&dependencies);

            if !lockfile_exists {
                outputs.push(format!(
                    "# Delete Lockfile: '{:?}', {}\n",
                    &lockfile,
                    std::fs::remove_file(&lockfile).is_ok()
                ));
            }

            let mut test_features = Vec::new();

            for feature in features {
                if env(format!("CARGO_FEATURE_{}", feature.to_uppercase())).is_none() {
                    outputs.push(format!("# checking for {}\n", &feature));
                    let mut test_src = PathBuf::from("conf_tests");
                    test_src.push(&feature);
                    test_src.set_extension("rs");
                    if test_src.exists() {
                        outputs.push(format!("# {} exists\n", test_src.display()));
                        outputs.push(format!("cargo:rerun-if-changed={}\n", test_src.display()));
                        if let Some(binary) =
                            Self::compile_test(&test_src, &edition, &extern_libs, &test_features)
                        {
                            outputs
                                .push(format!("# compiling ConfTest for {} success\n", &feature));
                            if let Some(stdout) = Self::run_test(&binary) {
                                outputs.push(format!(
                                    "# executing ConfTest for {} success\n",
                                    &feature
                                ));
                                outputs.push(format!("cargo:rustc-cfg=feature=\"{}\"\n", &feature));
                                outputs.push(stdout);
                                test_features.push(feature.clone());
                            } else {
                                outputs.push(format!(
                                    "# executing ConfTest for {} failed\n",
                                    &feature
                                ));
                            }
                        } else {
                            outputs.push(format!("# compiling ConfTest for {} failed\n", &feature));
                        }
                    } else {
                        outputs.push(format!("# test for '{}' does not exist\n", &feature));
                    }
                } else {
                    outputs.push(format!("# test for '{}' manually overridden\n", &feature));
                }
                outputs.push(String::from("\n"));
                test_features.push(feature.clone());
            }
        }

        for output in outputs {
            logfile.write_all(output.as_bytes()).unwrap();
            print!("{}", output);
        }
    }

    fn run_test(test_binary: &Path) -> Option<String> {
        let command = Command::new(test_binary).output().ok()?;
        if command.status.success() {
            Some(String::from_utf8_lossy(&command.stdout).to_string())
        } else {
            None
        }
    }

    fn compile_test(
        src: &Path,
        edition: &Edition,
        extern_libs: &BTreeMap<OsString, (String, PathBuf)>,
        features: &[String],
    ) -> Option<PathBuf> {
        let mut out_file = PathBuf::new();
        out_file.push(env("OUT_DIR").expect("env var OUT_DIR is not set"));
        out_file.push("conf_test");
        out_file.push(src.file_stem().unwrap());

        let mut rust_cmd = Command::new(env("RUSTC").unwrap_or_else(|| OsString::from("rustc")));
        let rust_cmd = rust_cmd
            .arg("--crate-type")
            .arg("bin")
            .arg("--edition")
            .arg(edition_to_str(edition))
            .arg("-o")
            .arg(&out_file)
            .arg("-v")
            .arg(src);

        for (name, filename) in extern_libs.values() {
            rust_cmd.arg("--extern").arg(format!(
                "{}={}", //FIXME: needs some better way to compose an OsString here
                name,
                filename.to_str().expect("invalid file name")
            ));
        }

        for feature in features {
            rust_cmd
                .arg("--cfg")
                .arg(format!("feature=\"{}\"", feature));
        }

        let rust_output = rust_cmd.output().ok()?;

        if rust_output.status.success() {
            Some(out_file)
        } else {
            None
        }
    }

    fn get_extern_libs(dependencies: &BTreeSet<String>) -> BTreeMap<OsString, (String, PathBuf)> {
        let mut extern_libs = BTreeMap::new();

        //PLANNED: get rid of extra target dir, is there any way to work around the build lock?
        let mut target_dir = PathBuf::new();
        target_dir.push(env("OUT_DIR").expect("env var OUT_DIR is not set"));
        target_dir.push("conf_test");

        // let cargo start a rustc process that does not build the project but returns the
        // metadata about compilation artifacts
        let mut cargo = Command::new(env("CARGO").unwrap_or_else(|| OsString::from("cargo")))
            .arg("--offline")
            .arg("rustc")
            .arg("--message-format")
            .arg("json")
            .arg("--target-dir")
            .arg(target_dir)
            .arg("--")
            .arg("--emit")
            .arg("metadata")
            .env("CONF_TEST_INHIBIT", "stop")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        let reader = std::io::BufReader::new(cargo.stdout.take().unwrap());

        for message in cargo_metadata::Message::parse_stream(reader) {
            if let Message::CompilerArtifact(artifact) = message.unwrap() {
                if dependencies.contains(&artifact.target.name) {
                    for filename in artifact.filenames {
                        let filename = PathBuf::from(filename);
                        let id = OsString::from(filename.file_stem().expect("invalid file name"));
                        let extension = filename.extension();
                        let name = String::from(&artifact.target.name);

                        match extension.and_then(OsStr::to_str) {
                            Some("rlib") => {
                                extern_libs.insert(id, (name, filename));
                            }
                            Some("rmeta") => {
                                if extern_libs.contains_key(&id) {
                                    let stored_extension = extern_libs[&id]
                                        .1
                                        .extension()
                                        .and_then(OsStr::to_str)
                                        .unwrap();
                                    if stored_extension == "rlib" {
                                        continue;
                                    }
                                    extern_libs.insert(id, (name, filename));
                                }
                            }
                            Some(_other) => {
                                if extern_libs.contains_key(&id) {
                                    let stored_extension = extern_libs[&id]
                                        .1
                                        .extension()
                                        .and_then(OsStr::to_str)
                                        .unwrap();
                                    if stored_extension == "rmeta" || stored_extension == "rlib" {
                                        continue;
                                    }
                                    extern_libs.insert(id, (name, filename));
                                }
                            }
                            None => {
                                panic!("extension is not utf8 {:?}", extension);
                            }
                        }
                    }
                }
            }
        }

        cargo.wait().expect("Couldn't get cargo's exit status");

        extern_libs
    }
}

fn edition_to_str(edition: &Edition) -> &str {
    match edition {
        Edition::E2015 => "2015",
        Edition::E2018 => "2018",
        Edition::E2021 => "2021",
        _ => todo!("send PR for new editions"),
    }
}
