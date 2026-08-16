//! The compiler tested against `.map` text, not against its own internals.
//!
//! # Why the fixtures are generated and not pasted
//!
//! A brush face in a `.map` file is three points whose *winding order* decides
//! which way the face points, and getting one wrong inverts a brush — you fall
//! through the floor and cannot leave the level. Hand-writing six of those per
//! box, in a test, is a way to write a test that passes against the same
//! mistake the compiler makes. [`box_brush`] derives the winding from the
//! outward normal instead, so a fixture is right by construction and the tests
//! below can be about the compiler.

use super::*;

/// Format a Valve 220 face.
///
/// The three points are `o + u`, `o`, `o + v`, in that order, because Quake's
/// convention is `normal = (p0 - p1) × (p2 - p1)` — so `u × v` is the outward
/// normal, and every caller below picks `u` and `v` to make it so.
fn face(o: [i32; 3], u: [i32; 3], v: [i32; 3], texture: &str) -> String {
    let p =
        |a: [i32; 3], b: [i32; 3]| format!("( {} {} {} )", a[0] + b[0], a[1] + b[1], a[2] + b[2]);
    format!(
        "{} {} {} {texture} [ 1 0 0 0 ] [ 0 -1 0 0 ] 0 0.5 0.5\n",
        p(o, u),
        p(o, [0, 0, 0]),
        p(o, v),
    )
}

/// An axis-aligned solid box, as six Valve 220 faces.
///
/// The `u`/`v` pairs are chosen so `u × v` is each face's outward normal: `X ×
/// Y = Z`, `Y × Z = X`, `Z × X = Y`, and the negatives are the same pairs
/// swapped.
fn box_brush(mins: [i32; 3], maxs: [i32; 3], texture: &str) -> String {
    let (x, y, z) = ([64, 0, 0], [0, 64, 0], [0, 0, 64]);
    let lo = mins;
    let hi_x = [maxs[0], mins[1], mins[2]];
    let hi_y = [mins[0], maxs[1], mins[2]];
    let hi_z = [mins[0], mins[1], maxs[2]];

    let mut out = String::from("{\n");
    out.push_str(&face(lo, z, y, texture)); // -X
    out.push_str(&face(hi_x, y, z, texture)); // +X
    out.push_str(&face(lo, x, z, texture)); // -Y
    out.push_str(&face(hi_y, z, x, texture)); // +Y
    out.push_str(&face(lo, y, x, texture)); // -Z
    out.push_str(&face(hi_z, x, y, texture)); // +Z
    out.push_str("}\n");
    out
}

/// A point entity.
fn point_entity(classname: &str, keys: &[(&str, &str)]) -> String {
    let mut out = format!("{{\n\"classname\" \"{classname}\"\n");
    for (k, v) in keys {
        out.push_str(&format!("\"{k}\" \"{v}\"\n"));
    }
    out.push_str("}\n");
    out
}

/// A brush entity: keys, then brushes.
fn brush_entity(classname: &str, keys: &[(&str, &str)], brushes: &[String]) -> String {
    let mut out = format!("{{\n\"classname\" \"{classname}\"\n");
    for (k, v) in keys {
        out.push_str(&format!("\"{k}\" \"{v}\"\n"));
    }
    for b in brushes {
        out.push_str(b);
    }
    out.push_str("}\n");
    out
}

/// A small course: a floor, four walls, a spawn, and a timed start and finish.
///
/// Deliberately shaped like the thing this crate exists to compile — not a
/// single brush — so the tests exercise entity resolution and brush ordering
/// rather than one plane.
fn course() -> String {
    let mut world = String::new();
    world.push_str(&box_brush(
        [-512, -512, -64],
        [512, 512, 0],
        "base_floor/tile",
    )); // floor
    world.push_str(&box_brush(
        [-576, -512, 0],
        [-512, 512, 256],
        "base_wall/brick",
    )); // west
    world.push_str(&box_brush(
        [512, -512, 0],
        [576, 512, 256],
        "base_wall/brick",
    )); // east
    world.push_str(&box_brush(
        [-512, -576, 0],
        [512, -512, 256],
        "common/caulk",
    )); // south
    world.push_str(&box_brush([-512, 512, 0], [512, 576, 256], "common/caulk")); // north

    let mut map = brush_entity("worldspawn", &[("message", "test course")], &[world]);
    map.push_str(&point_entity(
        "info_player_deathmatch",
        &[("origin", "0 -256 24"), ("angle", "90")],
    ));
    map.push_str(&point_entity(
        "target_startTimer",
        &[("targetname", "t_start"), ("origin", "0 -128 32")],
    ));
    map.push_str(&point_entity(
        "target_stopTimer",
        &[("targetname", "t_stop"), ("origin", "0 384 32")],
    ));
    map.push_str(&brush_entity(
        "trigger_multiple",
        &[("target", "t_start")],
        &[box_brush(
            [-128, -160, 0],
            [128, -96, 128],
            "common/trigger",
        )],
    ));
    map.push_str(&brush_entity(
        "trigger_multiple",
        &[("target", "t_stop")],
        &[box_brush([-128, 352, 0], [128, 416, 128], "common/trigger")],
    ));
    map
}

