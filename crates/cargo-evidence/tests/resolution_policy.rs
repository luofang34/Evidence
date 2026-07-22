//! Integration tests for the locked/offline dependency-resolution
//! policy (LLR-139 / LLR-140 / LLR-142).
//!
//! Wire-level contracts, driven against tempdir workspaces:
//!
//! - cert/record + `--online` is refused before any bundle work
//!   (`POLICY_ONLINE_RESOLUTION_FORBIDDEN`).
//! - dev + `--online` succeeds and records `online_opt_in` in
//!   `index.json`.
//! - A locked/offline run whose cargo cache lacks the locked graph
//!   fails before evidence success with
//!   `BUNDLE_LOCKED_GRAPH_UNAVAILABLE` — the network is never a
//!   fallback.
//! - A locked/offline run against a fully prepared cache succeeds
//!   and records `locked_offline` (network-disabled proof: every
//!   cargo subprocess resolved from local data). This test's setup
//!   may use the network once (`cargo fetch --locked`); when the
//!   sandbox has none, the test skips gracefully — the negative
//!   test above is the always-runnable half.
//! - `verify` rejects a bundle pairing `online_opt_in` with a
//!   cert/record profile (`VERIFY_ONLINE_RESOLUTION`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should panic immediately"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

use evidence_core::bundle::EvidenceIndex;
use evidence_core::hash::{sha256_file, write_sha256sums};

fn cargo_evidence() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("cargo-evidence").unwrap()
}

/// A one-crate workspace with a boundary config. `registry_dep`
/// adds a dependency on `hex` (tiny, no transitive deps, present in
/// this repo's own lockfile), which forces full graph resolution to
/// consult the cargo cache. `forbid_build_rs` toggles the boundary
/// gate that runs the FULL `cargo metadata` (with graph resolution)
/// during generate.
fn write_tiny_workspace(dir: &Path, registry_dep: bool, forbid_build_rs: bool) {
    fs::create_dir_all(dir.join("src")).unwrap();
    let dep = if registry_dep { "hex = \"0.4\"\n" } else { "" };
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"tinyws\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{dep}"
        ),
    )
    .unwrap();
    fs::write(dir.join("src/lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();
    fs::create_dir_all(dir.join("cert")).unwrap();
    fs::write(
        dir.join("cert/boundary.toml"),
        format!(
            r#"
[schema]
version = "{ver}"

[scope]
in_scope = ["tinyws"]

[policy]
no_out_of_scope_deps = false
forbid_build_rs = {forbid_build_rs}
forbid_proc_macros = false
"#,
            ver = evidence_core::schema_versions::BOUNDARY
        ),
    )
    .unwrap();
}

/// The `dev-*` bundle directory produced under `out_dir`.
fn bundle_dir_under(out_dir: &Path) -> PathBuf {
    fs::read_dir(out_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .starts_with("dev-")
        })
        .expect("bundle directory under out_dir")
}

fn index_json(bundle: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(bundle.join("index.json")).unwrap()).unwrap()
}

/// TEST-156 (CLI arm): cert + `--online` is refused with the typed
/// code before any bundle work — the refusal needs no workspace,
/// boundary config, or git state.
#[test]
fn generate_cert_with_online_flag_is_refused() {
    let tmp = TempDir::new().unwrap();
    let out = TempDir::new().unwrap();
    let result = cargo_evidence()
        .arg("evidence")
        .arg("generate")
        .arg("--profile")
        .arg("cert")
        .arg("--online")
        .arg("--out-dir")
        .arg(out.path())
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(
        !result.status.success(),
        "cert + --online must fail: stdout={}",
        String::from_utf8_lossy(&result.stdout)
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("POLICY_ONLINE_RESOLUTION_FORBIDDEN"),
        "stderr must carry the typed refusal code:\n{stderr}"
    );
    assert!(
        !bundle_dir_under_optional(out.path()),
        "no bundle directory may be produced on refusal"
    );
}

fn bundle_dir_under_optional(out_dir: &Path) -> bool {
    fs::read_dir(out_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.path().is_dir())
}

