//! `cargo xtask check-evidence`: published numbers that stop reproducing.
//!
//! # The failure this exists to stop
//!
//! `probes/coil-course/results/coil.txt` published a state checksum on
//! 2026-08-16 and told a reader to run the shipped binary and see it. A commit
//! the next day added four fields to the folded simulation state. Every
//! document quoting that checksum became wrong on the spot, the trajectory did
//! not move by a millimetre, and nothing in the tree noticed for nine days —
//! not through inattention, but because no command re-derived a published
//! number and compared it. `README.md` already states the doctrine: *a number
//! pasted into prose is a claim with an expiry date nobody can see.* This is
//! the machine that enforces it.
//!
//! # Three categories, and why two of them are not "old" and "current"
//!
//! **Executable pin.** The number lives in code beside the machine that
//! compares it — `COIL_RUN_MS` in `crates/straf3-game/tests/windowed_playback.rs`,
//! `0x4350ccc31bec5d4c` in `crates/straf3-replay/src/identity.rs`. These cannot
//! expire silently: `cargo test` goes red. **This gate does not cover them, and
//! does not need to.** They are the destination — every live claim that can be
//! turned into an executable pin should be.
//!
//! **Live claim.** Prose asserting something about the tree as it stands, which
//! a reader is invited to reproduce. `PLAYING.md`'s "On the release build of
//! this tree:" over a checksum. This is the gate's scope, and it is checked for
//! its *value*.
//!
//! **Dated snapshot.** A record of what a specific build printed on a specific
//! day, whose worth is precisely that nobody touched it afterwards. It is
//! checked for its *provenance*.
//!
//! The test that separates the last two, sharp enough to apply without a
//! judgement call:
//!
//! > A number is a **live claim** if a reader who ran the command today and got
//! > a different number would be entitled to file a bug. It is a **dated
//! > snapshot** if they would not.
//!
//! `docs/web/ARCHITECTURE.md`'s glibc/musl divergence table is why these are
//! not "current" and "old": a reader who *could* reproduce those two numbers
//! today would file a bug, because they record a divergence that has since been
//! fixed. **Some snapshots must never reproduce again.** A gate that treated
//! reproduction as universally good would be wrong about them in the dangerous
//! direction.
//!
//! # The category is declared, never detected
//!
//! Nothing in the text of a number distinguishes a live claim from a snapshot.
//! `0x9a854d1a3653d8b7` appears in both `PLAYING.md` and `coil.txt` and is a
//! live claim in one and a dated snapshot in the other — identical text,
//! opposite fates. Any inference from a date-like header, from a file path, or
//! from living under `probes/` would be wrong, and wrong *silently*, which is
//! the failure being removed. So the category is part of the marker:
//!
//! ```text
//! straf3:claim kind=coil-replay-checksum          live: must reproduce
//! straf3:snapshot taken=2026-08-16 build=59cfd8f  dated: must carry provenance
//! straf3:snapshot taken=... build=... scope=eof   ...and every site below it
//! ```
//!
//! A marker governs the next value-bearing line. `scope=eof` covers every
//! remaining site in the file, which is what lets a captured-stdout artefact be
//! declared wholesale **without editing a byte of the stdout itself** — the
//! property that makes it evidence in the first place.
//!
//! The comment syntax around the marker is irrelevant; the gate looks for the
//! bare token. `<!-- straf3:claim ... -->` in Markdown and `# straf3:claim ...`
//! in a text file are the same marker.
//!
//! # `kind=snapshot` is not an escape hatch
//!
//! Marking something a snapshot does not mean the gate ignores it. It means the
//! gate checks something else. **A snapshot without a date and a build
//! identifier is indistinguishable from a stale live claim** — which is exactly
//! how the incident above happened, since `coil.txt`'s header sentence reads as
//! an instruction and only the paragraph above it makes it a dated record. So a
//! snapshot marker must carry both, and it is a failure if it does not.
//!
//! Live claims are checked for value; snapshots are checked for provenance.
//! Neither gets to be the unexamined one, which is what makes the distinction a
//! discipline rather than a loophole.
//!
//! # The category attaches to the claim, never to the file
//!
//! `docs/web/evidence/r6-selftest.txt` settles this on its own. Its
//! `collision digest 0x47263b8845d8bb4b` still reproduces today, because the
//! map has not changed. Its `rolling digest 0x79f08409c1bd3ccf` folds
//! per-command `SimState` checksums and moved with the state. One file, two
//! numbers, two fates. "This file is evidence" is not a licence over everything
//! printed in it.
//!
//! # Why a declared inventory rather than a scan of the tree
//!
//! Because the default for an unmarked number has to go somewhere, and both
//! obvious answers are bad. Defaulting unmarked to *snapshot* is the status quo
//! with extra machinery: every future document that publishes a number without
//! thinking decays silently. Defaulting unmarked to *live* turns CI red the
//! next time somebody adds an evidence file, and a gate that cries wolf gets
//! deleted — taking its real coverage with it.
//!
//! So the gate scans [`INVENTORY`], a declared list of files. Inside an
//! inventoried file every digest site must be marked and an unmarked one is a
//! failure; a file not in the inventory is not scanned at all. That is honest
//! rather than evasive: it moves the failure from "the gate silently missed a
//! document", which is undetectable, to "the inventory does not list it", which
//! is a short greppable list a human can audit in half a minute. It is the same
//! move [`crate::probes`] makes by listing a directory instead of carrying a
//! list, except that here the set genuinely cannot be derived from the
//! filesystem, and its explicitness is the point.
//!
//! Two guards keep the inventory from rotting: an inventoried file that no
//! longer exists is a failure, and a marked claim whose `kind` is unknown is a
//! failure. The third guard is [`NOT_COVERED`], read out on every run, so the
//! gate's silence is documented rather than mistaken for a guarantee.
//!
//! # What this gate does not cover, deliberately
//!
//! - **The canonical physics digest `0x4350ccc31bec5d4c`.** It is already an
//!   executable pin: `identity.rs::the_freeze_did_not_move_the_physics_digest`
//!   goes red under `cargo test --workspace`, which is strictly stronger than
//!   anything here. Covering it twice would only add a way for the two to
//!   disagree.
//! - **Bare integers that nobody declared.** A digest is mechanically
//!   identifiable — `0x` and sixteen hex digits — so *requiring* a marker on
//!   one is safe. A run time is just an integer, indistinguishable from every
//!   other integer in a page of captured stdout, so requiring markers on
//!   integers would either miss most of them or drown the reader. Run times are
//!   therefore checked **when explicitly claimed** (`kind=coil-replay-ms`) and
//!   not otherwise; the one that matters most is an executable pin already.
//! - **`docs/web/ARCHITECTURE.md`'s divergence tables and `probes/pacing`'s
//!   CSVs.** Historical measurements of specific events. Re-deriving them is
//!   not meaningful and, for the divergence tables, reproducing them would be
//!   the bug.
//! - **Provenance *reachability*.** A snapshot's `build=` is checked for shape,
//!   not resolved against the object database. `actions/checkout` clones to
//!   depth 1 by default, so resolving an old sha would fail in CI for a reason
//!   that has nothing to do with evidence — precisely the cry-wolf failure this
//!   design is built to avoid. Set `fetch-depth: 0` and this could be
//!   tightened.
//!
//! # Fail closed
//!
//! An unrecognised `kind`, a missing inventoried file, a marker governing
//! nothing, a binary that will not build, output that cannot be parsed — each
//! is a failure with a named reason, never a skip and never a warning. A gate
//! that skips is a gate that is green for the wrong reason, and this module
//! exists because something was green for the wrong reason.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