fn compiled() -> CompiledMap {
    compile(&course()).expect("the fixture course must compile")
}

/// Whether a point is inside a compiled hull, asked the way the physics asks:
/// a zero-extent sweep that goes nowhere, and `start_solid` is the answer.
///
/// Not a plane loop written here. `compile_hull` adds bevel planes and permutes
/// the list, so "inside" is a question only the tracer can answer without
/// drifting from what the player will actually experience.
fn inside(hull: &Hull, point: Vec3) -> bool {
    let sweep = straf3_sim::world::Sweep {
        start: point,
        end: point,
        half_extents: Vec3::ZERO,
        center_offset: Vec3::ZERO,
    };
    let mut trace = straf3_sim::world::Trace::clear();
    straf3_collision::trace_hull(hull, &sweep, &mut trace);
    trace.start_solid
}

#[test]
fn an_empty_source_compiles_to_an_empty_map() {
    let map = compile("").expect("empty is not malformed");
    assert_eq!(map.spawn, Vec3::ZERO);
    assert!(map.hulls.is_empty() && map.triggers.is_empty());
    assert!(map.mesh.vertices.is_empty());
}

#[test]
fn nonsense_is_a_parse_error_and_says_where() {
    let err = compile("{ this is not a map").unwrap_err();
    match err {
        CompileError::Parse(msg) => assert!(!msg.is_empty(), "the parser's message is passed on"),
        other => panic!("expected a parse error, got {other:?}"),
    }
}

#[test]
fn the_course_compiles_to_one_hull_per_solid_brush() {
    let map = compiled();
    assert_eq!(map.hulls.len(), 5, "floor plus four walls");
    for hull in &map.hulls {
        assert_eq!(hull.planes.len(), 6, "a box is six half-spaces");
    }
    assert!(map.warnings.is_empty(), "unexpected: {:?}", map.warnings);
}

#[test]
fn a_floor_brush_has_an_exactly_upward_face() {
    let map = compiled();
    let floor = &map.hulls[0];
    let up = floor
        .planes
        .iter()
        .find(|p| p.normal == Vec3::Z)
        .expect("the floor's top face must be exactly +Z, not 0.9999999");
    assert_eq!(up.dist, 0.0, "the floor's top is at z = 0");
    // …and the hull agrees about where its inside is.
    assert!(inside(floor, Vec3::new(0.0, 0.0, -32.0)));
    assert!(!inside(floor, Vec3::new(0.0, 0.0, 1.0)));
}

#[test]
fn the_floors_bounds_are_tight_on_its_own_corners() {
    // Not padded. `trace_hull` widens the broadphase reject by
    // SURFACE_CLIP_EPSILON itself, exactly as Q3's `CM_BoundsIntersect` does,
    // so padding here would be a second epsilon on top of that one.
    let map = compiled();
    let floor = &map.hulls[0];
    assert_eq!(floor.mins, Vec3::new(-512.0, -512.0, -64.0));
    assert_eq!(floor.maxs, Vec3::new(512.0, 512.0, 0.0));
}

#[test]
fn the_spawn_is_read_and_lifted_an_eighth_off_the_floor() {
    let map = compiled();
    assert_eq!(map.spawn, Vec3::new(0.0, -256.0, 24.125));
    assert_eq!(map.spawn_yaw, 90.0);
    assert_eq!(map.spawns.len(), 1);
    // The lift is the whole difference: the entity is where the hull rests
    // exactly on the floor, and exactly on is inside.
    assert!(!inside(
        &map.hulls[0],
        map.spawn - Vec3::new(0.0, 0.0, 24.0)
    ));
}

#[test]
fn a_map_with_geometry_and_nowhere_to_stand_is_an_error() {
    let map = brush_entity(
        "worldspawn",
        &[],
        &[box_brush([-64, -64, -16], [64, 64, 0], "base_floor/tile")],
    );
    assert_eq!(compile(&map), Err(CompileError::NoPlayerSpawn));
}

#[test]
fn the_timer_triggers_are_found_through_their_targets() {
    let map = compiled();
    assert_eq!(map.triggers.len(), 2);
    assert_eq!(map.triggers[0].kind, TriggerKind::Start);
    assert_eq!(map.triggers[1].kind, TriggerKind::Finish);
    assert!(map.has_timing());
    assert_eq!(map.triggers_of(TriggerKind::Start).count(), 1);
}