/// TEST-159 (CLI arm): dev + `--online` is honored and recorded.
/// The tiny workspace has no registry dependencies, so this run
/// needs no network and no prepared cache: the opt-in only relaxes
/// the flags, nothing here exercises them.
#[test]
fn generate_dev_with_online_flag_records_online_opt_in() {
    let tmp = TempDir::new().unwrap();
    write_tiny_workspace(tmp.path(), false, false);
    let out = TempDir::new().unwrap();
    cargo_evidence()
        .arg("evidence")
        .arg("generate")
        .arg("--profile")
        .arg("dev")
        .arg("--online")
        .arg("--skip-tests")
        .arg("--out-dir")
        .arg(out.path())
        .current_dir(tmp.path())
        .assert()
        .success();
    let bundle = bundle_dir_under(out.path());
    assert_eq!(
        index_json(&bundle)["resolution_policy"],
        Value::String("online_opt_in".to_string()),
        "index.json must record the online opt-in"
    );
}

/// Run `cargo` in `dir` with the given CARGO_HOME (or the ambient
/// one). Returns the exit status so callers can skip gracefully
/// when setup needs a network the sandbox doesn't have.
fn cargo_setup(dir: &Path, args: &[&str], cargo_home: Option<&Path>) -> std::process::ExitStatus {
    let mut cmd = std::process::Command::new("cargo");
    cmd.args(args).current_dir(dir);
    if let Some(home) = cargo_home {
        cmd.env("CARGO_HOME", home);
    }
    cmd.status().expect("spawn cargo for test setup")
}

/// TEST-157 (negative arm): with `CARGO_HOME` pointed at an empty
/// directory, the locked/offline boundary-gate `cargo metadata`
/// cannot resolve the locked `hex` dependency. Generate must fail
/// before evidence success with the actionable
/// `BUNDLE_LOCKED_GRAPH_UNAVAILABLE` diagnostic — never by silently
/// reaching the network.
///
/// Setup runs `cargo generate-lockfile` with the ambient cargo home
/// (hex is a dependency of this repository, so a host that built it
/// has the index cached); when even that fails (no network, no
/// cache), the test skips — it cannot construct its own premise.
#[test]
fn missing_cache_locked_offline_fails_with_actionable_diagnostic() {
    let tmp = TempDir::new().unwrap();
    write_tiny_workspace(tmp.path(), true, true);
    if !cargo_setup(tmp.path(), &["generate-lockfile"], None).success() {
        eprintln!(
            "skipping: `cargo generate-lockfile` failed (no network and no ambient cache); \
             cannot construct the test premise"
        );
        return;
    }

    let empty_cargo_home = TempDir::new().unwrap();
    let out = TempDir::new().unwrap();

    // Premise probe: the fixture must actually be unable to resolve
    // offline with the isolated cache. A wrapped cargo (e.g. a Nix
    // sandbox whose vendor config makes every crate available
    // offline regardless of CARGO_HOME) defeats the premise; skip
    // there — the standard Check runners exercise the failure for
    // real. HOME is isolated too so a home-level vendor config
    // cannot leak in.
    let probe = std::process::Command::new("cargo")
        .arg("metadata")
        .arg("--locked")
        .arg("--offline")
        .arg("--format-version")
        .arg("1")
        .current_dir(tmp.path())
        .env("CARGO_HOME", empty_cargo_home.path())
        .env("HOME", empty_cargo_home.path())
        .output()
        .unwrap();
    if probe.status.success() {
        eprintln!(
            "skipping: this environment resolves crates offline regardless of \
             CARGO_HOME (wrapped/vendored cargo); the missing-cache premise \
             cannot be constructed here"
        );
        return;
    }

    let result = cargo_evidence()
        .arg("evidence")
        .arg("generate")
        .arg("--profile")
        .arg("dev")
        .arg("--skip-tests")
        .arg("--out-dir")
        .arg(out.path())
        .current_dir(tmp.path())
        .env("CARGO_HOME", empty_cargo_home.path())
        .env("HOME", empty_cargo_home.path())
        .output()
        .unwrap();
    assert!(
        !result.status.success(),
        "locked/offline generate with an empty cache must fail: stdout={}",
        String::from_utf8_lossy(&result.stdout)
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("BUNDLE_LOCKED_GRAPH_UNAVAILABLE"),
        "stderr must carry the locked-graph code:\n{stderr}"
    );
    assert!(
        stderr.contains("cargo fetch --locked"),
        "stderr must name the remediation:\n{stderr}"
    );
}

