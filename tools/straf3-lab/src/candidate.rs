//! The three candidate mechanics, expressed as runs rather than as opinions.
//!
//! # What this module is for
//!
//! `docs/movement-canon.md` Part 1 sets eight gates and seven weighed criteria a
//! mechanic must face before it enters canon, and §1.2 defines **one sweep** that
//! four of the weighed criteria are scored from. This module is that sweep. The
//! report section that scores it is [`crate::measure::candidates`]; everything
//! here is measurement, and nothing here decides anything.
//!
//! # The three rules the evidence has to obey, and how each is obeyed here
//!
//! §1.6 states them. They are not commentary — each one changed the shape of the
//! code below.
//!
//! 1. **Measured under the integration canon will ship with.** Nothing in this
//!    module chooses an integration; it runs [`straf3_sim::step`], so the answer
//!    to "was this measured under sub-stepping" is whatever `straf3-sim` does on
//!    the tree the document's header names. `docs/movement-lab.md`'s
//!    sub-stepping section publishes which.
//! 2. **One variable.** Each candidate gets its *own* profile carrying that
//!    mechanic's constants and nothing else ([`Mechanic::profile`]), rather than
//!    `PhysicsProfile::experimental()`, which carries all three at once. A number
//!    measured under `experimental` is a number about three mechanics.
//!    `the_candidate_profiles_change_one_mechanic_each` holds this to
//!    `experimental()`'s own values, so a candidate profile cannot drift into
//!    being a fourth tuning nobody agreed to.
//! 3. **Against a stated control.** Every run here is taken **twice**, in
//!    lockstep: the same command stream, from the same state, under the candidate
//!    profile and under [`PhysicsProfile::cpm`] ([`Paired`]). The control is
//!    therefore not "a similar run" — it is the same run with the mechanic
//!    absent, and the difference between the two is the mechanic and cannot be
//!    anything else.
//!
//! Running the pair in lockstep buys one more thing, and it is what settles G3:
//! the command index at which the two velocities first differ is recorded
//! ([`Paired::diverged_at`]). A mechanic whose effect lands on the command of the
//! press diverges *on* that command. A wind-up diverges later, and the number
//! says which.
//!
//! # Why the aim is held for the whole run rather than only on the press
//!
//! The swept parameter §1.2 asks for is "the wish direction relative to the
//! current velocity". Held only on the invoking command, that measures the
//! mechanic in isolation — but W3 asks whether the mechanic *composes* with
//! strafejumping, and a run that stops strafing after one command has no
//! strafejump to compose with. So the aim is re-aimed off the current velocity
//! every command, exactly as [`crate::harness::strafe_for`] does, and the control
//! holds the same angle. At aim 0 the run is a straight-ahead run; at the
//! optimum it is a strafe run; the difference between candidate and control at
//! any aim is still the mechanic alone, because the control holds that aim too.
//!
//! # Determinism
//!
//! Same rules as the rest of the lab: no clock, no randomness, no parallelism,
//! no container whose iteration order is not part of its type, and no libm on any
//! path that feeds a number — headings come from [`crate::num::atan2_degrees`].

use straf3_collision::HullWorld;
use straf3_sim::num::{Scalar, s, vec3};
use straf3_sim::{Buttons, PhysicsProfile, SimState, UserCmd, step};

use crate::geometry;
use crate::harness::{Axis, HZ, MS, holding, yaw_for};
use crate::num::{heading_degrees, horizontal_speed};

/// Aim resolution, in degrees. §1.2: "the wish direction relative to the current
/// velocity, at 5°".
pub const AIM_STEP: f32 = 5.0;

/// How many aims that is, sweeping the whole circle.
///
/// The whole circle rather than a half, even though six of the seven contexts
/// are mirror-symmetric about the XZ plane: [`geometry::corner`] is not, and a
/// sweep that assumed a symmetry the geometry does not have would report the
/// corner's answer for the wrong side.
pub const AIMS: usize = 72;

/// Entry speeds, in ups. §1.2's set: 320 is the ground cap, 1000 is above what a
/// bunnyhop reaches, and the candidates' entry thresholds sit at 400 so the band
/// has to straddle it.
pub const ENTRY_SPEEDS: &[f32] = &[320.0, 400.0, 500.0, 640.0, 800.0, 1000.0];

/// The horizon, in commands: §1.2's "1 second after the invocation window
/// closes".
pub const HORIZON: usize = HZ;

/// How long a prefix may run looking for a mechanic's arming event before the
/// cell is declared unreachable.
///
/// Four seconds. A wall the player never reaches and a landing that never
/// happens are both real answers — the corner is the only context with a wall in
/// it — and this bound is what turns them into an answer rather than a hang.
const PREFIX_CAP: usize = 4 * HZ;