#[test]
fn a_trigger_volume_is_not_solid_and_still_knows_where_it_is() {
    let map = compiled();
    let start = &map.triggers[0];
    // It is not part of the world: running through it is not blocked.
    assert_eq!(map.hulls.len(), 5, "the trigger brush is not a solid");
    // The player crosses it as a box, not as a point — a 30x30x56 hull whose
    // centre is 4 above the origin, straight from the profile the sim uses.
    assert!(start.contains_point(Vec3::new(0.0, -128.0, 32.0)));
    assert!(!start.contains_point(Vec3::new(0.0, 0.0, 32.0)));
    // Standing just short of the volume, the hull's front face is already in it.
    let half = Vec3::new(15.0, 15.0, 28.0);
    assert!(start.intersects_box(Vec3::new(0.0, -170.0, 32.0), half));
    assert!(!start.intersects_box(Vec3::new(0.0, -260.0, 32.0), half));
}

#[test]
fn a_map_with_no_timers_says_so_rather_than_pretending() {
    let mut map = brush_entity(
        "worldspawn",
        &[],
        &[box_brush([-64, -64, -16], [64, 64, 0], "base_floor/tile")],
    );
    map.push_str(&point_entity(
        "info_player_deathmatch",
        &[("origin", "0 0 24")],
    ));
    let compiled = compile(&map).unwrap();
    assert!(!compiled.has_timing());
    assert!(compiled.warnings.contains(&Warning::NoTimerTriggers));
}

#[test]
fn caulk_is_solid_and_never_drawn() {
    let map = compiled();
    // Five solids, but only three of them are drawable: the two caulk walls
    // contribute collision and no triangles.
    let drawn_faces = map.mesh.triangle_count();
    assert!(drawn_faces > 0);

    let all_caulk = brush_entity(
        "worldspawn",
        &[],
        &[box_brush([-64, -64, -16], [64, 64, 0], "common/caulk")],
    ) + &point_entity("info_player_deathmatch", &[("origin", "0 0 24")]);
    let caulked = compile(&all_caulk).unwrap();
    assert_eq!(caulked.hulls.len(), 1, "caulk still blocks the player");
    assert_eq!(
        caulked.mesh.triangle_count(),
        0,
        "caulk is never drawn — it is the inside of the world"
    );
}

#[test]
fn a_box_draws_as_twelve_triangles() {
    let map = brush_entity(
        "worldspawn",
        &[],
        &[box_brush([-64, -64, -16], [64, 64, 0], "base_floor/tile")],
    ) + &point_entity("info_player_deathmatch", &[("origin", "0 0 24")]);
    let compiled = compile(&map).unwrap();
    assert_eq!(compiled.mesh.triangle_count(), 12, "six quads, two each");
    assert_eq!(compiled.mesh.vertices.len(), 24);
    // Every vertex is a corner of the box, and the mesh agrees with the hull.
    for v in &compiled.mesh.vertices {
        assert!(v.position[0].abs() == 64.0 || v.position[1].abs() == 64.0 || v.position[2] <= 0.0);
    }
}

#[test]
fn a_sloped_brush_keeps_its_slope() {
    // A wedge: a box with the top cut by a 1:1 plane rising along +X. The face
    // is built from three points on the slope, wound so it faces up and out.
    let slope = "{\n".to_string()
        + &face([-256, -256, 0], [0, 0, 64], [0, 64, 0], "base_floor/tile") // -X
        + &face([256, -256, 0], [0, 64, 0], [0, 0, 64], "base_floor/tile") // +X
        + &face([-256, -256, 0], [64, 0, 0], [0, 0, 64], "base_floor/tile") // -Y
        + &face([-256, 256, 0], [0, 0, 64], [64, 0, 0], "base_floor/tile") // +Y
        + &face([-256, -256, -64], [0, 64, 0], [64, 0, 0], "base_floor/tile") // -Z
        // The incline: through (-256,*,0) and (256,*,512), so u = (64,0,64)
        // rising along X, v = +Y, and u x v points up and back along -X.
        + &face([-256, -256, 0], [64, 0, 64], [0, 64, 0], "base_floor/tile")
        + "}\n";
    let map = brush_entity("worldspawn", &[], &[slope])
        + &point_entity("info_player_deathmatch", &[("origin", "0 0 600")]);
    let compiled = compile(&map).unwrap();
    assert_eq!(compiled.hulls.len(), 1);

    let incline = compiled.hulls[0]
        .planes
        .iter()
        .find(|p| p.normal.z > 0.0 && p.normal.z < 1.0)
        .unwrap_or_else(|| panic!("no sloped face in {:?}", compiled.hulls[0].planes));
    // A 1:1 slope's normal Z is exactly 1/√2 — say that rather than a decimal.
    assert!(
        (incline.normal.z - core::f32::consts::FRAC_1_SQRT_2).abs() < 1.0e-6,
        "slope normal was {:?}",
        incline.normal
    );
    assert!(incline.normal.x < 0.0, "it rises along +X, so it faces -X");
}

