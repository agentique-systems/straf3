//! The six measurement families criterion 1 asks for.
//!
//! Each submodule owns one, returns a [`Section`], and knows nothing about the
//! others. Adding a seventh is adding a module and a line to [`all`].
//!
//! # Why the profile list is an argument
//!
//! It used to be a function returning the two canon profiles, with a note
//! saying that `PhysicsProfile::experimental()` would one day be appended to it.
//! It is a parameter now, and the reason is G2.
//!
//! `docs/movement-canon.md` §1.1's second gate asks for the *whole* published
//! measurement set, re-taken under a candidate profile and diffed against the
//! control — "every measurement that does not involve the mechanic's own input
//! must be bit-identical". That is not a column added to a table. It is this
//! whole family of measurements, run again, under a profile that is not in the
//! report. So [`vocabulary`] takes the profiles it is to measure and [`all`]
//! passes it the report's two; the candidate harness passes it one candidate at
//! a time.
//!
//! No measurement function names a profile, no table is shaped around there
//! being two, and no key format assumes a profile's name is three letters —
//! which is what makes the second caller possible at all.

use straf3_sim::PhysicsProfile;

use crate::dataset::Section;

pub mod crossvalidate;
pub mod overbounce;
pub mod ramps;
pub mod steps;
pub mod strafe;
pub mod substepping;
pub mod terminal;
pub mod windows;

/// The profiles the published report measures, in report order.
///
/// Canon only. The candidates are deliberately **not** here: they are measured
/// against a control in section 8 rather than published as two more columns of
/// canon's tables, because a column beside `vq3` and `cpm` reads as a third
/// ruleset the game has, and none of the three has earned that.
#[must_use]
pub fn profiles() -> Vec<(&'static str, PhysicsProfile)> {
    vec![
        ("vq3", PhysicsProfile::vq3()),
        ("cpm", PhysicsProfile::cpm()),
    ]
}

/// The six measurement families, taken under whichever profiles are asked for.
///
/// Cross-validation is not here: it restates another seat's published `vq3`
/// numbers, so it means nothing under a profile list that does not contain
/// `vq3`, and re-running it per candidate would report the same agreement three
/// more times.
#[must_use]
pub fn vocabulary(profiles: &[(&str, PhysicsProfile)]) -> Vec<Section> {
    vec![
        strafe::measure(profiles),
        windows::measure(profiles),
        ramps::measure(profiles),
        overbounce::measure(profiles),
        steps::measure(profiles),
        terminal::measure(profiles),
    ]
}

/// Take every measurement, in report order.
///
/// The cross-validation section is built last and from the others' output
/// rather than from its own measurements: it exists to put *this report's*
/// numbers beside another seat's, and one that recomputed them would be
/// checking a second implementation nobody is being asked to trust.
#[must_use]
pub fn all() -> Vec<Section> {
    let canon = profiles();
    let canon: Vec<(&str, PhysicsProfile)> = canon.iter().map(|(n, p)| (*n, *p)).collect();
    let mut sections = vocabulary(&canon);
    sections.push(crossvalidate::measure(&sections));
    sections.push(substepping::measure());
    sections
}

/// A number in a measurement key, zero-padded so that lexicographic order is
/// numeric order.
///
/// Without this, `angle10` sorts before `angle5` and both the pinned file and
/// every diff over it read as though the rows had been shuffled.
#[must_use]
pub fn pad(value: u32, width: usize) -> String {
    format!("{value:0width$}")
}
