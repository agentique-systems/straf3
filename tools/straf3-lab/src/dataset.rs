//! What a measurement is, and how it is written down.
//!
//! # The two audiences, and why the numbers are formatted once
//!
//! The results document has two readers: a person deciding whether a mechanic
//! is worth keeping, and the regression test in `tests/pinned.rs` deciding
//! whether a movement change was intended. Both read the *same* numbers, from
//! the same [`Dataset`], because a document and a fixture that are generated
//! separately will eventually disagree and the disagreement will be discovered
//! by nobody.
//!
//! # Where the precision landed, and why it is a judgement rather than a default
//!
//! Every value here is formatted at a fixed number of decimals, chosen per
//! measurement kind. The tension the choice resolves:
//!
//! - Too many decimals and the document churns. `f32` physics carries ~7
//!   significant digits; printing all of them means an unrelated refactor that
//!   reassociates one multiplication rewrites hundreds of lines, and a real
//!   change is then invisible in the noise of a diff nobody reads.
//! - Too few and a real movement change hides. Rounding a speed to whole
//!   units per second conceals a 0.3% change in `air_accelerate`, which a
//!   player feels.
//!
//! Where it landed:
//!
//! | Kind | Decimals | Reasoning |
//! |---|---|---|
//! | speed (ups) | 2 | 0.01 ups on a 320 ups run is 3·10⁻⁵ — finer than any tuning change worth making, coarser than `f32`'s last 7 mantissa bits |
//! | angle (°) | 2 | one 16-bit view-angle quantum is 0.0055°, so 0.01° is one quantum's worth of resolution: the finest input the game can express is visible, and nothing finer is |
//! | distance (units) | 3 | step and edge measurements turn on `SURFACE_CLIP_EPSILON` = 0.125 and on hull half-widths; a milli-unit resolves those and stops well short of float noise |
//! | time (ms) | 0 | every timer in the simulation is an integer millisecond. A fractional one would be a lie about the representation |
//! | ratio | 4 | dimensionless fractions are read against 1.0, where 10⁻⁴ is the same relative resolution 2 decimals give a speed |
//!
//! A measurement that needs different treatment says so at its own call site
//! rather than widening these.

use std::fmt::Write as _;

/// One number, with a name a diff can print.
///
/// The name is the whole point. Criterion 2 asks for a regression test that
/// *names which measurement moved*; that is only possible if every number has
/// carried a stable name since the moment it was computed, rather than being
/// identified after the fact by its position in a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    /// Dotted, stable, sortable. Numbers inside a key are zero-padded so that
    /// lexicographic order is numeric order and the document's rows do not
    /// jump about between runs.
    pub key: String,
    /// The value, already formatted. See the module docs on precision.
    pub value: String,
    /// What the value is in. `""` for a dimensionless flag or a label.
    pub unit: &'static str,
}

impl Measurement {
    /// A speed, in units per second.
    #[must_use]
    pub fn ups(key: impl Into<String>, v: f32) -> Self {
        Self::new(key, format!("{v:.2}"), "ups")
    }

    /// An angle, in degrees.
    #[must_use]
    pub fn degrees(key: impl Into<String>, v: f32) -> Self {
        Self::new(key, format!("{v:.2}"), "deg")
    }

    /// A distance, in Quake units.
    #[must_use]
    pub fn units(key: impl Into<String>, v: f32) -> Self {
        Self::new(key, format!("{v:.3}"), "units")
    }

    /// A duration, in whole milliseconds.
    #[must_use]
    pub fn ms(key: impl Into<String>, v: u32) -> Self {
        Self::new(key, format!("{v}"), "ms")
    }

    /// A count of something.
    #[must_use]
    pub fn count(key: impl Into<String>, v: u32) -> Self {
        Self::new(key, format!("{v}"), "n")
    }

    /// A dimensionless fraction.
    #[must_use]
    pub fn ratio(key: impl Into<String>, v: f32) -> Self {
        Self::new(key, format!("{v:.4}"), "")
    }

    /// A yes/no answer. Spelled out, because `1` and `0` in a results table are
    /// ambiguous in a way `yes` and `no` are not.
    #[must_use]
    pub fn flag(key: impl Into<String>, v: bool) -> Self {
        Self::new(key, if v { "yes" } else { "no" }.to_string(), "")
    }

    /// A short label — a state name, a classification.
    #[must_use]
    pub fn label(key: impl Into<String>, v: impl Into<String>) -> Self {
        Self::new(key, v.into(), "")
    }