#[test]
fn compiling_the_same_source_twice_gives_the_same_bits() {
    // The C7 requirement 3 property, as far as one machine can check it: no
    // hash-map iteration, no accumulated order dependence, nothing ambient.
    let source = course();
    let a = compile(&source).unwrap();
    let b = compile(&source).unwrap();
    assert_eq!(a.collision_digest(), b.collision_digest());
    assert_eq!(a.full_digest(), b.full_digest());
    assert_eq!(a, b, "the whole compiled map, not just its digest");
}

#[test]
fn moving_a_brush_by_one_unit_changes_the_collision_digest() {
    let a = compiled();
    let moved = course().replace("-512 -512 -64", "-512 -512 -63");
    let b = compile(&moved).unwrap();
    assert_ne!(
        a.collision_digest(),
        b.collision_digest(),
        "a digest that cannot see a moved floor certifies a run that no longer replays"
    );
}

#[test]
fn recolouring_a_face_does_not_change_the_collision_digest() {
    // The other half of the same promise: a recording must not be invalidated
    // by something the physics cannot feel.
    let a = compiled();
    // Twelve palette entries collide, and a shader that happens to land on the
    // same colour would make this test pass while proving nothing about the
    // digest — so pick a name that actually repaints the wall.
    let repaint = ["gothic_wall/slate", "base_wall/metal", "e7/e7brickwall"]
        .into_iter()
        .find(|n| mesh::color_for(n) != mesh::color_for("base_wall/brick"))
        .expect("some shader name gets a different colour");
    let repainted = course().replace("base_wall/brick", repaint);
    let b = compile(&repainted).unwrap();
    assert_eq!(a.collision_digest(), b.collision_digest());
    assert_ne!(a.full_digest(), b.full_digest(), "the mesh did change");
}

#[test]
fn the_trigger_volumes_are_in_the_collision_digest() {
    // C7 requirement 4 names them explicitly: hulls *and* trigger volumes. A
    // start line moved 8 units is a different course and a different time.
    let a = compiled();
    let moved = course().replace("-128 -160 0", "-128 -152 0");
    let b = compile(&moved).unwrap();
    assert_ne!(a.collision_digest(), b.collision_digest());
}

#[test]
fn hull_planes_come_out_in_q3maps_canonical_order() {
    // Load-bearing: the tracer resolves ties between coincident faces by index,
    // so plane order decides which surface a player slides along. It is q3map's
    // order — the six axial planes as `-X +X -Y +Y -Z +Z`, then everything else
    // — because matching Q3's plane order is matching Q3's choice of impact
    // normal. `compile_hull` does the permutation; this test is here so that a
    // change to it is visible from the map compiler's side too, since the
    // collision digest folds the compiled order and every recording binds to it.
    let map = brush_entity(
        "worldspawn",
        &[],
        &[box_brush([-64, -64, -16], [64, 64, 0], "base_floor/tile")],
    ) + &point_entity("info_player_deathmatch", &[("origin", "0 0 24")]);
    let compiled = compile(&map).unwrap();
    let normals: Vec<Vec3> = compiled.hulls[0].planes.iter().map(|p| p.normal).collect();
    assert_eq!(
        normals,
        vec![
            Vec3::NEG_X,
            Vec3::X,
            Vec3::NEG_Y,
            Vec3::Y,
            Vec3::NEG_Z,
            Vec3::Z
        ],
        "q3map's canonical axial order"
    );
}

#[test]
fn a_diagonal_brush_gains_the_bevel_planes_a_swept_box_needs() {
    // A brush rotated 45° about Z has no axial faces at all, so a swept *box*
    // needs the four axial bevel planes its bounding box supplies — without
    // them the player is stopped by an invisible wall up to a hull-width out
    // from the visible surface. `compile_hull` adds them; this asserts the map
    // compiler is actually going through that path rather than handing the
    // tracer a literal six-plane hull.
    let d = 128;
    let diamond = format!(
        "{{\n{}{}{}{}{}{}}}\n",
        // Four vertical faces, each rotated 45°: the plane through (±d,0) and
        // (0,±d). `u` runs along the wall and `v` is up, so `u x v` points out.
        face([d, 0, 0], [-d, d, 0], [0, 0, 64], "base_wall/brick"),
        face([0, d, 0], [-d, -d, 0], [0, 0, 64], "base_wall/brick"),
        face([-d, 0, 0], [d, -d, 0], [0, 0, 64], "base_wall/brick"),
        face([0, -d, 0], [d, d, 0], [0, 0, 64], "base_wall/brick"),
        face([-d, -d, 0], [0, 64, 0], [64, 0, 0], "base_wall/brick"), // -Z
        face([-d, -d, 128], [64, 0, 0], [0, 64, 0], "base_wall/brick"), // +Z
    );
    let map = brush_entity("worldspawn", &[], &[diamond])
        + &point_entity("info_player_deathmatch", &[("origin", "0 0 200")]);
    let compiled = compile(&map).unwrap();
    let hull = &compiled.hulls[0];
    assert!(
        hull.planes.len() > 6,
        "a diagonal brush must gain bevels, got {} planes",
        hull.planes.len()
    );
    for axis in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y] {
        assert!(
            hull.planes.iter().any(|p| p.normal == axis),
            "missing the {axis:?} axial bevel: {:?}",
            hull.planes.iter().map(|p| p.normal).collect::<Vec<_>>()
        );
    }
    // The six faces are still the six the mapper textured — bevels bound no
    // face and must not reach the render mesh.
    assert_eq!(compiled.mesh.triangle_count(), 4 * 2 + 2 * 2);
}