/// What kind of place a context is. §1.2 scores W4 on how many *kinds* a
/// mechanic pays in, not just how many contexts, because three ramp angles are
/// one situation measured three times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Something to stand on.
    Surface,
    /// Something to run off or up.
    Edge,
    /// Something to run into.
    Wall,
    /// Something to duck under.
    Ceiling,
}

impl Kind {
    /// The name this kind carries in the report.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Surface => "surface",
            Self::Edge => "edge",
            Self::Wall => "wall",
            Self::Ceiling => "ceiling",
        }
    }
}

/// One of §1.2's seven contexts: a world, and where a run into it starts.
pub struct Context {
    /// Name in a measurement key.
    pub name: &'static str,
    /// Which kind it counts as for W4.
    pub kind: Kind,
    /// The world, from [`straf3_collision::testbed`] via [`crate::geometry`].
    pub world: HullWorld,
    /// Where the context's feature sits on X: the ramp foot, the step riser, the
    /// ledge edge, the wall face. Every `testbed` world puts its feature at x=0
    /// except [`geometry::corner`], whose walls stand at
    /// `testbed::CORNER_INNER`.
    pub feature_x: Scalar,
    /// Z of the surface the player starts on.
    pub surface_z: Scalar,
    /// How high above that surface the top of a hop is, in this world.
    ///
    /// `jump_velocity² / (2·gravity)` where nothing is in the way, which is
    /// 45.5625 units under canon. Under a ceiling it is whatever the headroom
    /// allows, because a hop that would end inside the slab is not a hop the
    /// player can take.
    pub hop_apex: Scalar,
    /// Whether a standing hull does not fit in this world at all.
    ///
    /// True only for [`geometry::ceiling_at`], whose slab spans the whole world:
    /// a floor at 0 and a ceiling at 48 leave 48 units, and a standing player is
    /// 56 tall. The consequence is measured rather than worked around — see
    /// [`Mechanic::CrouchSlide`]'s note on it.
    pub crouch_only: bool,
}

/// Height of the low ceiling, in units.
///
/// 48 is the value `straf3_collision::testbed::ceiling_at`'s own documentation
/// argues for: a crouched player spans 40 and a standing one 56, so 48 is the
/// only band that admits one and refuses the other.
const CEILING_Z: f32 = 48.0;

/// How high the top of an unobstructed hop is above the surface it left.
///
/// From the profile rather than written down: `v²/2g` is where a launch at
/// [`PhysicsProfile::jump_velocity`] runs out of climb under
/// [`PhysicsProfile::gravity`].
#[must_use]
pub fn hop_apex() -> Scalar {
    let p = Mechanic::control();
    p.jump_velocity * p.jump_velocity / (s(2.0) * p.gravity)
}

/// How long a fall from `apex` takes, in seconds.
///
/// `sqrt(2h/g)`. A square root rather than a libm call, so the answer is fixed
/// by IEEE 754 like everything else the lab publishes.
#[must_use]
pub fn fall_seconds(apex: Scalar) -> Scalar {
    (s(2.0) * apex / Mechanic::control().gravity).sqrt()
}

/// The seven contexts of §1.2, in the document's order.
///
/// Nothing new is built. The table in §1.2 names exactly these seven
/// constructors and this list is that table; a candidate whose case needed
/// geometry outside it would have to say so in the verdict, and none of the
/// three does.
#[must_use]
pub fn contexts() -> Vec<Context> {
    let full = hop_apex();
    // Under the ceiling, the top of a hop is the headroom and not the profile's
    // arithmetic: a crouched player's hull top sits 16 above the origin, which
    // rests 24.125 above the floor, so 48 − 16 − 24.125 leaves 7.875 units of
    // climb. Half a unit is kept in hand so a run starts below the slab rather
    // than against it.
    let ducked = s(CEILING_Z) - s(16.0) - geometry::resting_origin_z(s(0.0)) - s(0.5);
    vec![
        Context {
            name: "floor",
            kind: Kind::Surface,
            world: geometry::floor(),
            feature_x: s(0.0),
            surface_z: geometry::FLOOR_TOP,
            hop_apex: full,
            crouch_only: false,
        },
        Context {
            name: "ramp26",
            kind: Kind::Surface,
            world: geometry::ramp(s(26.0)),
            feature_x: s(0.0),
            surface_z: geometry::FLOOR_TOP,
            hop_apex: full,
            crouch_only: false,
        },
        Context {
            name: "ramp50",
            kind: Kind::Surface,
            world: geometry::ramp(s(50.0)),
            feature_x: s(0.0),
            surface_z: geometry::FLOOR_TOP,
            hop_apex: full,
            crouch_only: false,
        },
        Context {
            name: "step18",
            kind: Kind::Edge,
            world: geometry::step(s(18.0)),
            feature_x: geometry::STEP_RISER_X,
            surface_z: geometry::FLOOR_TOP,
            hop_apex: full,
            crouch_only: false,
        },
        Context {
            name: "ledge256",
            kind: Kind::Edge,
            world: geometry::ledge(s(256.0)),
            feature_x: geometry::LEDGE_EDGE_X,
            surface_z: geometry::FLOOR_TOP,
            hop_apex: full,
            crouch_only: false,
        },
        Context {
            name: "corner",
            kind: Kind::Wall,
            world: geometry::corner(),
            feature_x: geometry::CORNER_INNER,
            surface_z: geometry::FLOOR_TOP,
            hop_apex: full,
            crouch_only: false,
        },
        Context {
            name: "ceiling48",
            kind: Kind::Ceiling,
            world: geometry::ceiling_at(s(CEILING_Z)),
            feature_x: s(0.0),
            surface_z: geometry::FLOOR_TOP,
            hop_apex: ducked,
            crouch_only: true,
        },
    ]
}