/// TEST-157 (positive arm): from a fully prepared locked cache,
/// generate succeeds with the network effectively disabled — every
/// cargo subprocess (`metadata` in the boundary gate, `--no-deps`
/// metadata in input scoping, `cargo build` in the output
/// inventory, and the projection's full metadata) resolves offline
/// under `--locked --offline` — and the bundle records
/// `locked_offline`.
///
/// Setup populates a throwaway CARGO_HOME with `cargo fetch
/// --locked`, which is the one step allowed to touch the network;
/// when the sandbox has none, the test skips gracefully. The
/// negative test above is the always-runnable proof that a locked
/// run cannot silently reach out.
#[test]
fn prepared_cache_offline_generate_succeeds_and_records_policy() {
    let tmp = TempDir::new().unwrap();
    write_tiny_workspace(tmp.path(), true, true);
    let prepared = TempDir::new().unwrap();
    // Lock first (ambient cache may serve this without network), then
    // fetch exactly what the lockfile pins into the throwaway
    // CARGO_HOME — the one step allowed to touch the network.
    if !cargo_setup(tmp.path(), &["generate-lockfile"], None).success()
        || !cargo_setup(tmp.path(), &["fetch", "--locked"], Some(prepared.path())).success()
    {
        eprintln!(
            "skipping: `cargo fetch --locked` failed (no network); the prepared-cache \
             premise cannot be constructed in this sandbox"
        );
        return;
    }

    let out = TempDir::new().unwrap();
    cargo_evidence()
        .arg("evidence")
        .arg("generate")
        .arg("--profile")
        .arg("dev")
        .arg("--skip-tests")
        .arg("--inventory-outputs")
        .arg("--out-dir")
        .arg(out.path())
        .current_dir(tmp.path())
        .env("CARGO_HOME", prepared.path())
        .assert()
        .success();

    let bundle = bundle_dir_under(out.path());
    assert_eq!(
        index_json(&bundle)["resolution_policy"],
        Value::String("locked_offline".to_string()),
        "index.json must record the locked/offline policy"
    );

    // The output inventory ran `cargo build --locked --offline`
    // against the prepared cache and attested real deliverables.
    let outputs: Value =
        serde_json::from_str(&fs::read_to_string(bundle.join("outputs_hashes.json")).unwrap())
            .unwrap();
    assert!(
        outputs.as_object().map(|m| !m.is_empty()).unwrap_or(false),
        "outputs_hashes.json must record built deliverables"
    );

    // The dev bundle carries the metadata projection because the
    // boundary policy claims forbid_build_rs; it binds the RESOLVED
    // dependency graph — `hex` appears with its resolved version.
    let metadata: Value =
        serde_json::from_str(&fs::read_to_string(bundle.join("cargo_metadata.json")).unwrap())
            .unwrap();
    let deps = metadata["dependencies"].as_object().unwrap();
    let flattened: Vec<&str> = deps
        .iter()
        .flat_map(|(k, vs)| {
            std::iter::once(k.as_str())
                .chain(vs.as_array().unwrap().iter().filter_map(|v| v.as_str()))
        })
        .collect();
    assert!(
        flattened.iter().any(|id| id.starts_with("hex ")),
        "resolved graph must bind hex with its resolved version: {flattened:?}"
    );
    assert!(
        flattened.iter().any(|id| id.starts_with("tinyws ")),
        "resolved graph must bind the workspace crate itself: {flattened:?}"
    );
}

/// TEST-159 (verify arm): a bundle pairing `resolution_policy =
/// "online_opt_in"` with a cert profile is rejected by
/// `cargo evidence verify` with `VERIFY_ONLINE_RESOLUTION`, even
/// though the generator gate that would have refused it was never
/// involved (hand-assembled bundle).
#[test]
fn verify_rejects_online_opt_in_cert_bundle() {
    let (_tmp, bundle) = build_online_cert_bundle();
    let out = cargo_evidence()
        .args(["evidence", "verify", "--format=jsonl"])
        .arg(&bundle)
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "verify must reject an online-resolution cert bundle"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let codes: Vec<String> = stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| v["code"].as_str().map(str::to_string))
        .collect();
    assert!(
        codes.iter().any(|c| c == "VERIFY_ONLINE_RESOLUTION"),
        "expected a VERIFY_ONLINE_RESOLUTION diagnostic; codes: {codes:?}"
    );
}