#[test]
fn the_legacy_alignment_format_compiles_to_the_same_geometry_as_valve_220() {
    // Every other fixture here is Valve 220, because that is what the crate is
    // named for. Most of the Quake 3 corpus is not: Radiant wrote the legacy
    // five-number alignment (`offX offY rot scaleX scaleY`) for years, and a
    // Defrag map from 2002 is a legacy-format file. Texture alignment is out of
    // scope this wave either way, so the two must produce identical geometry —
    // and if they ever do not, this is the test that says so before a real map
    // does.
    let valve = course();
    let legacy = valve.replace("[ 1 0 0 0 ] [ 0 -1 0 0 ] 0 0.5 0.5", "0 0 0 0.5 0.5");
    assert!(!legacy.contains('['), "the fixture is legacy-format now");

    let a = compile(&valve).unwrap();
    let b = compile(&legacy).unwrap();
    assert_eq!(a.collision_digest(), b.collision_digest());
    assert_eq!(a.hulls.len(), b.hulls.len());
    assert_eq!(a.mesh, b.mesh);
}

#[test]
fn quake_3s_trailing_flag_triple_is_read_off_a_legacy_face() {
    // Radiant writes three extra integers after the legacy alignment —
    // contents, surface flags, value. A face marked SURF_SLICK (0x2) by hand is
    // slick even though nothing in its shader name says so, and the simulation
    // has to hear about it: slick is what removes ground friction.
    let slick_floor = box_brush([-64, -64, -16], [64, 64, 0], "base_floor/tile")
        .replace("[ 1 0 0 0 ] [ 0 -1 0 0 ] 0 0.5 0.5", "0 0 0 0.5 0.5 0 2 0");
    let map = brush_entity("worldspawn", &[], &[slick_floor])
        + &point_entity("info_player_deathmatch", &[("origin", "0 0 24")]);
    let compiled = compile(&map).unwrap();
    assert!(
        compiled.hulls[0]
            .surface
            .contains(straf3_sim::world::SurfaceFlags::SLICK),
        "an explicitly slick face must reach the simulation"
    );
}

#[test]
fn a_duplicated_face_does_not_become_a_second_plane() {
    // Real maps carry these: a mapper clips the same corner twice. Each one
    // would cost a plane test on every trace, forever.
    let mut brush = box_brush([-64, -64, -16], [64, 64, 0], "base_floor/tile");
    let duplicate = face([-64, -64, 0], [64, 0, 0], [0, 64, 0], "base_floor/tile");
    brush = brush.replace("}\n", &format!("{duplicate}}}\n"));
    let map = brush_entity("worldspawn", &[], &[brush])
        + &point_entity("info_player_deathmatch", &[("origin", "0 0 24")]);
    let compiled = compile(&map).unwrap();
    assert_eq!(compiled.hulls[0].planes.len(), 6);
}

#[test]
fn a_patch_is_dropped_loudly() {
    let patch = "{\npatchDef2\n{\ncommon/caulk\n( 3 3 0 0 0 )\n(\n( ( 0 0 0 0 0 ) ( 0 0 0 0 0 ) \
                 ( 0 0 0 0 0 ) )\n)\n}\n}\n";
    let world = box_brush([-64, -64, -16], [64, 64, 0], "base_floor/tile") + patch;
    let map = brush_entity("worldspawn", &[], &[world])
        + &point_entity("info_player_deathmatch", &[("origin", "0 0 24")]);
    let compiled = compile(&map).unwrap();
    assert_eq!(compiled.hulls.len(), 1, "the box survives");
    assert!(
        compiled.warnings.contains(&Warning::PatchDropped {
            count: 1,
            severity: PatchLoss::Partial,
        }),
        "a dropped patch is missing collision and must be reported: {:?}",
        compiled.warnings
    );
    assert_eq!(compiled.patch_loss(), Some((1, PatchLoss::Partial)));
}