/// One of the three mechanics under judgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mechanic {
    /// Crouch at speed on the ground: [`PhysicsProfile::slide_friction`] replaces
    /// [`PhysicsProfile::friction`] for [`PhysicsProfile::slide_duration_ms`].
    ///
    /// **Its one context is degenerate and that is a measurement, not a gap.**
    /// Under [`Context::crouch_only`] the player is crouched from the first
    /// command because a standing hull does not fit, so the crouch *edge* the
    /// slide is armed on can only ever land on command 0. The sweep still runs
    /// every timing; only command 0 fires, and the published availability count
    /// says one command rather than pretending to a window. A slide entered
    /// part-way down a low corridor would need a corridor with a mouth, which
    /// `testbed` does not have.
    CrouchSlide,
    /// A jump press in the air inside a window a landing opened: `PM_Accelerate`'s
    /// clamp with no acceleration limit, along the wish direction.
    Dash,
    /// A jump press in the air inside a window a wall touch opened:
    /// [`PhysicsProfile::jump_velocity`] vertically plus
    /// [`PhysicsProfile::wall_jump_velocity`] along the wall's normal.
    WallJump,
}

/// The three, in the order the verdicts are written in.
#[must_use]
pub fn mechanics() -> Vec<Mechanic> {
    vec![Mechanic::CrouchSlide, Mechanic::Dash, Mechanic::WallJump]
}

impl Mechanic {
    /// The name this mechanic carries in a measurement key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::CrouchSlide => "crouch_slide",
            Self::Dash => "dash",
            Self::WallJump => "wall_jump",
        }
    }

    /// The heading it carries in the report.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::CrouchSlide => "Crouch slide",
            Self::Dash => "Dash",
            Self::WallJump => "Wall jump",
        }
    }

    /// The profile carrying **this mechanic's constants and nothing else**, on
    /// top of the control.
    ///
    /// §1.6.2, and the same rule `PhysicsProfile::experimental()`'s own doc
    /// comment gives for being `..Self::cpm()`. Every value below is copied from
    /// `experimental()` rather than chosen here; the test at the foot of this
    /// module holds them equal.
    #[must_use]
    pub fn profile(self) -> PhysicsProfile {
        let e = PhysicsProfile::experimental();
        match self {
            Self::CrouchSlide => PhysicsProfile {
                slide_entry_speed: e.slide_entry_speed,
                slide_friction: e.slide_friction,
                slide_duration_ms: e.slide_duration_ms,
                ..Self::control()
            },
            Self::Dash => PhysicsProfile {
                dash_speed: e.dash_speed,
                dash_window_ms: e.dash_window_ms,
                ..Self::control()
            },
            Self::WallJump => PhysicsProfile {
                wall_jump_velocity: e.wall_jump_velocity,
                wall_contact_window_ms: e.wall_contact_window_ms,
                wall_normal_max: e.wall_normal_max,
                ..Self::control()
            },
        }
    }

    /// The control every candidate is measured against: canon, unmodified.
    #[must_use]
    pub fn control() -> PhysicsProfile {
        PhysicsProfile::cpm()
    }

    /// How many commands wide the invocation window is.
    ///
    /// For the dash and the wall jump this is the profile's own window divided
    /// by the command duration, so the sweep covers exactly the commands the
    /// mechanic can be invoked on. The slide has no armed window — it is
    /// available whenever the player is grounded above
    /// [`PhysicsProfile::slide_entry_speed`], which is a speed condition and
    /// therefore decays — so it gets a fixed nominal window and the *measured*
    /// availability is published beside it.
    #[must_use]
    pub fn window_commands(self) -> usize {
        let p = self.profile();
        match self {
            // 256 ms. Long enough to cover the decay from 1000 ups through the
            // 400 ups threshold at canon friction (about 14 commands) with room
            // to spare, so the sweep never truncates the real availability.
            Self::CrouchSlide => 32,
            Self::Dash => usize::from(p.dash_window_ms / MS),
            Self::WallJump => usize::from(p.wall_contact_window_ms / MS),
        }
    }

    /// The earliest command after the anchor on which invocation is possible at
    /// all, as a matter of the input rules rather than of the geometry.
    ///
    /// Two for the dash, and the reason is the input language rather than a
    /// timer: the anchor is the landing, the player spends the next command's
    /// jump press on the ordinary jump that gets them airborne, and `jump_held`
    /// then refuses the following press until the input has been released for a
    /// command. One press buys one thing.
    #[must_use]
    pub const fn earliest(self) -> usize {
        match self {
            Self::CrouchSlide | Self::WallJump => 0,
            Self::Dash => 2,
        }
    }

    /// Whether a run reaches this mechanic from the air.
    ///
    /// The slide is a ground technique — `check_slide` is reached only from
    /// `PM_WalkMove` — and the other two are reached only from `PM_AirMove`.
    #[must_use]
    pub const fn starts_airborne(self) -> bool {
        match self {
            Self::CrouchSlide => false,
            Self::Dash | Self::WallJump => true,
        }
    }

    /// How far short of the context's feature a run starts. See [`entering`].
    #[must_use]
    pub fn approach(self, ctx: &Context, entry: Scalar) -> Scalar {
        match self {
            Self::CrouchSlide => s(64.0),
            Self::Dash | Self::WallJump => entry * fall_seconds(ctx.hop_apex),
        }
    }
}