// ── the inventory ───────────────────────────────────────────────────────────

/// The files this gate reads. Every digest site inside one of these must carry
/// a marker; a file absent from this list is not examined at all.
///
/// Adding a document that publishes a number means adding it here. That is the
/// deliberate cost of the design — see the module docs on why the alternative
/// defaults are both worse.
pub const INVENTORY: &[&str] = &["probes/coil-course/results/coil.txt"];

/// Number-bearing files known to be outside the inventory, and why. Printed on
/// every run so the gate's silence is a stated position rather than an
/// oversight a reader has to discover.
pub const NOT_COVERED: &[(&str, &str)] = &[
    (
        "PLAYING.md",
        "publishes 0x9a854d1a3653d8b7 as a LIVE claim at three sites (the \
         checksum under `On the release build of this tree:`). It is owned by \
         another session this wave; the markers and the corrected value are \
         specified for it, and it joins the inventory when they land. This is \
         the largest known gap.",
    ),
    (
        "docs/web/evidence/*.txt",
        "captured stdout with no date and no commit in the files themselves — \
         their provenance lives in a neighbouring README.md, which is weaker \
         than a self-describing header and does not survive the file being read \
         alone. They are snapshots and the remedy is a dated note beside each, \
         not a regeneration. Owned elsewhere; specified, not applied.",
    ),
    (
        "docs/web/ARCHITECTURE.md",
        "the glibc/musl divergence tables record a divergence since fixed. \
         Reproducing those numbers today would be the bug, not the pass.",
    ),
    (
        "crates/**",
        "executable pins. `cargo test` compares them already, which is stronger \
         than this gate.",
    ),
];