/// A map that loses most of its curves must say so in its *type*, not only in
/// a number the caller has to know how to read.
#[test]
fn wholesale_patch_loss_is_distinguishable_from_a_missing_arch() {
    let patch = "{\npatchDef2\n{\ncommon/caulk\n( 3 3 0 0 0 )\n(\n( ( 0 0 0 0 0 ) ( 0 0 0 0 0 ) \
                 ( 0 0 0 0 0 ) )\n)\n}\n}\n";
    let compile_with = |n: usize| {
        let world = box_brush([-64, -64, -16], [64, 64, 0], "base_floor/tile") + &patch.repeat(n);
        let map = brush_entity("worldspawn", &[], &[world])
            + &point_entity("info_player_deathmatch", &[("origin", "0 0 24")]);
        compile(&map).unwrap()
    };

    // Just under: decorative curves are missing, the map still plays.
    let few = compile_with(SUBSTANTIAL_PATCH_LOSS - 1);
    assert_eq!(
        few.patch_loss(),
        Some((SUBSTANTIAL_PATCH_LOSS - 1, PatchLoss::Partial))
    );

    // At the threshold, and well past it: this map's geometry is absent.
    for n in [SUBSTANTIAL_PATCH_LOSS, 706, 1123] {
        let many = compile_with(n);
        assert_eq!(
            many.patch_loss(),
            Some((n, PatchLoss::Substantial)),
            "{n} dropped patches is not a blemish"
        );
        // The brush geometry is still compiled — the warning is the signal, and
        // the compiler does not start refusing maps it used to accept.
        assert_eq!(many.hulls.len(), 1);
    }

    // And a clean map claims no loss at all, rather than Partial-with-zero.
    let clean = compile_with(0);
    assert_eq!(clean.patch_loss(), None);
}

#[test]
fn a_brush_def_map_compiles_to_the_same_geometry_as_a_legacy_one() {
    // The rewrite in `source.rs` is a syntax change and nothing else, so the
    // hulls it produces must be identical — this is the test that says so.
    let legacy = brush_entity(
        "worldspawn",
        &[],
        &[box_brush([-64, -64, -16], [64, 64, 0], "base_floor/tile")],
    ) + &point_entity("info_player_deathmatch", &[("origin", "0 0 24")]);

    // The same box written as brush primitives: same plane points, texture
    // matrix instead of the alignment quintet.
    let mut primitives = String::from("{\n\"classname\" \"worldspawn\"\n{\nbrushDef\n{\n");
    for line in box_brush([-64, -64, -16], [64, 64, 0], "base_floor/tile").lines() {
        if !line.starts_with('(') {
            continue;
        }
        // Replace `[ ... ] [ ... ] rot sx sy` with the brush-primitives matrix.
        let points: String = line.split(')').take(3).collect::<Vec<_>>().join(")") + ")";
        primitives.push_str(&format!(
            "{points} ( ( 0.0078125 0 0 ) ( 0 0.0078125 0 ) ) base_floor/tile 0 0 0\n"
        ));
    }
    primitives.push_str("}\n}\n}\n");
    primitives.push_str(&point_entity(
        "info_player_deathmatch",
        &[("origin", "0 0 24")],
    ));

    let a = compile(&legacy).unwrap();
    let b = compile(&primitives).unwrap();
    assert_eq!(
        a.collision_digest(),
        b.collision_digest(),
        "brushDef rewriting changed the geometry"
    );
}

#[test]
fn a_mover_is_compiled_where_it_stands_and_reported() {
    let map = brush_entity(
        "worldspawn",
        &[],
        &[box_brush(
            [-512, -512, -64],
            [512, 512, 0],
            "base_floor/tile",
        )],
    ) + &brush_entity(
        "func_door",
        &[("angle", "-1")],
        &[box_brush([-64, 0, 0], [64, 32, 128], "base_wall/brick")],
    ) + &point_entity("info_player_deathmatch", &[("origin", "0 -256 24")]);
    let compiled = compile(&map).unwrap();
    assert_eq!(compiled.hulls.len(), 2, "the door is solid where it stands");
    assert!(compiled.warnings.iter().any(|w| matches!(
        w,
        Warning::MoverFrozen { classname, .. } if classname == "func_door"
    )));
}

#[test]
fn a_trigger_pointing_at_nothing_keeps_its_volume_and_says_so() {
    let map = brush_entity(
        "worldspawn",
        &[],
        &[box_brush(
            [-512, -512, -64],
            [512, 512, 0],
            "base_floor/tile",
        )],
    ) + &brush_entity(
        "trigger_multiple",
        &[("target", "does_not_exist")],
        &[box_brush([-64, -64, 0], [64, 64, 64], "common/trigger")],
    ) + &point_entity("info_player_deathmatch", &[("origin", "0 -256 24")]);
    let compiled = compile(&map).unwrap();
    assert_eq!(compiled.triggers.len(), 1);
    assert_eq!(compiled.triggers[0].kind, TriggerKind::Other);
    assert!(compiled.warnings.iter().any(|w| matches!(
        w,
        Warning::UnresolvedTarget { target, .. } if target == "does_not_exist"
    )));
}