/// A run of the candidate and its control, stepped in lockstep.
///
/// The two share a command *script* but not a command *stream*: each aims its
/// wish direction off its own velocity, so once the mechanic has fired the two
/// runs are genuinely different runs of the same policy — which is what a player
/// who dashed and a player who did not would each be doing.
#[derive(Debug, Clone, Copy)]
pub struct Paired {
    /// Where the candidate run ended.
    pub candidate: SimState,
    /// Where the control run ended.
    pub control: SimState,
    /// The first command index at which the two velocities differed, if they
    /// ever did. G3 reads this.
    pub diverged_at: Option<usize>,
}

impl Paired {
    /// Horizontal speed the candidate reached.
    #[must_use]
    pub fn candidate_speed(&self) -> Scalar {
        horizontal_speed(self.candidate.player.velocity)
    }

    /// Horizontal speed the control reached.
    #[must_use]
    pub fn control_speed(&self) -> Scalar {
        horizontal_speed(self.control.player.velocity)
    }

    /// What the mechanic was worth: the candidate's outcome less the control's,
    /// which is §1.6.3's "difference from a control" and the only number any
    /// criterion is scored on.
    #[must_use]
    pub fn gain(&self) -> Scalar {
        self.candidate_speed() - self.control_speed()
    }
}

/// One command of a run: hold `aim` degrees off the current velocity, and press
/// what the script says.
fn command(st: &SimState, aim: Scalar, jump: bool, crouch: bool) -> UserCmd {
    let want = heading_degrees(st.player.velocity) + aim;
    let mut buttons = Buttons::NONE;
    if jump {
        buttons = buttons.with(Buttons::JUMP);
    }
    if crouch {
        buttons = buttons.with(Buttons::CROUCH);
    }
    UserCmd {
        buttons,
        ..holding(Axis::Forward, yaw_for(Axis::Forward, want))
    }
}

/// Whether command `i` of a run presses jump and crouch.
///
/// This is the whole script, in one place, so that the candidate and the control
/// cannot be given different ones by accident.
fn presses(mech: Mechanic, ctx: &Context, i: usize, invoke_at: Option<usize>) -> (bool, bool) {
    let invoking = invoke_at == Some(i);
    match mech {
        // Crouch is a *tap*: pressed on the invoking command and released on the
        // next. Held, it would cap wish speed at `max_speed * duck_scale` for the
        // whole run and measure a posture rather than a technique. Under a
        // ceiling the player has no such choice, so crouch is held there — and
        // the edge the slide arms on therefore lands on command 0 whatever
        // `invoke_at` says, which is what makes that context's availability one
        // command wide.
        Mechanic::CrouchSlide => (false, if ctx.crouch_only { true } else { invoking }),
        // The command after the landing carries the ordinary jump that gets the
        // player airborne again — the bunnyhop rhythm the dash is spent out of —
        // and the invoking command carries the second press, which is why
        // `earliest` is 2 and not 0.
        Mechanic::Dash => (i == 0 || invoking, ctx.crouch_only),
        // The player is already airborne: the prefix jumped them into the wall.
        Mechanic::WallJump => (invoking, ctx.crouch_only),
    }
}

