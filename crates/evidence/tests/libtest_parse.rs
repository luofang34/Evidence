//! Regression test for `parse_cargo_test_output_with_outcomes` against
//! a committed real-world `cargo test --workspace` stdout capture.
//!
//! If libtest ever changes its stable stdout format (e.g. a future
//! Rust release renames `FAILED` to `failed` or reshapes the `Running`
//! header), this test fires and `cargo evidence check` is prevented
//! from silently miscounting.
//!
//! The fixture at `tests/fixtures/libtest_output_sample.txt` is
//! hand-crafted to cover:
//!
//! - Unit test lines with module paths (`module::fn_name`).
//! - Integration test lines with bare fn names (no module path).
//! - `ok` / `FAILED` / `ignored` outcomes.
//! - A deliberately ambiguous test name (`shared_fn_name`) appearing
//!   in two binaries, to pin the binary-prefix disambiguation rule.
//! - Multiple `Running` header shapes: `unittests src/lib.rs` and
//!   `tests/foo.rs`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use evidence::bundle::{TestOutcome, parse_cargo_test_output_with_outcomes};

const FIXTURE: &str = include_str!("fixtures/libtest_output_sample.txt");

#[test]
fn parses_summary_counters_across_crates() {
    let (summary, _outcomes) =
        parse_cargo_test_output_with_outcomes(FIXTURE).expect("fixture parses");

    // Four `test result:` lines in the fixture:
    //   evidence unit: FAILED. 3 passed; 1 failed; 0 ignored
    //   cargo-evidence unit: ok. 1 passed; 0 failed; 1 ignored
    //   verify_jsonl integ: ok. 3 passed; 0 failed; 0 ignored
    //   trace_sys_layer integ: ok. 2 passed; 0 failed; 0 ignored
    //   doc-tests: ok. 0 passed; 0 failed; 1 ignored
    // Totals: 9 passed, 1 failed, 2 ignored.
    assert_eq!(summary.passed, 9);
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.ignored, 2);
    assert_eq!(summary.filtered_out, 0);
    assert_eq!(summary.total, 12);
}

#[test]
fn outcome_map_keys_include_binary_prefix() {
    let (_summary, outcomes) =
        parse_cargo_test_output_with_outcomes(FIXTURE).expect("fixture parses");

    // Unit test in the evidence library — keyed as
    // `<binary>::<module_path>::<fn>` where binary is `evidence`.
    assert_eq!(
        outcomes
            .get("evidence::diagnostic::tests::severity_rejects_unknown_variant")
            .copied(),
        Some(TestOutcome::Passed),
    );
    assert_eq!(
        outcomes
            .get("evidence::trace::entries::tests::test_hlr_entry_fields")
            .copied(),
        Some(TestOutcome::Failed),
    );

    // Unit test in the cargo-evidence binary — note the `cargo_evidence`
    // binary name (cargo replaces `-` with `_`).
    assert_eq!(
        outcomes
            .get("cargo_evidence::cli::args::tests::scalar_value_parser_sanity")
            .copied(),
        Some(TestOutcome::Passed),
    );

    // Integration test with a bare fn name — binary name takes the
    // place of the module path.
    assert_eq!(
        outcomes
            .get("verify_jsonl::verify_ok_terminates_with_verify_ok_and_exit_zero")
            .copied(),
        Some(TestOutcome::Passed),
    );
    assert_eq!(
        outcomes
            .get("trace_sys_layer::sys_hlr_llr_test_chain_validates")
            .copied(),
        Some(TestOutcome::Passed),
    );
}

#[test]
fn ignored_tests_are_represented_in_the_map() {
    let (_summary, outcomes) =
        parse_cargo_test_output_with_outcomes(FIXTURE).expect("fixture parses");

    assert_eq!(
        outcomes
            .get("cargo_evidence::cli::output::tests::shared_fn_name")
            .copied(),
        Some(TestOutcome::Ignored),
    );
}

#[test]
fn same_fn_name_in_two_binaries_yields_two_distinct_keys() {
    // The fixture deliberately has `shared_fn_name` as both an
    // ignored unit test in cargo_evidence and a passed integration
    // test in verify_jsonl. The binary-prefix rule must distinguish
    // them so a downstream consumer (PR #46's check) can choose to
    // flag ambiguity on the unqualified `shared_fn_name` selector
    // rather than silently pick one.
    let (_summary, outcomes) =
        parse_cargo_test_output_with_outcomes(FIXTURE).expect("fixture parses");

    let a = outcomes.get("cargo_evidence::cli::output::tests::shared_fn_name");
    let b = outcomes.get("verify_jsonl::shared_fn_name");

    assert!(
        a.is_some(),
        "unit-test key missing: {:?}",
        outcomes.keys().collect::<Vec<_>>()
    );
    assert!(b.is_some(), "integration-test key missing");
    assert_eq!(a.copied(), Some(TestOutcome::Ignored));
    assert_eq!(b.copied(), Some(TestOutcome::Passed));
}

#[test]
fn parse_returns_none_on_empty_or_non_test_input() {
    assert!(parse_cargo_test_output_with_outcomes("").is_none());
    assert!(parse_cargo_test_output_with_outcomes("hello world").is_none());
}

/// Real-world regression: `Command::output()` captures stdout and
/// stderr as separate buffers; a caller that concatenates
/// stdout-then-stderr pushes every `Running target/debug/deps/<binary>`
/// header (stderr) to the *end* of the merged stream. A single-pass
/// parser would never see a binary name before the test lines it
/// describes and would key every test under `__unknown_binary__::…` —
/// which silently breaks `cargo evidence check` selector matching.
///
/// This test synthesizes that exact shape by splitting the fixture
/// into the two streams and concatenating stdout-first, asserting the
/// parser still attributes each test to the correct binary.
#[test]
fn parser_is_robust_to_stdout_then_stderr_concatenation() {
    let (stderr_lines, stdout_lines): (Vec<&str>, Vec<&str>) = FIXTURE
        .lines()
        .partition(|line| line.trim_start().starts_with("Running "));
    let stdout_first = format!(
        "{}\n{}\n",
        stdout_lines.join("\n"),
        stderr_lines.join("\n")
    );
    let (summary, outcomes) = parse_cargo_test_output_with_outcomes(&stdout_first)
        .expect("stdout-then-stderr concat should still parse");

    // Counters are unaffected by ordering.
    assert_eq!(summary.passed, 9);
    assert_eq!(summary.failed, 1);

    // Binary attribution must still work — the key regression.
    assert_eq!(
        outcomes
            .get("verify_jsonl::verify_ok_terminates_with_verify_ok_and_exit_zero")
            .copied(),
        Some(TestOutcome::Passed),
    );
    assert_eq!(
        outcomes
            .get("trace_sys_layer::sys_hlr_llr_test_chain_validates")
            .copied(),
        Some(TestOutcome::Passed),
    );
    assert_eq!(
        outcomes
            .get("evidence::trace::entries::tests::test_hlr_entry_fields")
            .copied(),
        Some(TestOutcome::Failed),
    );
}
