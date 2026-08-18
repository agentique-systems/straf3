//! The six measurement families criterion 1 asks for.
//!
//! Each submodule owns one, returns a [`Section`], and knows nothing about the
//! others. Adding a seventh is adding a module and a line to [`all`].
//!
//! # Why the profile list is a function
//!
//! `experimental` does not exist in this tree yet — the candidates seat adds
//! `PhysicsProfile::experimental()` this wave, and criterion 5's assessment of
//! each new mechanic will be written against these numbers. When it lands,
//! every measurement below covers it by adding one line to [`profiles`]: no
//! measurement function names a profile, no table is shaped around there being
//! two, and no key format assumes a profile's name is three letters.

use straf3_sim::PhysicsProfile;

use crate::dataset::Section;

pub mod crossvalidate;
pub mod overbounce;
pub mod ramps;
pub mod steps;
pub mod strafe;
pub mod terminal;
pub mod windows;

/// The profiles under measurement, in report order.
///
/// Canon only, this wave. When `PhysicsProfile::experimental()` exists, append
/// `("experimental", PhysicsProfile::experimental())` here and every section
/// grows a column.
#[must_use]
pub fn profiles() -> Vec<(&'static str, PhysicsProfile)> {
    vec![
        ("vq3", PhysicsProfile::vq3()),
        ("cpm", PhysicsProfile::cpm()),
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
    let mut sections = vec![
        strafe::measure(),
        windows::measure(),
        ramps::measure(),
        overbounce::measure(),
        steps::measure(),
        terminal::measure(),
    ];
    sections.push(crossvalidate::measure(&sections));
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