/// Step the candidate and its control forward together for `commands` commands.
fn walk_pair(
    mech: Mechanic,
    ctx: &Context,
    anchor: &SimState,
    aim: Scalar,
    invoke_at: Option<usize>,
    commands: usize,
) -> Paired {
    let candidate_profile = mech.profile();
    let control_profile = Mechanic::control();
    let mut out = Paired {
        candidate: *anchor,
        control: *anchor,
        diverged_at: None,
    };
    for i in 0..commands {
        let (jump, crouch) = presses(mech, ctx, i, invoke_at);
        let a = command(&out.candidate, aim, jump, crouch);
        let b = command(&out.control, aim, jump, crouch);
        out.candidate = step(&out.candidate, &a, &ctx.world, &candidate_profile);
        out.control = step(&out.control, &b, &ctx.world, &control_profile);
        if out.diverged_at.is_none() && out.candidate.player.velocity != out.control.player.velocity
        {
            out.diverged_at = Some(i);
        }
    }
    out
}

/// Where a run starts, and why it starts there.
///
/// # Every run is placed so the arming event happens at the feature
///
/// This is a choice and it is the most consequential one in the module, so it
/// is stated rather than buried. Each `testbed` world is a feature — a ramp
/// foot, a riser, a ledge edge, a wall — surrounded by thousands of units of
/// identical flat approach. A run placed anywhere else invokes the mechanic on
/// that approach, and then all seven contexts return the same number and W4's
/// "useful in more than one situation" is scored on a plain measured seven
/// times. So:
///
/// - a **crouch slide** is armed by a press the player chooses, so the run
///   starts on the ground 64 units short of the feature and the whole 32-command
///   window straddles it at every entry speed;
/// - a **dash** is armed by the landing that ends a hop, so the run starts at
///   the top of a hop, `entry · sqrt(2·apex/g)` units short of the feature —
///   one full fall — and the landing that arms it happens there;
/// - a **wall jump** is armed by a touch, and a hop descending onto the feature
///   passes through every height on the way, so it starts the same way. A riser
///   18 units tall is met near its base and a 512-unit wall anywhere.
///
/// The airborne starts set [`straf3_sim::PlayerState::left_ground_by_jumping`],
/// because a player at the top of a hop got there by jumping and the dash's
/// window is provenance-gated on exactly that. Nothing else about the state is
/// invented: the position is a height a jump reaches, the vertical velocity is
/// zero because that is what an apex is, and the control run starts from the
/// same state.
#[must_use]
pub fn entering(mech: Mechanic, ctx: &Context, entry: Scalar) -> SimState {
    let airborne = mech.starts_airborne();
    let x = ctx.feature_x - mech.approach(ctx, entry);
    let mut z = geometry::resting_origin_z(ctx.surface_z);
    if airborne {
        z += ctx.hop_apex;
    }
    let mut st = SimState::spawned_at(vec3(x, s(0.0), z), s(0.0));
    st.player.velocity = vec3(entry, s(0.0), s(0.0));
    if airborne {
        st.player.left_ground_by_jumping = true;
    }
    st
}

/// The state a mechanic's invocation window opens from, and how far into the run
/// that was.
#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    /// The state the window opens from.
    pub state: SimState,
    /// Commands spent reaching it.
    pub commands: usize,
}

/// Find the command the mechanic's window opens on, under the candidate profile.
///
/// # The anchor is the state the arming command *ended* in, not the one it began
///
/// `PmoveSingle` probes for ground once at the top of a command and **again
/// after moving**, "so the state a caller observes describes where the player
/// ended up, not where they began" (`step.rs`). A player who is 0.9 units above
/// the floor at the top of a command is airborne for that command's whole
/// pipeline and grounded only in the trailing probe — which is where
/// `dash_ms` is armed. An anchor taken one command earlier therefore hands the
/// script a state whose window has not opened yet, and every press it makes is
/// spent on an ordinary jump. That was the first version of this function and
/// the sweep it produced reported the dash as never available; it is written
/// down because the mistake is invisible in the output, which reads exactly like
/// a mechanic that does not fire.
///
/// # Why the prefix runs under the candidate profile
///
/// The arming event is the candidate's: `wall_contact_ms` is never written under
/// canon, so a control run could not tell the sweep where the wall is. Nothing
/// is invoked during a prefix, so the two profiles' motion is identical up to
/// here — which is G2's claim, and it is re-checked on every cell rather than
/// assumed, because the control continues from the *same* state the candidate
/// reached.
#[must_use]
pub fn anchor(mech: Mechanic, ctx: &Context, entry: Scalar, aim: Scalar) -> Option<Anchor> {
    let profile = mech.profile();
    let start = entering(mech, ctx, entry);
    match mech {
        // The window is open from the first command: a crouch press at speed is
        // available immediately, and the run starts on the ground.
        Mechanic::CrouchSlide => Some(Anchor {
            state: start,
            commands: 0,
        }),
        // The landing that arms the window, in the state it left behind.
        Mechanic::Dash => {
            let mut st = start;
            for i in 0..PREFIX_CAP {
                st = step(
                    &st,
                    &command(&st, aim, false, ctx.crouch_only),
                    &ctx.world,
                    &profile,
                );
                if st.player.ground.is_grounded() {
                    return Some(Anchor {
                        state: st,
                        commands: i + 1,
                    });
                }
            }
            None
        }
        // The command on which the slide solver first clipped the player against
        // a plane flat enough to count as a wall — and it has to still be
        // airborne, because the wall jump is reached only from `PM_AirMove`. A
        // player who has landed and is leaning on a wall has wall contact
        // recorded and no way to spend it.
        Mechanic::WallJump => {
            let mut st = start;
            for i in 0..PREFIX_CAP {
                st = step(
                    &st,
                    &command(&st, aim, false, ctx.crouch_only),
                    &ctx.world,
                    &profile,
                );
                if st.player.timers.wall_contact_ms > 0 && !st.player.ground.is_grounded() {
                    return Some(Anchor {
                        state: st,
                        commands: i + 1,
                    });
                }
            }
            None
        }
    }
}