#[test]
fn every_entitys_keys_survive_compilation() {
    // The run-clock and replay tracks read keys this crate has no rule for.
    // Dropping them here would mean re-parsing the map to get them back.
    let map = compiled();
    let world = &map.entities[0];
    assert_eq!(world.classname, "worldspawn");
    assert_eq!(world.get("message"), Some("test course"));
    assert_eq!(map.entities.len(), 6);
    assert_eq!(map.entities[2].classname, "target_starttimer");
}

#[test]
fn the_compiled_bounds_cover_the_course() {
    let map = compiled();
    assert!(map.bounds.mins.x <= -576.0 && map.bounds.maxs.x >= 576.0);
    assert!(map.bounds.mins.z <= -64.0 && map.bounds.maxs.z >= 256.0);
}

/// The digest of the fixture course, pinned.
///
/// # Why a hardcoded number is worth the maintenance
///
/// C7 requirement 4 is that a change to the *compiler* invalidates recordings
/// made against it, even when the `.map` file has not changed. That is a
/// promise about a number, and the only way to test a promise about a number is
/// to write the number down. If this test fails and the change to the compiler
/// was intentional, updating it is correct — and the update is the moment
/// someone notices that every recording and every ghost made before it is now
/// scrap, which is exactly the moment the spec wants noticed.
///
/// It is also the value `cargo xtask determinism` should compare across
/// targets: same source, same number, on glibc, musl, Windows and wasm.
const COURSE_COLLISION_DIGEST: u64 = 0x45af_888a_8371_4709;

#[test]
fn the_fixture_courses_digest_is_the_pinned_one() {
    assert_eq!(
        compiled().collision_digest(),
        COURSE_COLLISION_DIGEST,
        "the compiler's output changed — see this constant's doc comment before \
         updating it, because every recording made against the old value is scrap"
    );
}

// ---------------------------------------------------------------------------
// The collider carries the timing volumes — the link that makes a run a time.
//
// `collider()` is the single choke point every consumer goes through
// (`straf3-game`'s scene, `straf3-render`), so a trigger volume that does not
// arrive here arrives nowhere: the clock is built, correct, and never fed. The
// tests below are about that hand-over, not about the clock, which is
// `straf3-sim`'s own `tests/run_clock.rs`.

/// Walk `state` forward along +Y, one command at a time, and stop as soon as
/// the run is finished.
///
/// Deliberately the real `step_in_place` against the real collider rather than
/// a geometry query: the question is whether a *player* crossing these volumes
/// starts and stops the clock, and only the mover can answer that.
fn run_the_fixture_course(world: &HullWorld, from: Vec3) -> straf3_sim::SimState {
    use straf3_sim::{Buttons, PhysicsProfile, SimState, TickRate, UserCmd, ViewAngles};

    let rate = TickRate::HZ_125;
    let profile = PhysicsProfile::cpm();
    // Yaw 90 is +Y, which is the direction the fixture course runs in.
    let mut state = SimState::spawned_at(from, straf3_sim::num::s(90.0));
    let cmd = UserCmd {
        duration_ms: rate.command_millis(),
        forward_move: 127,
        right_move: 0,
        up_move: 0,
        buttons: Buttons::NONE,
        view: ViewAngles::from_degrees(
            straf3_sim::num::s(0.0),
            straf3_sim::num::s(90.0),
            straf3_sim::num::s(0.0),
        ),
    };
    for _ in 0..2_000 {
        straf3_sim::step_in_place(&mut state, &cmd, world, &profile);
        if matches!(state.run, straf3_sim::RunState::Finished { .. }) {
            break;
        }
    }
    state
}

#[test]
fn a_compiled_map_hands_over_a_collider_that_can_time_a_run() {
    let map = compiled();
    assert!(map.has_timing());

    let world = map.collider();
    let coverage = world.trigger_coverage();
    assert!(
        coverage.contains(TriggerSet::START),
        "the collider has no start volume, so no run in it can ever begin"
    );
    assert!(
        coverage.contains(TriggerSet::FINISH),
        "the collider has no finish volume, so no run in it can ever end"
    );

    // The volumes must not have become solid geometry: a player runs *through*
    // a finish line. Five solid brushes, as `a_trigger_volume_is_not_solid`
    // already pins, and two volumes alongside them.
    assert_eq!(world.hulls().len(), map.hulls.len());
    assert_eq!(world.hulls().len(), 5);
    assert_eq!(world.triggers().len(), 2);
}

