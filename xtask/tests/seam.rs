//! The seam rule as a test, so `cargo test --workspace` fails on a violation
//! even if nobody remembers to run `cargo xtask check-seam`.

#[test]
fn nothing_below_the_line_depends_on_anything_above_it() {
    let report = match xtask::seam::check() {
        Ok(report) => report,
        Err(e) => panic!(
            "could not read the dependency graph: {e}\n\
             (this is a resolution failure, not a seam violation)"
        ),
    };
    assert!(
        report.is_clean(),
        "the straf3 dependency seam has been broken:\n{}",
        report.render()
    );
}