    fn new(key: impl Into<String>, value: String, unit: &'static str) -> Self {
        Self {
            key: key.into(),
            value: unsign_zero(value),
            unit,
        }
    }
}

/// Turn `-0.00` into `0.00`.
///
/// Not cosmetic. A value hovering either side of zero — the gain from a strafe
/// angle whose clamp has closed, the cost of a step that costs nothing — lands
/// on a negative sign about half the time for reasons that are entirely below
/// the printed precision. Left alone it produces diff noise in the pinned file
/// that says "this measurement moved" when nothing about the movement did,
/// which is the one thing the regression test must not cry wolf about. A value
/// that is genuinely negative keeps its sign, because it has a nonzero digit.
fn unsign_zero(value: String) -> String {
    match value.strip_prefix('-') {
        Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
        _ => value,
    }
}

/// A table in the human-readable half of the report.
#[derive(Debug, Clone, Default)]
pub struct Table {
    /// What the table is showing, rendered above it.
    pub caption: String,
    /// Column headings.
    pub headers: Vec<String>,
    /// Rows, each the same length as `headers`.
    pub rows: Vec<Vec<String>>,
}

impl Table {
    /// An empty table with these headings.
    #[must_use]
    pub fn new(caption: impl Into<String>, headers: &[&str]) -> Self {
        Self::with_headers(caption, headers.iter().map(|h| (*h).to_string()).collect())
    }

    /// An empty table whose headings were computed rather than written out.
    #[must_use]
    pub fn with_headers(caption: impl Into<String>, headers: Vec<String>) -> Self {
        Self {
            caption: caption.into(),
            headers,
            rows: Vec::new(),
        }
    }

    /// Append a row.
    pub fn push(&mut self, row: Vec<String>) {
        debug_assert_eq!(row.len(), self.headers.len(), "ragged table row");
        self.rows.push(row);
    }

    /// Render as GitHub-flavoured Markdown.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "{}\n", self.caption);
        let _ = writeln!(out, "| {} |", self.headers.join(" | "));
        let _ = writeln!(
            out,
            "|{}|",
            self.headers
                .iter()
                .map(|_| "---")
                .collect::<Vec<_>>()
                .join("|")
        );
        for row in &self.rows {
            let _ = writeln!(out, "| {} |", row.join(" | "));
        }
        out
    }
}

/// One piece of a section's human-readable half.
///
/// Prose and tables share one ordered list rather than living in two, because
/// the order they were written in *is* the order they should be read in:
/// setup, then numbers, then what the numbers mean. Two lists forced the
/// renderer to invent an interleaving, and it got it wrong for any section
/// whose interpretation belongs after its table.
#[derive(Debug, Clone)]
pub enum Block {
    /// A paragraph.
    Prose(String),
    /// A table.
    Table(Table),
}

/// One chapter of the report: what was done, the tables a person reads, and the
/// measurements the regression test reads.
#[derive(Debug, Clone)]
pub struct Section {
    /// Heading text.
    pub title: String,
    /// Prose and tables, in the order they should be rendered.
    pub blocks: Vec<Block>,
    /// The named numbers this section produced.
    pub data: Vec<Measurement>,
}

impl Section {
    /// An empty section.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            blocks: Vec::new(),
            data: Vec::new(),
        }
    }

    /// Add a paragraph.
    pub fn say(&mut self, paragraph: impl Into<String>) {
        self.blocks.push(Block::Prose(paragraph.into()));
    }

    /// Add a table.
    pub fn table(&mut self, table: Table) {
        self.blocks.push(Block::Table(table));
    }

    /// Record a measurement.
    pub fn record(&mut self, m: Measurement) {
        self.data.push(m);
    }
}

/// Every measurement the lab took, in one place.
#[derive(Debug, Clone, Default)]
pub struct Dataset {
    entries: Vec<Measurement>,
}

impl Dataset {
    /// Collect the measurements from a run's sections.
    ///
    /// Sorted by key and checked for duplicates: two measurements sharing a
    /// name would make the regression test's report ambiguous about which one
    /// moved, which is exactly the failure it exists to prevent.
    #[must_use]
    pub fn from_sections(sections: &[Section]) -> Self {
        let mut entries: Vec<Measurement> =
            sections.iter().flat_map(|s| s.data.iter().cloned()).collect();
        entries.sort_by(|a, b| a.key.cmp(&b.key));
        for pair in entries.windows(2) {
            assert_ne!(
                pair[0].key, pair[1].key,
                "two measurements share the key `{}`; a regression report could not \
                 say which one moved",
                pair[0].key
            );
        }
        Self { entries }
    }

