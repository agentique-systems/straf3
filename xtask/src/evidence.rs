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

/// Where a snapshot's number was measured.
///
/// `docs/web/ARCHITECTURE.md` §1.2 forced this distinction. Four of its digests
/// were measured on a `/tmp` copy of `straf3-sim` with `libm` substituted, and
/// that copy has since been deleted — so no command in this repository can ever
/// re-derive them, and no future one will be able to either.
///
/// That has to be sayable without becoming a way to silence the gate. It is
/// not one, for two reasons: an out-of-tree snapshot still has to carry a date
/// and a build like any other, and the gate **names every one of them
/// individually** on every run rather than folding them into a count. The
/// escape hatch a reader cannot see is the dangerous kind; this one is read
/// aloud each time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measured {
    /// Produced by this repository at the stated commit. The default.
    InTree,
    /// Produced somewhere this repository cannot reach. Nothing can re-derive
    /// it, now or later, and saying so is the honest record.
    OutOfTree,
}

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
        measured: Measured,
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
    let (token, rest) = match (line.find(CLAIM_TOKEN), line.find(SNAPSHOT_TOKEN)) {
        (Some(at), _) => (CLAIM_TOKEN, &line[at + CLAIM_TOKEN.len()..]),
        (None, Some(at)) => (SNAPSHOT_TOKEN, &line[at + SNAPSHOT_TOKEN.len()..]),
        (None, None) => return None,
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
                    KINDS.iter().map(|k| k.name).collect::<Vec<_>>().join(", ")
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
    // `measured=` is optional, but a value outside the enumeration is a
    // failure rather than a shrug: a typo'd `measured=out_of_tree` must not
    // quietly become the default.
    let measured = match fields.get("measured").map(String::as_str) {
        None | Some("in-tree") => Measured::InTree,
        Some("out-of-tree") => Measured::OutOfTree,
        Some(other) => {
            return Some(Err(Finding::new(
                file,
                line_no,
                format!(
                    "`measured={other}` is not a value this gate knows. Use \
                     `in-tree` (the default, may be omitted) or `out-of-tree` \
                     for a number measured somewhere this repository cannot \
                     reach and can never re-derive."
                ),
            )));
        }
    };
    // `scope=` likewise: a misspelled scope would silently degrade a blanket
    // marker to a point marker and leave everything below it unmarked.
    let scope_eof = match fields.get("scope").map(String::as_str) {
        None => false,
        Some("eof") => true,
        Some(other) => {
            return Some(Err(Finding::new(
                file,
                line_no,
                format!("`scope={other}` is not a value this gate knows. The only scope is `eof`."),
            )));
        }
    };

    let taken = fields.get("taken");
    let build = fields.get("build");
    match (taken, build) {
        (Some(t), Some(b)) if is_iso_date(t) && is_commit_shaped(b) => Some(Ok(Marker {
            line: line_no,
            category: Category::Snapshot {
                taken: t.clone(),
                build: b.clone(),
                scope_eof,
                measured,
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
    // Case-folded first, so `0X` is found as readily as `0x`. Not cosmetic: a
    // digest this function fails to see is a digest the mandatory-marking rule
    // never demands a marker for, which is a hole in a gate whose whole promise
    // is that it fails closed. `to_ascii_lowercase` preserves byte length, so
    // the offsets below still index the original correctly.
    let lower = line.to_ascii_lowercase();
    let mut i = 0;
    while let Some(at) = lower[i..].find("0x") {
        let start = i + at + 2;
        // `take_while` runs to the end of the hex run, so a 15- or 17-digit
        // literal fails this length test rather than matching a prefix of it.
        let digits: String = lower[start..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .collect();
        if digits.len() == 16 {
            return Some(digits);
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
    let lower = line.to_ascii_lowercase();
    let mut cleaned = String::with_capacity(line.len());
    let mut rest = lower.as_str();
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

/// A snapshot site nothing in this repository can re-derive. Tracked
/// individually rather than counted, so it is named on every run — see
/// [`Measured::OutOfTree`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutOfTreeSite {
    pub file: String,
    pub line: usize,
    pub taken: String,
    pub build: String,
}

/// Everything one file's scan produced.
#[derive(Debug, Default)]
pub struct Scan {
    pub live: Vec<LiveClaim>,
    pub snapshots: usize,
    pub out_of_tree: Vec<OutOfTreeSite>,
    pub findings: Vec<Finding>,
}

/// Count a snapshot site, and name it if nothing can ever re-derive it.
fn record_snapshot(out: &mut Scan, file: &str, line_no: usize, m: &Marker) {
    out.snapshots += 1;
    if let Category::Snapshot {
        taken,
        build,
        measured: Measured::OutOfTree,
        ..
    } = &m.category
    {
        out.out_of_tree.push(OutOfTreeSite {
            file: file.to_owned(),
            line: line_no,
            taken: taken.clone(),
            build: build.clone(),
        });
    }
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
                Category::Snapshot { .. } => record_snapshot(&mut out, file, line_no, &m),
            }
            continue;
        }

        // No point marker. A digest here is covered only by a blanket snapshot.
        if digest.is_some() {
            if let Some(b) = blanket.clone() {
                record_snapshot(&mut out, file, line_no, &b);
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

// ── the completeness report ─────────────────────────────────────────────────

/// Where documents that publish numbers tend to live. Used **only** to print a
/// list, never to decide anything.
const EVIDENCE_PATHS: &[&str] = &["docs", "probes", "PLAYING.md", "PLAYTEST.md", "README.md"];

/// Every `.md`/`.txt` file under [`EVIDENCE_PATHS`] that contains a digest and
/// is not in the inventory.
///
/// This never fails the build, and that is the point of it. The inventory's
/// weakness is that a document can be forgotten; the answer is not to guess a
/// category from a path — that is the silent wrongness this module exists to
/// remove — but to make the completeness audit something a reader cannot avoid
/// seeing rather than something they must remember to perform.
fn uninventoried(root: &Path) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    for entry in EVIDENCE_PATHS {
        walk(&root.join(entry), root, &mut found);
    }
    found.sort();
    found
}

fn walk(path: &Path, root: &Path, found: &mut Vec<(String, usize)>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            walk(&p, root, found);
        }
        return;
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !matches!(ext, "md" | "txt") {
        return;
    }
    let Ok(rel) = path.strip_prefix(root) else {
        return;
    };
    let rel = rel.to_string_lossy().replace('\\', "/");
    if INVENTORY.contains(&rel.as_str()) {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let n = text.lines().filter(|l| find_digest(l).is_some()).count();
    if n > 0 {
        found.push((rel, n));
    }
}

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

    let exe = root.join("target/release").join(if cfg!(windows) {
        "straf3.exe"
    } else {
        "straf3"
    });
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
    let mut out_of_tree: Vec<OutOfTreeSite> = Vec::new();

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
        out_of_tree.extend(scan.out_of_tree);
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

    // ── numbers nothing can ever re-derive ──────────────────────────────────
    // Named, never merely counted: an escape hatch a reader cannot see is the
    // dangerous kind.
    if !out_of_tree.is_empty() {
        println!(
            "\n  measured OUT OF TREE — no command in this repository can \
             re-derive these,\n  now or ever. They are records, not claims:"
        );
        for s in &out_of_tree {
            println!(
                "    {}:{}  taken {} on {}",
                s.file, s.line, s.taken, s.build
            );
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

    // ── the completeness audit, as output rather than as a chore ────────────
    let missing = uninventoried(&root);
    println!(
        "\n  files under docs/, probes/ and the top-level documents that publish\n  \
         a digest and are NOT in the inventory ({}). This is a LIST, not a\n  \
         verdict — nothing here is failing, and the category of a number is\n  \
         never inferred from where it lives:",
        missing.len()
    );
    for (path, n) in &missing {
        println!("    {path:<52} {n} digest line(s)");
    }

    if findings.is_empty() {
        println!(
            "\n  {} live claim(s) re-derived from the command published beside \
             them and agree;\n  {snapshots} dated snapshot(s) carry a date and a \
             build, of which {} could\n  never be re-derived by anything here.",
            live.len(),
            out_of_tree.len()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// What the shipped binary actually prints, so the extractors are pinned
    /// against the real format rather than against a convenient invention. If
    /// the CLI's output shape changes, these tests are where it surfaces.
    ///
    /// Measured on this host: stdout, then `env_logger`'s stderr line. The
    /// output shape is *not* stable across history — at `59cfd8f` there was no
    /// `run` line at all — which is exactly why the gate reads by substring
    /// and fails closed when a line is absent.
    const REPLAY_TRANSCRIPT: &str = "\
straf3 replay (probes/coil-course/results/coil-run.txt)
  commands      864
  rate          125 Hz (8 ms per command)
  profile       cpm
  world         Map
final state
  tick          864
  time          6912 ms
  run           5096 ms  (5.096 s, start 1800 ms, finish 6896 ms)
  origin        23.528534 3409.000000 38.738361
  checksum      0xf3cabd183c90d8d7
[INFO  straf3_game::scene] map: 26 hulls, 4 triggers, collision digest 0x47263b8845d8bb4b
";

    fn findings(text: &str) -> Vec<String> {
        scan("f.txt", text)
            .findings
            .into_iter()
            .map(|f| f.detail)
            .collect()
    }

    // ── the mandatory-marking rule ──────────────────────────────────────────

    #[test]
    fn an_unmarked_digest_is_a_failure() {
        // The whole premise: inside an inventoried file, a number that does not
        // declare what it is cannot be allowed to sit there quietly.
        let f = findings("  checksum 0x9a854d1a3653d8b7\n");
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].contains("unmarked digest"), "{}", f[0]);
    }

    #[test]
    fn a_line_with_no_digest_needs_no_marker() {
        // The rule must not make ordinary prose unwritable.
        assert!(findings("the run took 5096 ms and finished.\n").is_empty());
    }

    // ── live claims ─────────────────────────────────────────────────────────

    #[test]
    fn a_live_claim_carries_its_published_value_and_kind() {
        let s = scan(
            "f.txt",
            "# straf3:claim kind=coil-replay-checksum\n  checksum 0x9a854d1a3653d8b7\n",
        );
        assert!(s.findings.is_empty(), "{:?}", s.findings);
        assert_eq!(s.live.len(), 1);
        assert_eq!(s.live[0].published, "0x9a854d1a3653d8b7");
        assert_eq!(s.live[0].kind, "coil-replay-checksum");
        assert_eq!(s.live[0].line, 2, "the value's line, not the marker's");
    }

    #[test]
    fn an_unknown_kind_is_a_failure_and_not_a_skip() {
        // A claim nothing can re-derive is precisely what this gate refuses. If
        // this ever became a skip, the gate would go green on a number it never
        // looked at.
        let f = findings("# straf3:claim kind=invented-yesterday\n  0x9a854d1a3653d8b7\n");
        assert!(f.iter().any(|d| d.contains("unknown claim kind")), "{f:?}");
    }

    #[test]
    fn a_claim_without_a_kind_is_a_failure() {
        let f = findings("# straf3:claim\n  0x9a854d1a3653d8b7\n");
        assert!(f.iter().any(|d| d.contains("without `kind=`")), "{f:?}");
    }

    #[test]
    fn a_run_time_claim_reads_an_integer_rather_than_a_digest() {
        let s = scan(
            "f.txt",
            "# straf3:claim kind=coil-replay-ms\n  run 5096 ms\n",
        );
        assert!(s.findings.is_empty(), "{:?}", s.findings);
        assert_eq!(s.live[0].published, "5096");
    }

    #[test]
    fn a_claim_whose_value_line_holds_no_such_value_is_a_failure() {
        // A digest where an integer was promised. Silently reading `0` out of
        // `0x9a85...` and comparing that against a run time would be a false
        // pass, which is the one outcome this module may not produce.
        let f = findings("# straf3:claim kind=coil-replay-ms\n  checksum 0x9a854d1a3653d8b7\n");
        assert!(f.iter().any(|d| d.contains("no integer value")), "{f:?}");
    }

    // ── snapshots are checked, for something else ───────────────────────────

    #[test]
    fn a_snapshot_without_provenance_is_a_failure() {
        // The heart of the design. A snapshot with no date and no build is
        // indistinguishable from a stale live claim — which is how coil.txt's
        // number went wrong for nine days without anyone being able to tell.
        let f = findings("# straf3:snapshot\n  0x9a854d1a3653d8b7\n");
        assert!(f.iter().any(|d| d.contains("needs both")), "{f:?}");
    }

    #[test]
    fn a_snapshot_missing_only_the_build_is_still_a_failure() {
        let f = findings("# straf3:snapshot taken=2026-08-16\n  0x9a854d1a3653d8b7\n");
        assert!(f.iter().any(|d| d.contains("needs both")), "{f:?}");
    }

    #[test]
    fn malformed_provenance_is_a_failure() {
        for bad in [
            "# straf3:snapshot taken=August build=59cfd8f",
            "# straf3:snapshot taken=2026-13-99 build=59cfd8f",
            "# straf3:snapshot taken=2026-08-16 build=xyz",
            "# straf3:snapshot taken=2026-08-16 build=59cf",
        ] {
            let f = findings(&format!("{bad}\n  0x9a854d1a3653d8b7\n"));
            assert!(!f.is_empty(), "accepted malformed provenance: {bad}");
        }
    }

    #[test]
    fn a_well_formed_snapshot_passes_and_is_counted() {
        let s = scan(
            "f.txt",
            "# straf3:snapshot taken=2026-08-16 build=59cfd8f\n  0x9a854d1a3653d8b7\n",
        );
        assert!(s.findings.is_empty(), "{:?}", s.findings);
        assert_eq!(s.snapshots, 1);
        assert!(s.live.is_empty(), "a snapshot is never re-derived");
    }

    // ── the load-bearing property ───────────────────────────────────────────

    #[test]
    fn the_same_digest_is_live_in_one_file_and_a_snapshot_in_another() {
        // This is why the category is declared and never detected.
        // `0x9a854d1a3653d8b7` is a live claim in PLAYING.md and a dated
        // snapshot in coil.txt — identical text, opposite fates. Any rule that
        // inferred the category from the number, the path or a nearby date
        // would get one of these two wrong, and silently.
        let live = scan(
            "PLAYING.md",
            "<!-- straf3:claim kind=coil-replay-checksum -->\n  checksum 0x9a854d1a3653d8b7\n",
        );
        let snap = scan(
            "coil.txt",
            "# straf3:snapshot taken=2026-08-16 build=59cfd8f\n  checksum 0x9a854d1a3653d8b7\n",
        );
        assert_eq!(live.live.len(), 1);
        assert_eq!(live.snapshots, 0);
        assert_eq!(snap.live.len(), 0);
        assert_eq!(snap.snapshots, 1);
        assert!(live.findings.is_empty() && snap.findings.is_empty());
    }

    #[test]
    fn a_blanket_snapshot_covers_captured_output_without_editing_it() {
        // `scope=eof` is what lets a probe's own stdout be declared wholesale.
        // Requiring a marker per line would mean editing the captured output,
        // destroying the one property that makes it evidence.
        let s = scan(
            "coil.txt",
            "# straf3:snapshot taken=2026-08-16 build=59cfd8f scope=eof\n\
             \n\
             collision digest 0x47263b8845d8bb4b\n\
             some prose\n\
             final checksum 0x9a854d1a3653d8b7\n",
        );
        assert!(s.findings.is_empty(), "{:?}", s.findings);
        assert_eq!(s.snapshots, 2);
    }

    // ── markers that promise coverage they do not deliver ───────────────────

    #[test]
    fn a_marker_governing_nothing_is_a_failure() {
        // A marker binding to nothing reads as coverage that does not exist,
        // which is worse than no marker at all.
        let f = findings("# straf3:claim kind=coil-replay-checksum\n");
        assert!(f.iter().any(|d| d.contains("governs nothing")), "{f:?}");
    }

    #[test]
    fn two_markers_in_a_row_means_the_first_governs_nothing() {
        let f = findings(
            "# straf3:claim kind=coil-replay-checksum\n\
             # straf3:claim kind=coil-replay-checksum\n\
             0x9a854d1a3653d8b7\n",
        );
        assert!(f.iter().any(|d| d.contains("governs nothing")), "{f:?}");
    }

    #[test]
    fn an_unknown_scope_or_measured_value_is_a_failure() {
        // A typo'd `scope=EOF` would silently degrade a blanket marker into a
        // point marker and leave every line below it unmarked; a typo'd
        // `measured=out_of_tree` would silently claim in-tree provenance.
        for bad in [
            "# straf3:snapshot taken=2026-08-16 build=59cfd8f scope=all",
            "# straf3:snapshot taken=2026-08-16 build=59cfd8f measured=out_of_tree",
        ] {
            let f = findings(&format!("{bad}\n0x9a854d1a3653d8b7\n"));
            assert!(!f.is_empty(), "accepted: {bad}");
        }
    }

    #[test]
    fn an_out_of_tree_snapshot_is_named_rather_than_merely_counted() {
        // ARCHITECTURE.md §1.2's digests were measured on a deleted /tmp copy.
        // That must be sayable without becoming a silent exemption, so every
        // one is named on every run.
        let s = scan(
            "ARCHITECTURE.md",
            "<!-- straf3:snapshot taken=2026-08-14 build=a0e62d4 measured=out-of-tree -->\n\
             glibc 0x2af318592c222e64\n",
        );
        assert!(s.findings.is_empty(), "{:?}", s.findings);
        assert_eq!(s.out_of_tree.len(), 1);
        assert_eq!(s.out_of_tree[0].line, 2);
        assert_eq!(s.out_of_tree[0].taken, "2026-08-14");
    }

    // ── syntax independence ─────────────────────────────────────────────────

    #[test]
    fn markers_work_in_any_comment_syntax() {
        // Markdown, shell-style text files and Rust all have to be able to
        // carry one, so the gate reads the bare token and ignores the wrapper.
        for m in [
            "<!-- straf3:claim kind=coil-replay-checksum -->",
            "# straf3:claim kind=coil-replay-checksum",
            "// straf3:claim kind=coil-replay-checksum",
            "/* straf3:claim kind=coil-replay-checksum */",
        ] {
            let s = scan("f", &format!("{m}\n  0x9a854d1a3653d8b7\n"));
            assert!(s.findings.is_empty(), "{m} -> {:?}", s.findings);
            assert_eq!(s.live.len(), 1, "{m}");
        }
    }

    // ── the value readers ───────────────────────────────────────────────────

    #[test]
    fn find_digest_wants_exactly_sixteen_hex_digits() {
        assert_eq!(
            find_digest("checksum 0x9a854d1a3653d8b7").as_deref(),
            Some("9a854d1a3653d8b7")
        );
        assert_eq!(find_digest("0x9a854d1a3653d8b").as_deref(), None, "15");
        assert_eq!(find_digest("0x9a854d1a3653d8b77").as_deref(), None, "17");
        assert_eq!(find_digest("bit 0x04").as_deref(), None);
        assert_eq!(find_digest("no digest here").as_deref(), None);
        // Case-folded, so a document may write either.
        assert_eq!(
            find_digest("0X9A854D1A3653D8B7").as_deref(),
            Some("9a854d1a3653d8b7")
        );
    }

    #[test]
    fn find_integer_ignores_hex_literals() {
        // `0x9a85...` starts with a digit. A naive scan would read `0`.
        assert_eq!(find_integer("checksum 0x9a854d1a3653d8b7"), None);
        assert_eq!(find_integer("  run  5096 ms  (5.096 s)"), Some(5096));
        assert_eq!(find_integer("0x47263b8845d8bb4b then 26"), Some(26));
    }

    // ── the comparator, against the real transcript ─────────────────────────

    #[test]
    fn each_kind_reads_its_own_field_out_of_one_replay() {
        // Three claims, one command. The grouping is what keeps the gate from
        // building the client once per number.
        let got = |name: &str| extract(kind(name).unwrap(), REPLAY_TRANSCRIPT).unwrap();
        assert_eq!(got("coil-replay-checksum"), "0xf3cabd183c90d8d7");
        assert_eq!(got("coil-replay-ms"), "5096");
        assert_eq!(got("coil-collision-digest"), "0x47263b8845d8bb4b");
    }

    #[test]
    fn extract_fails_closed_when_the_output_line_is_absent() {
        // The 59cfd8f case, made into a test: that build printed no `run` line
        // at all. Reading a missing field as "no change" would be the exact
        // false pass this module exists to prevent.
        let without_run = REPLAY_TRANSCRIPT
            .lines()
            .filter(|l| !l.contains("  run "))
            .collect::<Vec<_>>()
            .join("\n");
        let e = extract(kind("coil-replay-ms").unwrap(), &without_run).unwrap_err();
        assert!(e.contains("no line containing"), "{e}");
    }

    #[test]
    fn a_value_that_moved_is_visible_to_the_comparator() {
        // The gate's whole job, in one assertion: what a document published and
        // what the tree produces now are different strings.
        let published = "0x9a854d1a3653d8b7";
        let current = extract(kind("coil-replay-checksum").unwrap(), REPLAY_TRANSCRIPT).unwrap();
        assert_ne!(published, current);
    }

    // ── the table itself ────────────────────────────────────────────────────

    #[test]
    fn every_kind_is_named_once_and_is_reachable() {
        let mut names: Vec<&str> = KINDS.iter().map(|k| k.name).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), before, "two kinds share a name");
        for k in KINDS {
            assert!(kind(k.name).is_some());
            assert!(
                !k.reproducer.command_line().is_empty(),
                "{} has no published command",
                k.name
            );
        }
    }

    #[test]
    fn the_inventory_is_not_empty_and_names_real_paths() {
        // An empty inventory is a gate that checks nothing while exiting 0.
        assert!(!INVENTORY.is_empty());
        for f in INVENTORY {
            assert!(!f.contains('\\'), "{f} must use forward slashes");
        }
    }
}