/// One (context, entry speed) cell of the sweep.
pub struct Cell {
    /// Whether the mechanic's window ever opened here at all.
    pub reachable: bool,
    /// Commands spent reaching the window, at aim 0.
    pub anchor_commands: usize,
    /// Where the player was when the window opened, at aim 0.
    pub anchor_x: Scalar,
    /// Horizontal speed at the anchor, at aim 0. Not the nominal entry speed:
    /// the prefix costs whatever the geometry costs.
    pub anchor_speed: Scalar,
    /// Candidate outcome, indexed `[timing][aim]`. Horizontal speed at the
    /// horizon.
    pub candidate: Vec<Vec<Scalar>>,
    /// Control outcome, same indexing. §1.6.3's stated control.
    pub control: Vec<Vec<Scalar>>,
    /// Whether the mechanic actually fired, same indexing: the two runs diverged
    /// on exactly the invoking command.
    pub fired: Vec<Vec<bool>>,
    /// Commands between the invoking command and the first command the two runs
    /// differ on, over every cell entry where it fired. G3 fails on any non-zero.
    pub worst_latency: usize,
}

impl Cell {
    /// An empty cell, for a context the mechanic never reached.
    fn unreachable() -> Self {
        Self {
            reachable: false,
            anchor_commands: 0,
            anchor_x: s(0.0),
            anchor_speed: s(0.0),
            candidate: Vec::new(),
            control: Vec::new(),
            fired: Vec::new(),
            worst_latency: 0,
        }
    }

    /// Whether the mechanic fired anywhere in this cell.
    #[must_use]
    pub fn available(&self) -> bool {
        self.fired.iter().any(|row| row.iter().any(|f| *f))
    }

    /// How many timings it fired on, at the best aim's column — the measured
    /// availability window, in commands.
    #[must_use]
    pub fn available_commands(&self) -> usize {
        (0..self.fired.len())
            .filter(|t| self.fired[*t].iter().any(|f| *f))
            .count()
    }

    /// The best `(timing, aim, gain)` the sweep found, over the cells where the
    /// mechanic fired.
    #[must_use]
    pub fn best(&self) -> Option<(usize, usize, Scalar)> {
        let mut best: Option<(usize, usize, Scalar)> = None;
        for t in 0..self.fired.len() {
            for a in 0..AIMS {
                if !self.fired[t][a] {
                    continue;
                }
                let gain = self.candidate[t][a] - self.control[t][a];
                if best.is_none_or(|(_, _, g)| gain > g) {
                    best = Some((t, a, gain));
                }
            }
        }
        best
    }

    /// The naive play §1.2 W1 and §1.1 G5(b) are scored on: invoke at the first
    /// command it is available, aimed along the current heading.
    #[must_use]
    pub fn naive(&self) -> Option<(usize, Scalar)> {
        for t in 0..self.fired.len() {
            if self.fired[t][0] {
                return Some((t, self.candidate[t][0] - self.control[t][0]));
            }
        }
        None
    }

    /// The best absolute outcome the *control* reached anywhere in the sweep:
    /// the existing vocabulary given the same freedom of aim and timing.
    #[must_use]
    pub fn control_best(&self) -> Scalar {
        let mut best = Scalar::NEG_INFINITY;
        for row in &self.control {
            for v in row {
                if *v > best {
                    best = *v;
                }
            }
        }
        best
    }