    /// The measurements, sorted by key.
    #[must_use]
    pub fn entries(&self) -> &[Measurement] {
        &self.entries
    }

    /// How many measurements there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The machine-readable form: one `key<TAB>value<TAB>unit` line per
    /// measurement, sorted, newline-terminated.
    ///
    /// Tab-separated rather than JSON so that a `diff` of two of these is
    /// readable by a person, and so that parsing it needs no dependency — the
    /// regression test and `cargo xtask lab --check` both read it, and neither
    /// should have to pull in a parser to answer "did this number move".
    #[must_use]
    pub fn to_tsv(&self) -> String {
        let mut out = String::new();
        for m in &self.entries {
            let _ = writeln!(out, "{}\t{}\t{}", m.key, m.value, m.unit);
        }
        out
    }

    /// Parse the machine-readable form back. Blank lines and `#` comments are
    /// skipped so a pinned file can carry a header explaining itself.
    ///
    /// Returns the offending line on a malformed record rather than guessing:
    /// a fixture that half-parsed would silently drop the measurements it
    /// failed on, and those are exactly the ones a regression is hiding in.
    pub fn from_tsv(text: &str) -> Result<Self, String> {
        let mut entries = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split('\t');
            let (Some(key), Some(value)) = (fields.next(), fields.next()) else {
                return Err(format!("line {}: not a key/value record: {line:?}", n + 1));
            };
            let unit = fields.next().unwrap_or("");
            if fields.next().is_some() {
                return Err(format!("line {}: too many fields: {line:?}", n + 1));
            }
            entries.push(Measurement {
                key: key.to_string(),
                value: value.to_string(),
                // Units come from a closed set the lab itself emits; anything
                // else means the file was hand-edited, and the pinned file says
                // in its header not to.
                unit: match unit {
                    "ups" => "ups",
                    "deg" => "deg",
                    "units" => "units",
                    "ms" => "ms",
                    "n" => "n",
                    "" => "",
                    other => return Err(format!("line {}: unknown unit {other:?}", n + 1)),
                },
            });
        }
        entries.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(Self { entries })
    }
}

/// How one measurement differs between two datasets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The key is in the new dataset and not the pinned one.
    Added {
        /// The measurement's name.
        key: String,
        /// What it now reads.
        value: String,
    },
    /// The key is in the pinned dataset and not the new one.
    Removed {
        /// The measurement's name.
        key: String,
        /// What it used to read.
        was: String,
    },
    /// The key is in both and the value differs.
    Moved {
        /// The measurement's name.
        key: String,
        /// What it used to read.
        was: String,
        /// What it now reads.
        now: String,
    },
}

impl Change {
    /// The measurement's name, whichever kind of change this is.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::Added { key, .. } | Self::Removed { key, .. } | Self::Moved { key, .. } => key,
        }
    }

    /// One line, in the form a failing test prints.
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            Self::Added { key, value } => format!("  + {key}  (new, reads {value})"),
            Self::Removed { key, was } => format!("  - {key}  (gone, read {was})"),
            Self::Moved { key, was, now } => format!("  ~ {key}  {was} -> {now}"),
        }
    }
}

/// Every way `now` differs from `pinned`, in key order.
///
/// A merge over two sorted lists rather than a hash lookup, so the output order
/// is the key order and does not depend on a hasher.
#[must_use]
pub fn diff(pinned: &Dataset, now: &Dataset) -> Vec<Change> {
    let (a, b) = (pinned.entries(), now.entries());
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < a.len() || j < b.len() {
        match (a.get(i), b.get(j)) {
            (Some(x), Some(y)) if x.key == y.key => {
                if x.value != y.value || x.unit != y.unit {
                    out.push(Change::Moved {
                        key: x.key.clone(),
                        was: format!("{} {}", x.value, x.unit).trim_end().to_string(),
                        now: format!("{} {}", y.value, y.unit).trim_end().to_string(),
                    });
                }
                i += 1;
                j += 1;
            }
            (Some(x), Some(y)) if x.key < y.key => {
                out.push(Change::Removed {
                    key: x.key.clone(),
                    was: x.value.clone(),
                });
                i += 1;
            }
            (Some(_), Some(y)) => {
                out.push(Change::Added {
                    key: y.key.clone(),
                    value: y.value.clone(),
                });
                j += 1;
            }
            (Some(x), None) => {
                out.push(Change::Removed {
                    key: x.key.clone(),
                    was: x.value.clone(),
                });
                i += 1;
            }
            (None, Some(y)) => {
                out.push(Change::Added {
                    key: y.key.clone(),
                    value: y.value.clone(),
                });
                j += 1;
            }
            (None, None) => break,
        }
    }
    out
}

