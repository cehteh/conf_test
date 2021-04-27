# Rust conf_test

Run configuration tests from build.rs and set available features, similar to *autotools*
configure scripts.

# Description

`ConfTest::run()` called from 'build.rs' parses 'Cargo.toml'. Then for each *[feature]*
defined, it checks if that feature was not set manually (with `--features`) and a test in
'conf_tests/' exists. This test is then compiled and build. When that succeeds the
feature becomes enabled automatically.
