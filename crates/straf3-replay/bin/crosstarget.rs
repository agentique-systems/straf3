//! Print the `.s3d` cross-target report for whatever target this was built
//! for, and exit non-zero if any case failed its own on-target assertions.
//!
//! Nothing is compared here. Comparison is `crosstarget/verify.sh`'s job,
//! across the four reports — same division of labour as `straf3-det-runner`,
//! and for the same reason: a binary that judged its own output would have to
//! be trusted, and four texts that have to be identical do not.

fn main() -> std::process::ExitCode {
    let platform = if cfg!(target_os = "windows") {
        "native-x86_64-windows"
    } else if cfg!(target_env = "musl") {
        "native-x86_64-linux-musl"
    } else {
        "native-x86_64-linux-gnu"
    };

    print!("{}", straf3_replay::crosstarget::render(platform));

    if straf3_replay::crosstarget::all_ok() {
        std::process::ExitCode::SUCCESS
    } else {
        eprintln!(
            "at least one case failed on {}: see the round_trips / verifies / \
             refuses_stale columns",
            straf3_replay::crosstarget::TARGET
        );
        std::process::ExitCode::FAILURE
    }
}