/// Hand-assemble a minimal, hash-consistent cert-profile bundle whose
/// `index.json` records `resolution_policy = online_opt_in` — the
/// state the generate-time gate refuses to produce.
fn build_online_cert_bundle() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let bundle_dir = tmp.path().join("cert-20260207-000000Z-aabbccdd");
    fs::create_dir_all(&bundle_dir).unwrap();

    let profile = evidence_core::Profile::Cert;
    let env_fp = evidence_core::EnvFingerprint {
        profile,
        rustc: "rustc 1.85.0".to_string(),
        cargo: "cargo 1.85.0".to_string(),
        git_sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
        git_branch: "main".to_string(),
        git_dirty: false,
        in_nix_shell: false,
        tools: BTreeMap::new(),
        nav_env: BTreeMap::new(),
        llvm_version: None,
        host: evidence_core::Host::Linux {
            arch: "x86_64".to_string(),
            libc: None,
            kernel: None,
        },
        cargo_lock_hash: None,
        rust_toolchain_toml: None,
        rustflags: None,
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        tool_prerelease: false,
    };
    fs::write(
        bundle_dir.join("env.json"),
        serde_json::to_vec_pretty(&env_fp).unwrap(),
    )
    .unwrap();
    // Non-empty source baseline: verify fails closed on empty inputs.
    let mut inputs_map: BTreeMap<String, String> = BTreeMap::new();
    inputs_map.insert("Cargo.toml".to_string(), "0".repeat(64));
    fs::write(
        bundle_dir.join("inputs_hashes.json"),
        serde_json::to_vec_pretty(&inputs_map).unwrap(),
    )
    .unwrap();
    let outputs_map: BTreeMap<String, String> = BTreeMap::new();
    fs::write(
        bundle_dir.join("outputs_hashes.json"),
        serde_json::to_vec_pretty(&outputs_map).unwrap(),
    )
    .unwrap();
    let empty_cmds: Vec<Value> = vec![];
    fs::write(
        bundle_dir.join("commands.json"),
        serde_json::to_vec_pretty(&empty_cmds).unwrap(),
    )
    .unwrap();

    // The recipe manifest aggregates the recorded inputs and the
    // resolution policy, so it is written after inputs_hashes.json /
    // commands.json and must carry the same policy the index records.
    let recipe_inputs = evidence_core::env::RecipeInputs::from_bundle_dir(
        &bundle_dir,
        evidence_core::policy::ResolutionPolicy::OnlineOptIn,
    )
    .expect("gather recipe inputs");
    let manifest = env_fp.recipe_manifest(&recipe_inputs);
    fs::write(
        bundle_dir.join("deterministic-manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let sha256sums_path = bundle_dir.join("SHA256SUMS");
    write_sha256sums(&bundle_dir, &sha256sums_path).unwrap();
    let content_hash = sha256_file(&sha256sums_path).unwrap();
    let recipe_hash = sha256_file(&bundle_dir.join("deterministic-manifest.json")).unwrap();

    let index = EvidenceIndex {
        schema_version: evidence_core::schema_versions::INDEX.to_string(),
        boundary_schema_version: evidence_core::schema_versions::BOUNDARY.to_string(),
        trace_schema_version: evidence_core::schema_versions::TRACE.to_string(),
        profile,
        timestamp_rfc3339: "2026-02-07T00:00:00Z".to_string(),
        git_sha: "aabbccdd11223344aabbccdd11223344aabbccdd".to_string(),
        git_branch: "main".to_string(),
        git_dirty: false,
        engine_crate_version: "0.1.0".to_string(),
        engine_git_sha: "eeff001122334455667788990011223344556677".to_string(),
        engine_build_source: "git".to_string(),
        inputs_hashes_file: "inputs_hashes.json".to_string(),
        outputs_hashes_file: "outputs_hashes.json".to_string(),
        commands_file: "commands.json".to_string(),
        env_fingerprint_file: "env.json".to_string(),
        trace_roots: vec![],
        trace_outputs: vec![],
        bundle_complete: true,
        content_hash,
        recipe_hash,
        test_summary: None,
        tool_command_failures: Vec::new(),
        dal_map: BTreeMap::new(),
        boundary_policy: evidence_core::BoundaryPolicy::default(),
        resolution_policy: evidence_core::policy::ResolutionPolicy::OnlineOptIn,
    };
    fs::write(
        bundle_dir.join("index.json"),
        serde_json::to_vec_pretty(&index).unwrap(),
    )
    .unwrap();

    (tmp, bundle_dir)
}