    /// The best absolute outcome the candidate reached where it fired.
    #[must_use]
    pub fn candidate_best(&self) -> Option<Scalar> {
        let (t, a, _) = self.best()?;
        Some(self.candidate[t][a])
    }

    /// The best gain available without changing aim from the current heading:
    /// the mechanic used *instead of* the existing technique, for W3's chain
    /// gain.
    #[must_use]
    pub fn best_without_strafing(&self) -> Option<Scalar> {
        let mut best: Option<Scalar> = None;
        for t in 0..self.fired.len() {
            if self.fired[t][0] {
                let v = self.candidate[t][0];
                if best.is_none_or(|b| v > b) {
                    best = Some(v);
                }
            }
        }
        best
    }
}

/// Sweep one (mechanic, context, entry speed) cell.
///
/// §1.2's sweep, once: every invocation timing across the window at one-command
/// (8 ms) resolution, against every aim at 5°, each run paired with its control.
#[must_use]
pub fn sweep(mech: Mechanic, ctx: &Context, entry: Scalar) -> Cell {
    let window = mech.window_commands();
    let total = window + HORIZON;

    // The anchor depends on the aim, because the aim is held from the first
    // command of the run and therefore steers the approach. Found once per aim
    // and reused across every timing, which is what keeps the sweep affordable.
    let mut anchors: Vec<Option<Anchor>> = Vec::with_capacity(AIMS);
    for a in 0..AIMS {
        anchors.push(anchor(mech, ctx, entry, s(a as f32 * AIM_STEP)));
    }
    let Some(reference) = anchors[0] else {
        return Cell::unreachable();
    };

    let mut cell = Cell {
        reachable: true,
        anchor_commands: reference.commands,
        anchor_x: reference.state.player.origin.x,
        anchor_speed: horizontal_speed(reference.state.player.velocity),
        candidate: Vec::with_capacity(window),
        control: Vec::with_capacity(window),
        fired: Vec::with_capacity(window),
        worst_latency: 0,
    };

    for t in 0..window {
        let mut candidate_row = Vec::with_capacity(AIMS);
        let mut control_row = Vec::with_capacity(AIMS);
        let mut fired_row = Vec::with_capacity(AIMS);
        for (a, anchor) in anchors.iter().enumerate() {
            let Some(anchor) = anchor else {
                candidate_row.push(s(0.0));
                control_row.push(s(0.0));
                fired_row.push(false);
                continue;
            };
            let invoke_at = if t < mech.earliest() { None } else { Some(t) };
            let run = walk_pair(
                mech,
                ctx,
                &anchor.state,
                s(a as f32 * AIM_STEP),
                invoke_at,
                total,
            );
            let fired = run.diverged_at.is_some();
            if let (Some(at), Some(d)) = (invoke_at, run.diverged_at) {
                cell.worst_latency = cell.worst_latency.max(d.saturating_sub(at));
            }
            candidate_row.push(run.candidate_speed());
            control_row.push(run.control_speed());
            fired_row.push(fired);
        }
        cell.candidate.push(candidate_row);
        cell.control.push(control_row);
        cell.fired.push(fired_row);
    }
    cell
}