// ── what a marker says ──────────────────────────────────────────────────────

/// The category a marker declares. Never inferred from the text around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Category {
    /// Must reproduce. Checked for its value.
    Live { kind: String },
    /// A record of what a build printed on a day. Checked for its provenance.
    Snapshot {
        taken: String,
        build: String,
        /// Covers every remaining site in the file rather than just the next
        /// one. This is what lets captured stdout be declared without editing
        /// the stdout.
        scope_eof: bool,
    },
}

/// One parsed marker line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// 1-based, so a failure can be clicked.
    pub line: usize,
    pub category: Category,
}

/// Something the gate refuses to let pass. Carries enough to act on without
/// re-deriving the diagnosis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub file: String,
    /// 1-based; `0` when the finding is about the file rather than a line.
    pub line: usize,
    pub detail: String,
}

impl Finding {
    fn new(file: &str, line: usize, detail: impl Into<String>) -> Self {
        Self {
            file: file.to_owned(),
            line,
            detail: detail.into(),
        }
    }
}

// ── marker parsing ──────────────────────────────────────────────────────────

/// The token that opens a live-claim marker, in any comment syntax.
const CLAIM_TOKEN: &str = "straf3:claim";
/// The token that opens a dated-snapshot marker, in any comment syntax.
const SNAPSHOT_TOKEN: &str = "straf3:snapshot";

/// Pull `key=value` pairs out of the remainder of a marker line.
///
/// Values are bare words. A marker is written by hand in a comment, so the
/// grammar is kept small enough that there is no way to write one that parses
/// differently than it reads. Trailing comment syntax (`-->`, `*/`) is dropped.
fn marker_fields(rest: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for word in rest.split_whitespace() {
        let word = word.trim_end_matches("-->").trim_end_matches("*/");
        if let Some((k, v)) = word.split_once('=')
            && !k.is_empty()
            && !v.is_empty()
        {
            out.insert(k.to_owned(), v.to_owned());
        }
    }
    out
}

/// `YYYY-MM-DD`, checked for shape and plausible ranges.
fn is_iso_date(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return false;
    }
    if !parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    let month: u32 = parts[1].parse().unwrap_or(0);
    let day: u32 = parts[2].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