#[test]
fn a_player_walking_the_fixture_course_gets_a_time_in_whole_milliseconds() {
    let map = compiled();
    let world = map.collider();
    let state = run_the_fixture_course(&world, map.spawn);

    let straf3_sim::RunState::Finished {
        started_at_ms,
        finished_at_ms,
    } = state.run
    else {
        panic!("the walk never finished: run was {:?}", state.run);
    };

    // The interesting assertion is not the number, it is that the number came
    // from the geometry. The spawn is at y=-256, the start volume ends at
    // y=-96 and the finish begins at y=352, so the clock must start well after
    // the first command and stop well before the last.
    assert!(
        started_at_ms > 0,
        "the clock started before the player moved"
    );
    assert!(finished_at_ms > started_at_ms);

    let elapsed = state.run.elapsed_ms(state.time_ms).expect("a finished run");
    assert_eq!(elapsed, finished_at_ms - started_at_ms);

    // Walking speed under CPM is 320 ups and the volumes are ~450 units apart
    // measured leading-edge to leading-edge, so a second and a half either way
    // is a generous window around a walk and still excludes a clock that
    // started at spawn or stopped at the end of the loop.
    assert!(
        (500..3_000).contains(&elapsed),
        "a walk down a 450-unit course took {elapsed} ms, which is not a walk"
    );

    // `u32` milliseconds, not float seconds, all the way out (spec: no float
    // seconds anywhere). This is a type-level assertion; it is here so that
    // changing `RunState` to carry an `f32` fails a test rather than a review.
    let _: u32 = elapsed;
}

#[test]
fn a_volume_that_is_not_timing_gets_no_bit_rather_than_a_spare_one() {
    // A jump pad and a teleporter are recorded geometry. Giving either one a
    // bit would put it in the same alphabet as a finish line, and a run would
    // stop when the player took a shortcut.
    assert_eq!(TriggerKind::Teleport.trigger_set(), None);
    assert_eq!(TriggerKind::Push.trigger_set(), None);
    assert_eq!(TriggerKind::Other.trigger_set(), None);
    assert_eq!(TriggerKind::Start.trigger_set(), Some(TriggerSet::START));
    assert_eq!(TriggerKind::Finish.trigger_set(), Some(TriggerSet::FINISH));
    assert_eq!(
        TriggerKind::Checkpoint(0).trigger_set(),
        TriggerSet::checkpoint(0)
    );

    let mut source = course();
    source.push_str(&point_entity(
        "t_dest",
        &[("targetname", "t_dest"), ("origin", "0 256 32")],
    ));
    source.push_str(&brush_entity(
        "trigger_teleport",
        &[("target", "t_dest")],
        &[box_brush([-128, 96, 0], [128, 160, 128], "common/trigger")],
    ));
    let map = compile(&source).expect("a teleporter is not a compile error");

    // Kept in the entity data — the geometry is not lost...
    assert_eq!(map.triggers.len(), 3);
    // ...and still absent from the collider, which only speaks timing.
    let world = map.collider();
    assert_eq!(
        world.triggers().len(),
        2,
        "a teleporter reached the run clock's alphabet"
    );
}

#[test]
fn checkpoints_past_the_bit_budget_are_reported_rather_than_dropped_in_silence() {
    // A missing 31st split looks exactly like a player who missed it, so the
    // compiler has to say so. No map in the tree comes near this — coil.map
    // has two — but the limit is real and silent loss is what this crate's
    // warnings exist to prevent.
    let over = TriggerSet::MAX_CHECKPOINTS + 2;
    let mut source = course();
    for i in 0..over {
        source.push_str(&point_entity(
            "target_checkpoint",
            &[
                ("targetname", &format!("cp{i}")),
                ("origin", &format!("0 {} 32", 16 * i)),
            ],
        ));
        source.push_str(&brush_entity(
            "trigger_multiple",
            &[("target", &format!("cp{i}"))],
            &[box_brush(
                [-8, 16 * i as i32, 0],
                [8, 16 * i as i32 + 8, 128],
                "common/trigger",
            )],
        ));
    }
    let map = compile(&source).expect("too many checkpoints is a warning, not an error");

    assert_eq!(
        map.warnings
            .iter()
            .filter(|w| **w == Warning::TooManyCheckpoints { dropped: 2 })
            .count(),
        1,
        "warnings were {:?}",
        map.warnings
    );

    // Every checkpoint is still in the entity data; only the last two lack a
    // bit, so the collider carries the rest plus start and finish.
    assert_eq!(map.triggers.len(), 2 + over as usize);
    assert_eq!(
        map.collider().triggers().len(),
        2 + TriggerSet::MAX_CHECKPOINTS as usize
    );
}