/// A run that never invokes the mechanic at all, for the gates that ask what
/// happens to a player who does not use it.
///
/// G5(a) reads this: a player who never performs the arming event must never
/// find the mechanic available.
#[must_use]
pub fn never_invoking(mech: Mechanic, ctx: &Context, entry: Scalar, commands: usize) -> Paired {
    walk_pair(
        mech,
        ctx,
        &entering(mech, ctx, entry),
        s(0.0),
        None,
        commands,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §1.6.2, enforced rather than asserted in prose: each candidate profile
    /// differs from the control by *that* mechanic's constants, and by nothing
    /// else — and those constants are `experimental()`'s own, not a fourth set
    /// invented here.
    #[test]
    fn the_candidate_profiles_change_one_mechanic_each() {
        let control = Mechanic::control();
        let e = PhysicsProfile::experimental();

        let slide = Mechanic::CrouchSlide.profile();
        assert_eq!(slide.slide_entry_speed, e.slide_entry_speed);
        assert_eq!(slide.slide_friction, e.slide_friction);
        assert_eq!(slide.slide_duration_ms, e.slide_duration_ms);
        assert_eq!(slide.dash_speed, control.dash_speed);
        assert_eq!(slide.wall_jump_velocity, control.wall_jump_velocity);

        let dash = Mechanic::Dash.profile();
        assert_eq!(dash.dash_speed, e.dash_speed);
        assert_eq!(dash.dash_window_ms, e.dash_window_ms);
        assert_eq!(dash.slide_duration_ms, control.slide_duration_ms);
        assert_eq!(dash.wall_contact_window_ms, control.wall_contact_window_ms);

        let wall = Mechanic::WallJump.profile();
        assert_eq!(wall.wall_jump_velocity, e.wall_jump_velocity);
        assert_eq!(wall.wall_contact_window_ms, e.wall_contact_window_ms);
        assert_eq!(wall.wall_normal_max, e.wall_normal_max);
        assert_eq!(wall.slide_duration_ms, control.slide_duration_ms);
        assert_eq!(wall.dash_speed, control.dash_speed);

        // And the three together are exactly `experimental()`: no candidate
        // constant is measured under a value the tree does not carry.
        let composed = PhysicsProfile {
            slide_entry_speed: slide.slide_entry_speed,
            slide_friction: slide.slide_friction,
            slide_duration_ms: slide.slide_duration_ms,
            dash_speed: dash.dash_speed,
            dash_window_ms: dash.dash_window_ms,
            wall_jump_velocity: wall.wall_jump_velocity,
            wall_contact_window_ms: wall.wall_contact_window_ms,
            wall_normal_max: wall.wall_normal_max,
            ..control
        };
        assert_eq!(composed, e);
    }

    /// The control profile really is canon, so that "the control" in every
    /// published number means the ruleset the game ships.
    #[test]
    fn the_control_is_canon() {
        assert_eq!(Mechanic::control(), PhysicsProfile::cpm());
        assert_eq!(Mechanic::control().slide_duration_ms, 0);
        assert_eq!(Mechanic::control().dash_window_ms, 0);
        assert_eq!(Mechanic::control().wall_contact_window_ms, 0);
    }

    /// The seven contexts are the seven §1.2 names, in its order, spanning its
    /// four kinds.
    #[test]
    fn the_contexts_are_the_seven_the_criteria_name() {
        let cs = contexts();
        assert_eq!(cs.len(), 7);
        let names: Vec<&str> = cs.iter().map(|c| c.name).collect();
        assert_eq!(
            names,
            vec![
                "floor",
                "ramp26",
                "ramp50",
                "step18",
                "ledge256",
                "corner",
                "ceiling48"
            ]
        );
        for kind in [Kind::Surface, Kind::Edge, Kind::Wall, Kind::Ceiling] {
            assert!(
                cs.iter().any(|c| c.kind == kind),
                "no context of kind {}",
                kind.key()
            );
        }
    }

    /// The ceiling context is the one that cannot be entered standing, and the
    /// sweep has to know it: a run that spawned a standing hull there would
    /// start inside geometry and measure a stuck player.
    #[test]
    fn only_the_ceiling_context_refuses_a_standing_hull() {
        for c in contexts() {
            assert_eq!(
                c.crouch_only,
                c.name == "ceiling48",
                "{} disagrees about whether a standing player fits",
                c.name
            );
        }
    }

    /// A candidate run and its control are the same run when nothing is
    /// invoked. This is G2's claim in miniature, and it is checked here as
    /// well as in the full-set diff because every outcome in the sweep rests on
    /// it: if a non-invoking command stream already differed, the difference
    /// between candidate and control would not be the mechanic.
    #[test]
    fn a_run_that_never_invokes_is_identical_under_both_profiles() {
        for mech in mechanics() {
            for ctx in contexts() {
                let run = never_invoking(mech, &ctx, s(640.0), 200);
                // The one exception, and it is a finding rather than an escape.
                // Under a ceiling that refuses a standing hull the player is
                // crouched from the first command because there is nowhere to
                // stand up, so the crouch press the slide is armed on is not
                // something they chose to press. There is no non-invoking run
                // to compare against there, and the sweep says so by publishing
                // an availability of one command in that context.
                if mech == Mechanic::CrouchSlide && ctx.crouch_only {
                    assert_eq!(
                        run.diverged_at,
                        Some(0),
                        "a crouch slide under a low ceiling should arm on the \
                         first command, because crouching there is forced"
                    );
                    continue;
                }
                assert_eq!(
                    run.diverged_at,
                    None,
                    "{} diverged from canon in {} without being invoked, at command {:?}",
                    mech.key(),
                    ctx.name,
                    run.diverged_at
                );
            }
        }
    }

    /// The sweep's grid is the one §1.2 specifies: 5° of aim over the whole
    /// circle, and one command of timing resolution.
    #[test]
    fn the_sweep_grid_is_the_one_the_criteria_ask_for() {
        assert!((AIM_STEP * AIMS as f32 - 360.0).abs() < 1e-3);
        assert_eq!(
            MS, 8,
            "the timing resolution is one command, and §1.2 wants 8 ms"
        );
        assert_eq!(Mechanic::Dash.window_commands(), 50);
        assert_eq!(Mechanic::WallJump.window_commands(), 25);
        assert_eq!(ENTRY_SPEEDS, &[320.0, 400.0, 500.0, 640.0, 800.0, 1000.0]);
    }
}