/// A plausible abbreviated or full commit id. Shape only — see the module docs
/// on why reachability is deliberately not checked.
fn is_commit_shaped(s: &str) -> bool {
    (7..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Parse one marker line. `Err` is a finding, not an absence: a line that opens
/// a marker and then does not say what it means is worse than no marker.
pub fn parse_marker(file: &str, line_no: usize, line: &str) -> Option<Result<Marker, Finding>> {
    let (token, rest) = if let Some(at) = line.find(CLAIM_TOKEN) {
        (CLAIM_TOKEN, &line[at + CLAIM_TOKEN.len()..])
    } else if let Some(at) = line.find(SNAPSHOT_TOKEN) {
        (SNAPSHOT_TOKEN, &line[at + SNAPSHOT_TOKEN.len()..])
    } else {
        return None;
    };
    let fields = marker_fields(rest);

    if token == CLAIM_TOKEN {
        let Some(kind) = fields.get("kind") else {
            return Some(Err(Finding::new(
                file,
                line_no,
                "`straf3:claim` without `kind=`. A live claim has to say which \
                 reproducer settles it.",
            )));
        };
        if !KINDS.iter().any(|k| k.name == kind) {
            return Some(Err(Finding::new(
                file,
                line_no,
                format!(
                    "unknown claim kind `{kind}`. Known kinds: {}. An unknown \
                     kind is a failure rather than a skip — a claim nothing can \
                     re-derive is exactly what this gate exists to refuse.",
                    KINDS
                        .iter()
                        .map(|k| k.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )));
        }
        return Some(Ok(Marker {
            line: line_no,
            category: Category::Live { kind: kind.clone() },
        }));
    }

    // A snapshot. Provenance is the whole of what is checked, so its absence is
    // the whole of the failure.
    let taken = fields.get("taken");
    let build = fields.get("build");
    match (taken, build) {
        (Some(t), Some(b)) if is_iso_date(t) && is_commit_shaped(b) => Some(Ok(Marker {
            line: line_no,
            category: Category::Snapshot {
                taken: t.clone(),
                build: b.clone(),
                scope_eof: fields.get("scope").map(String::as_str) == Some("eof"),
            },
        })),
        (None, _) | (_, None) => Some(Err(Finding::new(
            file,
            line_no,
            "`straf3:snapshot` needs both `taken=YYYY-MM-DD` and `build=<commit>`. \
             A snapshot without provenance is indistinguishable from a stale live \
             claim, which is how the incident this gate exists for happened.",
        ))),
        (Some(t), Some(b)) => Some(Err(Finding::new(
            file,
            line_no,
            format!(
                "`straf3:snapshot` provenance is malformed: taken=`{t}` \
                 (want YYYY-MM-DD), build=`{b}` (want 7-40 hex digits)."
            ),
        ))),
    }
}

// ── value sites ─────────────────────────────────────────────────────────────

/// Find a `0x` followed by exactly 16 hex digits. Returns the digits.
///
/// Exactly sixteen: a 64-bit digest is the class of number that expires
/// silently here, and a looser pattern would sweep up byte counts and colour
/// literals and make the mandatory-marking rule unlivable.
pub fn find_digest(line: &str) -> Option<String> {
    let mut i = 0;
    while let Some(at) = line[i..].find("0x") {
        let start = i + at + 2;
        // `take_while` runs to the end of the hex run, so a 15- or 17-digit
        // literal fails this length test rather than matching a prefix of it.
        let digits: String = line[start..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .collect();
        if digits.len() == 16 {
            return Some(digits.to_ascii_lowercase());
        }
        i = start;
    }
    None
}

/// Find the first non-negative integer on a line.
///
/// Hex literals are removed first. `0x9a85...` begins with a digit, so a naive
/// scan would read the integer `0` out of a checksum line and compare that
/// against a run time — a false pass, which is the one outcome this module may
/// not produce.
fn find_integer(line: &str) -> Option<u64> {
    let mut cleaned = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("0x") {
        cleaned.push_str(&rest[..at]);
        cleaned.push(' ');
        let tail = &rest[at + 2..];
        let n = tail.chars().take_while(char::is_ascii_hexdigit).count();
        rest = &tail[n..];
    }
    cleaned.push_str(rest);

    cleaned
        .split(|c: char| !c.is_ascii_digit())
        .find(|t| !t.is_empty())
        .and_then(|t| t.parse().ok())
}

// ── the kinds, and what re-derives each ─────────────────────────────────────

/// How a published value is read out of a document, and how the same quantity
/// is read out of a reproducer's output.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueShape {
    /// A 64-bit hex digest.
    Digest,
    /// A count of milliseconds.
    Millis,
}

/// One kind of live claim: what it means, and the command that settles it.
pub struct Kind {
    pub name: &'static str,
    pub shape: ValueShape,
    /// Which reproducer produces it. Several kinds share one, so the command
    /// runs once and every claim reads its own field out of the same output.
    pub reproducer: Reproducer,
    /// The line in the reproducer's output the value is read from, identified
    /// by a substring rather than a position.
    pub output_marker: &'static str,
}

/// A command the gate can actually run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reproducer {
    /// Build the shipped release binary and replay the committed course
    /// fixture against the committed map. This is the command
    /// `probes/coil-course/results/coil.txt` publishes beside its numbers, run
    /// for real rather than believed.
    CoilReplay,
}

impl Reproducer {
    /// What a reader would type. Printed in every mismatch so the failure is
    /// actionable without reading this file.
    pub fn command_line(self) -> &'static str {
        match self {
            Reproducer::CoilReplay => {
                "cargo run --release -p straf3-game --bin straf3 -- \
                 --replay probes/coil-course/results/coil-run.txt \
                 --map assets/maps/coil.map"
            }
        }
    }
}

/// The claim kinds this gate understands. A `kind` outside this table is a
/// failure, which is what keeps the table from silently going stale.
pub const KINDS: &[Kind] = &[
    Kind {
        name: "coil-replay-checksum",
        shape: ValueShape::Digest,
        reproducer: Reproducer::CoilReplay,
        output_marker: "checksum",
    },
    Kind {
        name: "coil-replay-ms",
        shape: ValueShape::Millis,
        reproducer: Reproducer::CoilReplay,
        output_marker: "  run ",
    },
    Kind {
        name: "coil-collision-digest",
        shape: ValueShape::Digest,
        reproducer: Reproducer::CoilReplay,
        output_marker: "collision digest",
    },
];

fn kind(name: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.name == name)
}

// ── scanning one file ───────────────────────────────────────────────────────

/// A live claim found in a document: where it is, what it says, what settles it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveClaim {
    pub file: String,
    /// Line of the value, not of the marker.
    pub line: usize,
    pub kind: String,
    /// Exactly as published.
    pub published: String,
}