/// How many individual changes a report lists before it stops naming them.
///
/// A movement change can move a thousand measurements at once, and a thousand
/// lines of diff is not a report — it is a wall that gets scrolled past. The
/// grouped counts above the list stay complete however large the change is, so
/// nothing is hidden; only the enumeration is bounded, and it says so.
const MAX_LISTED: usize = 40;

/// The failure message: what moved, grouped by family, then named.
///
/// The grouping is the part that makes this legible. A change to
/// `air_accelerate` moves every number under `*.strafe.*` and `*.terminal.*`
/// and nothing else, and *that shape* is the diagnosis — far more useful than
/// the first forty individual lines, which all say the same thing. The families
/// are `<profile>.<section>`, which is one line per profile per measurement
/// family: enough to see the shape, few enough to read.
#[must_use]
pub fn summarise(changes: &[Change]) -> String {
    let mut out = String::new();
    if changes.is_empty() {
        return out;
    }

    // Moved first, and counted apart from the rest, because the two mean
    // completely different things to whoever is reading the failure. A *moved*
    // measurement is a movement change: something the mover does differently
    // than it did. An *added* or *removed* one is a fixture that has not been
    // re-blessed — which is what a new profile landing looks like, and is not a
    // regression at all. Both fail the gate, because a stale fixture stops
    // guarding, but running them together in one number would report the day
    // `experimental` lands as a thousand-measurement regression.
    let moved = changes
        .iter()
        .filter(|c| matches!(c, Change::Moved { .. }))
        .count();
    let added = changes
        .iter()
        .filter(|c| matches!(c, Change::Added { .. }))
        .count();
    let removed = changes
        .iter()
        .filter(|c| matches!(c, Change::Removed { .. }))
        .count();
    if moved > 0 {
        let _ = writeln!(
            out,
            "{moved} measurement(s) MOVED — the simulation does something \
             different than it did."
        );
    }
    if added > 0 || removed > 0 {
        let _ = writeln!(
            out,
            "{added} appeared and {removed} vanished — the fixture predates a \
             change to what is measured (a new profile, a new sweep) and needs \
             re-blessing. Not a regression on its own."
        );
    }
    let _ = writeln!(out);

    // A sorted Vec rather than a map: keys are sorted, so any prefix's members
    // are contiguous and a single pass merges them — and there is no hasher
    // whose iteration order could reach the output.
    let family = |key: &str| -> String { key.split('.').take(2).collect::<Vec<_>>().join(".") };
    let mut families: Vec<(String, usize)> = Vec::new();
    for change in changes {
        let f = family(change.key());
        match families.last_mut() {
            Some((name, count)) if *name == f => *count += 1,
            _ => families.push((f, 1)),
        }
    }
    families.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let _ = writeln!(out, "by measurement family:");
    for (name, count) in &families {
        let _ = writeln!(out, "  {count:>5}  {name}.*");
    }
    let _ = writeln!(out);

    for change in changes.iter().take(MAX_LISTED) {
        let _ = writeln!(out, "{}", change.render());
    }
    if changes.len() > MAX_LISTED {
        let _ = writeln!(
            out,
            "  … and {} more, all counted in the families above",
            changes.len() - MAX_LISTED
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ds(pairs: &[(&str, &str)]) -> Dataset {
        Dataset::from_sections(&[Section {
            title: String::new(),
            blocks: Vec::new(),
            data: pairs
                .iter()
                .map(|(k, v)| Measurement::label(*k, *v))
                .collect(),
        }])
    }

    #[test]
    fn a_dataset_round_trips_through_its_machine_readable_form() {
        let d = Dataset::from_sections(&[{
            let mut s = Section::new("t");
            s.record(Measurement::ups("cpm.terminal.ground", 320.0));
            s.record(Measurement::ms("cpm.window.double_jump", 400));
            s.record(Measurement::flag("vq3.overbounce.seen", false));
            s
        }]);
        let back = Dataset::from_tsv(&d.to_tsv()).expect("round trip");
        assert_eq!(back.entries(), d.entries());
    }

    #[test]
    fn a_comment_header_and_blank_lines_survive_parsing() {
        let text = "# pinned\n\na.b\t1.00\tups\n";
        let d = Dataset::from_tsv(text).expect("parse");
        assert_eq!(d.len(), 1);
        assert_eq!(d.entries()[0].key, "a.b");
    }

    #[test]
    fn the_diff_names_every_kind_of_change() {
        let pinned = ds(&[("a", "1"), ("b", "2"), ("c", "3")]);
        let now = ds(&[("a", "1"), ("b", "9"), ("d", "4")]);
        let changes = diff(&pinned, &now);
        assert_eq!(
            changes,
            vec![
                Change::Moved {
                    key: "b".into(),
                    was: "2".into(),
                    now: "9".into()
                },
                Change::Removed {
                    key: "c".into(),
                    was: "3".into()
                },
                Change::Added {
                    key: "d".into(),
                    value: "4".into()
                },
            ]
        );
        // The whole point: the failure message carries the name.
        assert!(changes[0].render().contains('b'));
    }

    #[test]
    fn an_identical_dataset_diffs_to_nothing() {
        let d = ds(&[("a", "1"), ("b", "2")]);
        assert!(diff(&d, &d).is_empty());
    }

    /// The property the whole regression test rests on: a large change is still
    /// legible, because the family counts are complete even when the
    /// enumeration is not.
    #[test]
    fn a_huge_change_is_summarised_rather_than_dumped() {
        let keys: Vec<String> = (0..500)
            .map(|i| format!("cpm.strafe.forward.entry{i:04}.gain_per_s"))
            .chain((0..3).map(|i| format!("vq3.ramp.deg{i:02}.normal_z")))
            .collect();
        let pinned = ds(&keys
            .iter()
            .map(|k| (k.as_str(), "1"))
            .collect::<Vec<_>>());
        let now = ds(&keys
            .iter()
            .map(|k| (k.as_str(), "2"))
            .collect::<Vec<_>>());

        let changes = diff(&pinned, &now);
        assert_eq!(changes.len(), 503);
        let report = summarise(&changes);

        // Every family is counted, in full.
        assert!(report.contains("500  cpm.strafe.*"), "{report}");
        assert!(report.contains("3  vq3.ramp.*"), "{report}");
        // The enumeration is bounded, and says so rather than trailing off.
        assert!(report.contains("and 463 more"), "{report}");
        assert!(report.lines().count() < 60, "the report is a wall: {report}");
        // All 503 are movement changes, so the headline says so and does not
        // mention a stale fixture.
        assert!(report.contains("503 measurement(s) MOVED"), "{report}");
        assert!(!report.contains("appeared"), "{report}");
    }

    /// A new profile landing must not read as a regression. Its measurements
    /// are *additions*, the canon ones are untouched, and the headline has to
    /// say which is which — otherwise the day `experimental` arrives the gate
    /// reports a thousand-measurement movement change and gets ignored.
    #[test]
    fn a_new_profile_reads_as_a_stale_fixture_and_not_as_a_regression() {
        let canon: Vec<String> = (0..20)
            .map(|i| format!("vq3.strafe.forward.entry{i:04}.gain_per_s"))
            .collect();
        let pinned = ds(&canon.iter().map(|k| (k.as_str(), "1")).collect::<Vec<_>>());

        let mut with_new: Vec<(String, &str)> =
            canon.iter().map(|k| (k.clone(), "1")).collect();
        with_new.extend(
            (0..20).map(|i| (format!("experimental.strafe.forward.entry{i:04}.gain_per_s"), "2")),
        );
        let now = ds(&with_new
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect::<Vec<_>>());

        let changes = diff(&pinned, &now);
        assert_eq!(changes.len(), 20);
        assert!(changes.iter().all(|c| matches!(c, Change::Added { .. })));

        let report = summarise(&changes);
        assert!(report.contains("20 appeared and 0 vanished"), "{report}");
        assert!(!report.contains("MOVED"), "{report}");
        assert!(report.contains("20  experimental.strafe.*"), "{report}");
    }

    #[test]
    fn summarising_nothing_says_nothing() {
        assert!(summarise(&[]).is_empty());
    }
}
