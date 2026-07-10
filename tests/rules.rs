// Rules: End-to-end tests freezing the architectural dep-rule contract in
// --strict-check — a `deny` rule over a real forbidden edge and a `forbid_cycles` rule
// over a real dependency cycle each surface as a report line with exit 1, and do not
// disturb annotation linting. Runs over dedicated fixtures, never `sample/`. This is
// the e2e proof the config `[rules]` flags reach the report, complementing the pure
// algorithm tests in `src/rules.rs`. | I/O: (rules fixtures) -> asserted (stdout, code)

use std::path::PathBuf;

use annotated_tree::Cli;
use clap::Parser;

fn run_strict_check(fixture: &str) -> (String, i32) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let argv = [
        "annotated-tree".to_string(),
        "--strict-check".to_string(),
        dir.to_string_lossy().into_owned(),
    ];
    let cli = Cli::parse_from(argv);
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = annotated_tree::run(&cli, &mut out, &mut err).expect("run failed");
    (String::from_utf8(out).unwrap(), code)
}

#[test]
fn deny_rule_over_a_forbidden_edge_fails_strict_check() {
    // The fixture's web -> core Cargo path dependency violates `deny = [["web","core"]]`.
    let (out, code) = run_strict_check("rules");

    assert_eq!(code, 1, "a forbidden edge must fail --strict-check:\n{out}");
    assert!(
        out.contains("rule: denied dependency: web must not depend on core"),
        "the report must name the violated deny rule:\n{out}"
    );
    // Annotations in the fixture are all valid, so the failure is the rule alone: none
    // of the annotation-linter's prose (`annotation::validate`) may leak into the report.
    assert!(
        !out.contains("missing annotation") && !out.contains("annotation missing required"),
        "no annotation errors expected, only the rule finding:\n{out}"
    );
}

#[test]
fn forbid_cycles_over_a_real_cycle_fails_strict_check() {
    // The fixture's alpha <-> beta Cargo path dependencies form a cycle, which
    // `[rules] forbid_cycles = true` flags. This freezes that the config flag actually
    // reaches the strict report (not just the pure algorithm in src/rules.rs).
    let (out, code) = run_strict_check("cycle");

    assert_eq!(
        code, 1,
        "a dependency cycle must fail --strict-check:\n{out}"
    );
    assert!(
        out.contains("rule: dependency cycle:"),
        "the report must name the cycle violation:\n{out}"
    );
    assert!(
        out.contains("alpha") && out.contains("beta"),
        "the cycle finding must name both packages in the loop:\n{out}"
    );
    // The fixture's annotations are all valid, so the failure is the rule alone: none
    // of the annotation-linter's prose (`annotation::validate`) may leak into the report.
    assert!(
        !out.contains("missing annotation") && !out.contains("annotation missing required"),
        "no annotation errors expected, only the rule finding:\n{out}"
    );
}