/// Everything one file's scan produced.
#[derive(Debug, Default)]
pub struct Scan {
    pub live: Vec<LiveClaim>,
    pub snapshots: usize,
    pub findings: Vec<Finding>,
}

/// Read a document's markers and the sites they govern.
///
/// Pure: it takes text and returns findings, so the rules can be tested without
/// building anything. The value comparison happens later, against a reproducer.
pub fn scan(file: &str, text: &str) -> Scan {
    let mut out = Scan::default();
    // A point marker waiting for the line it governs.
    let mut pending: Option<Marker> = None;
    // A `scope=eof` snapshot, once one has been declared.
    let mut blanket: Option<Marker> = None;

    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;

        if let Some(parsed) = parse_marker(file, line_no, raw) {
            // A marker line is never also a value site: the value it governs is
            // below it. Two point markers in a row means the first governs
            // nothing.
            if let Some(dangling) = pending.take() {
                out.findings.push(Finding::new(
                    file,
                    dangling.line,
                    "this marker governs nothing — the next line carries no value \
                     it could apply to. A marker that binds to nothing is a \
                     failure, because it reads as coverage that does not exist.",
                ));
            }
            match parsed {
                Ok(m) => {
                    if matches!(
                        m.category,
                        Category::Snapshot {
                            scope_eof: true,
                            ..
                        }
                    ) {
                        blanket = Some(m);
                    } else {
                        pending = Some(m);
                    }
                }
                Err(f) => out.findings.push(f),
            }
            continue;
        }

        let digest = find_digest(raw);

        // A pending point marker binds to this line whatever it looks like: for
        // a `Millis` claim the value is an ordinary integer, and demanding that
        // it look like a digest would make run times unclaimable.
        if let Some(m) = pending.take() {
            match &m.category {
                Category::Live { kind: k } => {
                    let spec = kind(k).expect("parse_marker rejects unknown kinds");
                    let published = match spec.shape {
                        ValueShape::Digest => digest.clone().map(|d| format!("0x{d}")),
                        ValueShape::Millis => find_integer(raw).map(|v| v.to_string()),
                    };
                    match published {
                        Some(p) => out.live.push(LiveClaim {
                            file: file.to_owned(),
                            line: line_no,
                            kind: k.clone(),
                            published: p,
                        }),
                        None => out.findings.push(Finding::new(
                            file,
                            line_no,
                            format!(
                                "the `straf3:claim kind={k}` marker on line {} governs \
                                 this line, but no {} value could be read from it.",
                                m.line,
                                match spec.shape {
                                    ValueShape::Digest => "0x<16 hex digits>",
                                    ValueShape::Millis => "integer",
                                }
                            ),
                        )),
                    }
                }
                Category::Snapshot { .. } => out.snapshots += 1,
            }
            continue;
        }

        // No point marker. A digest here is covered only by a blanket snapshot.
        if digest.is_some() {
            if blanket.is_some() {
                out.snapshots += 1;
            } else {
                out.findings.push(Finding::new(
                    file,
                    line_no,
                    "unmarked digest. Every digest in an inventoried file must \
                     declare whether it is a live claim that must reproduce \
                     (`straf3:claim kind=...`) or a dated snapshot that must \
                     carry provenance (`straf3:snapshot taken=... build=...`). \
                     The category cannot be inferred from the text — the same \
                     digest is a live claim in one file and a snapshot in \
                     another.",
                ));
            }
        }
    }

    if let Some(dangling) = pending {
        out.findings.push(Finding::new(
            file,
            dangling.line,
            "this marker is the last thing in the file and governs nothing.",
        ));
    }
    out
}

// ── running the reproducers ─────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask/ always has a parent")
        .to_path_buf()
}

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// Build the shipped binary and replay the committed fixture, returning
/// everything it printed.
///
/// stdout **and** stderr, concatenated: the collision digest is logged through
/// `env_logger`, which writes to stderr, while the checksum is on stdout. A
/// gate that read only one of the two would silently stop covering whichever
/// number moved channels.
fn run_coil_replay(root: &Path) -> Result<String, String> {
    let build = Command::new(cargo())
        .current_dir(root)
        .args(["build", "--release", "-p", "straf3-game", "--bin", "straf3"])
        .output()
        .map_err(|e| format!("could not run cargo: {e}"))?;
    if !build.status.success() {
        let stderr = String::from_utf8_lossy(&build.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(15).collect();
        return Err(format!(
            "the release client did not build, so no published number could be \
             re-derived:\n{}",
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        ));
    }

    let exe = root
        .join("target/release")
        .join(if cfg!(windows) { "straf3.exe" } else { "straf3" });
    if !exe.is_file() {
        return Err(format!(
            "cargo reported success but {} does not exist",
            exe.display()
        ));
    }

    let out = Command::new(&exe)
        .current_dir(root)
        .args([
            "--replay",
            "probes/coil-course/results/coil-run.txt",
            "--map",
            "assets/maps/coil.map",
        ])
        .output()
        .map_err(|e| format!("could not run {}: {e}", exe.display()))?;
    if !out.status.success() {
        return Err(format!(
            "the replay exited {:?}:\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

/// Read one kind's current value out of a reproducer's output.
pub fn extract(spec: &Kind, output: &str) -> Result<String, String> {
    let line = output
        .lines()
        .find(|l| l.contains(spec.output_marker))
        .ok_or_else(|| {
            format!(
                "no line containing `{}` in the reproducer's output. The output \
                 format changed, or the command did not do what this gate thinks \
                 it does; either way the claim is unverified, which is a failure.",
                spec.output_marker.trim()
            )
        })?;
    match spec.shape {
        ValueShape::Digest => find_digest(line)
            .map(|d| format!("0x{d}"))
            .ok_or_else(|| format!("no digest on the `{}` line: {line:?}", spec.output_marker)),
        ValueShape::Millis => find_integer(line)
            .map(|v| v.to_string())
            .ok_or_else(|| format!("no integer on the `{}` line: {line:?}", spec.output_marker)),
    }
}

// ── the command ─────────────────────────────────────────────────────────────

/// Scan the inventory, re-derive every live claim, and report everything that
/// does not hold. `Ok(false)` means findings; `Err` means the check itself
/// could not run, which is also a failure at the call site.
pub fn run(argv: &[String]) -> Result<bool, String> {
    if let Some(bad) = argv.first() {
        return Err(format!(
            "unknown argument {bad}\nusage: cargo xtask check-evidence"
        ));
    }
    let root = workspace_root();

    // ── scan ────────────────────────────────────────────────────────────────
    let mut findings: Vec<Finding> = Vec::new();
    let mut live: Vec<LiveClaim> = Vec::new();
    let mut snapshots = 0usize;

    println!("  inventory ({} file(s)):", INVENTORY.len());
    for rel in INVENTORY {
        let path = root.join(rel);
        let Ok(text) = std::fs::read_to_string(&path) else {
            println!("    {rel:<48} MISSING");
            findings.push(Finding::new(
                rel,
                0,
                "inventoried file does not exist. Either it moved and the \
                 inventory is stale, or it was deleted and its claims went with \
                 it. Both are failures: this list is the gate's only record of \
                 what it is supposed to be covering.",
            ));
            continue;
        };
        let scan = scan(rel, &text);
        println!(
            "    {rel:<48} {} live, {} snapshot, {} finding(s)",
            scan.live.len(),
            scan.snapshots,
            scan.findings.len()
        );
        snapshots += scan.snapshots;
        live.extend(scan.live);
        findings.extend(scan.findings);
    }

    // ── re-derive ───────────────────────────────────────────────────────────
    // Group by reproducer so each command runs once however many claims read it.
    let mut needed: Vec<Reproducer> = Vec::new();
    for c in &live {
        let spec = kind(&c.kind).expect("scan only emits known kinds");
        if !needed.contains(&spec.reproducer) {
            needed.push(spec.reproducer);
        }
    }

    let mut outputs: BTreeMap<&'static str, Result<String, String>> = BTreeMap::new();
    for r in &needed {
        println!("\n  re-deriving with: {}", r.command_line());
        let produced = match r {
            Reproducer::CoilReplay => run_coil_replay(&root),
        };
        match &produced {
            Ok(_) => println!("    ran"),
            Err(e) => println!("    FAILED: {e}"),
        }
        outputs.insert(r.command_line(), produced);
    }

    if !live.is_empty() {
        println!("\n  live claims:");
    }
    for c in &live {
        let spec = kind(&c.kind).expect("scan only emits known kinds");
        let current = match outputs
            .get(spec.reproducer.command_line())
            .expect("every needed reproducer ran")
        {
            Ok(text) => extract(spec, text),
            Err(e) => Err(e.clone()),
        };
        match current {
            Ok(now) if now == c.published => {
                println!("    ok   {}:{} {} = {}", c.file, c.line, c.kind, now);
            }
            Ok(now) => {
                println!("    RED  {}:{} {}", c.file, c.line, c.kind);
                findings.push(Finding::new(
                    &c.file,
                    c.line,
                    format!(
                        "published {} but this tree produces {now}.\n\
                         kind:      {}\n\
                         reproduce: {}",
                        c.published,
                        c.kind,
                        spec.reproducer.command_line()
                    ),
                ));
            }
            Err(e) => {
                println!("    RED  {}:{} {} (unreproducible)", c.file, c.line, c.kind);
                findings.push(Finding::new(
                    &c.file,
                    c.line,
                    format!(
                        "could not be re-derived, which is a failure and not a \
                         skip: {e}\nreproduce: {}",
                        spec.reproducer.command_line()
                    ),
                ));
            }
        }
    }

    // ── the stated hole ─────────────────────────────────────────────────────
    println!("\n  NOT covered by this gate, deliberately:");
    for (what, why) in NOT_COVERED {
        println!("    {what}");
        for chunk in wrap(why, 68) {
            println!("        {chunk}");
        }
    }

    if findings.is_empty() {
        println!(
            "\n  {} live claim(s) re-derived and agree; {snapshots} dated \
             snapshot(s) carry provenance.",
            live.len()
        );
        return Ok(true);
    }

    println!("\n  {} finding(s):\n", findings.len());
    let mut report = String::new();
    for f in &findings {
        if f.line == 0 {
            let _ = writeln!(report, "  ── {} ──", f.file);
        } else {
            let _ = writeln!(report, "  ── {}:{} ──", f.file, f.line);
        }
        for line in f.detail.lines() {
            for chunk in wrap(line, 72) {
                let _ = writeln!(report, "     {chunk}");
            }
        }
        let _ = writeln!(report);
    }
    print!("{report}");
    Ok(false)
}

/// Wrap at word boundaries. Hand-rolled because xtask has no dependencies.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}
