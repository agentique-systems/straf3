//! The browser entry point: a URL's map and physics in, a played — or
//! watched — session out.
//!
//! # What this module is responsible for
//!
//! `docs/web/URLS.md` §4 is a fixed contract, and four of its clauses are
//! discharged here rather than by the page:
//!
//! | Clause | Where |
//! |---|---|
//! | the link puts the player straight in, with no menu step | [`boot_play`] |
//! | a pinned physics digest this build does not implement is **refused** | [`resolve_physics`] |
//! | a `ghost` that will not resolve degrades to playing without one | [`resolve_ghost`] |
//! | `/watch/` takes its map and physics from the recording's own header | [`boot_watch`] |
//!
//! The division of labour with the host page is the wave contract's §B: the
//! page owns the DOM, the network policy and the sign-in token; this module
//! owns the canvas, the simulation and the recording. **This module never
//! talks to `/v1` and never sees a token.** It fetches exactly the URLs the
//! page handed it, and a finished run leaves through `onRunFinished` for the
//! page to do something with.
//!
//! # Why refusing and degrading are different code paths
//!
//! They look similar — both are a message on screen — and conflating them is
//! the single most likely way to get §4 wrong. A pinned physics digest this
//! build cannot honour means the run the link promised is not the run this
//! build would produce, so there is nothing honest to do but stop
//! ([`refuse`]). A ghost that will not load costs the player an opponent and
//! nothing else, so the map still loads and the failure is stated
//! ([`notice`]). One returns without starting the game; the other returns
//! `None` and carries on.
//!
//! # Why the config is JSON and not a widening argument list
//!
//! Three seats integrate on this signature. `start_web(backend)` already had
//! to become `start_web(config)` once; a JSON string means the next field
//! costs the page a key and costs the other seats nothing, and an old page
//! calling a new build gets a stated parse error rather than a silently
//! dropped argument.
//!
//! JS values are read with `js_sys` rather than deserialised with serde. The
//! whole browser bundle is ~131 KiB gzipped and a derive stack is a large
//! fraction of that for a handful of fields.

use std::cell::RefCell;
use std::sync::Arc;

use js_sys::{Array, Function, JSON, Object, Reflect, Uint8Array};
use straf3_replay::{Recording, WorldId, physics_digest};
use straf3_sim::PhysicsProfile;
use straf3_sim::num::{s, to_bits};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::prelude::*;

use crate::app::{Options, Playback, RunSink};
use crate::scene::WorldChoice;

/// The category a `/play/<map>` link plays under when it names none.
///
/// CPM rather than VQ3 because the shipped course is authored for it, and
/// because a straf3 link with no `?p=` should land on the movement the project
/// is about. Stated here rather than defaulted deep in a `unwrap_or` so that
/// the answer to "what did this link actually run?" has one place to look.
const DEFAULT_FAMILY: &str = "cpm";

/// Where a map's source is fetched from when the page does not say.
///
/// `/assets/maps/` is a reserved first segment in URLS.md §6 and the wave
/// contract mounts the repository's `assets/maps/` there, so this is the
/// layout the one origin already serves. A page that mounts them elsewhere
/// overrides it with `map.source_url`, or with `map_url_template` when it does
/// not know the slug in advance — which is exactly the `/watch/<run>` case,
/// where the map's name comes out of the recording's header.
const DEFAULT_MAP_URL_TEMPLATE: &str = "/assets/maps/{slug}.map";

// ── the page's half of the interface ────────────────────────────────────────

/// The object the page hangs its callbacks on, if it defined one.
///
/// Absent is normal and is not an error: the client's own dev shell defines
/// every callback, a bare page defines none, and the site defines the ones it
/// cares about. Nothing here may crash because a callback is missing.
fn callbacks() -> Option<Object> {
    Reflect::get(&js_sys::global(), &JsValue::from_str("straf3"))
        .ok()
        .and_then(|value| value.dyn_into::<Object>().ok())
}

/// Call `globalThis.straf3.<name>(..args)` if the page defined it.
///
/// A callback that throws is logged and swallowed. The page's error handling
/// is the page's business, and a simulation that stopped because a status
/// listener threw would be the client punishing the player for the site's bug.
fn call_page(name: &str, args: &[JsValue]) {
    let Some(object) = callbacks() else {
        return;
    };
    let Some(function) = Reflect::get(&object, &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.dyn_into::<Function>().ok())
    else {
        return;
    };
    let arguments = Array::new();
    for argument in args {
        arguments.push(argument);
    }
    if let Err(e) = function.apply(&object, &arguments) {
        log::error!("the page's straf3.{name} threw: {e:?}");
    }
}

/// Put text in an element of the host page, if it has one by that id.
fn write_element(id: &str, text: &str) {
    if let Some(element) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
    {
        element.set_text_content(Some(text));
    }
}

/// Report where the session has got to.
///
/// `kind` is the wave contract's vocabulary — `loading`, `ready`, `error`,
/// `refused` — and goes to the page's `onStatus` as well as to the big centred
/// element, because a visitor whose browser cannot run straf3 is looking at a
/// blank canvas and not at devtools.
fn status(kind: &str, message: &str) {
    write_element("straf3-status", message);
    call_page(
        "onStatus",
        &[JsValue::from_str(kind), JsValue::from_str(message)],
    );
}

/// Refuse to start, and say exactly what could not be honoured.
///
/// URLS.md §4 behaviour 3 and §7.4's discipline: the alternative is to run the
/// nearest thing, and a run produced under physics the URL did not name is not
/// the run the link promised. The caller returns immediately after this — no
/// path in this module both refuses and starts.
fn refuse(message: &str) {
    log::error!("refusing to start: {message}");
    status("refused", message);
}

/// State a failure that costs the player something less than the map.
///
/// Written to its own element rather than through [`status`], so that the
/// "ready" that follows does not wipe it: a ghost that failed to load is worth
/// saying *while the player is playing without it*, not for the 200 ms before
/// the game starts.
fn notice(message: &str) {
    log::warn!("{message}");
    write_element("straf3-notice", message);
    call_page(
        "onStatus",
        &[JsValue::from_str("error"), JsValue::from_str(message)],
    );
}

/// Tell the page the browser took or dropped the pointer.
///
/// Called from [`crate::app::App::sync_pointer_lock`], which reads the
/// browser's own `document.pointerLockElement` rather than assuming our
/// request was honoured.
pub(crate) fn pointer_lock_changed(locked: bool) {
    call_page("onPointerLock", &[JsValue::from_bool(locked)]);
}

// ── fetching ────────────────────────────────────────────────────────────────

/// Describe a rejected promise well enough to act on.
fn describe(error: &JsValue) -> String {
    error
        .as_string()
        .or_else(|| {
            Reflect::get(error, &JsValue::from_str("message"))
                .ok()
                .and_then(|m| m.as_string())
        })
        .unwrap_or_else(|| format!("{error:?}"))
}

/// `fetch(url)`, with a non-2xx treated as the failure it is.
///
/// `fetch` resolves happily on a 404 and leaves the status in the response,
/// which is how a missing map turns into a compile error about HTML further
/// down. Checking `ok` here is what makes the message name the real problem.
async fn get(url: &str) -> Result<web_sys::Response, String> {
    let window = web_sys::window().ok_or_else(|| "there is no window to fetch from".to_owned())?;
    let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|e| format!("{url} could not be fetched: {}", describe(&e)))?;
    let response: web_sys::Response = response
        .dyn_into()
        .map_err(|_| format!("{url}: fetch did not return a Response"))?;
    if !response.ok() {
        return Err(format!(
            "{url}: HTTP {} {}",
            response.status(),
            response.status_text()
        ));
    }
    Ok(response)
}

async fn get_text(url: &str) -> Result<String, String> {
    let response = get(url).await?;
    let promise = response
        .text()
        .map_err(|e| format!("{url}: {}", describe(&e)))?;
    wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|e| format!("{url}: {}", describe(&e)))?
        .as_string()
        .ok_or_else(|| format!("{url}: the body was not text"))
}

async fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let response = get(url).await?;
    let promise = response
        .array_buffer()
        .map_err(|e| format!("{url}: {}", describe(&e)))?;
    let buffer = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|e| format!("{url}: {}", describe(&e)))?;
    Ok(Uint8Array::new(&buffer).to_vec())
}

// ── the config ──────────────────────────────────────────────────────────────

/// What the session is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// `/play/<map>` — the player drives.
    Play,
    /// `/watch/<run>` — a recording drives.
    Watch,
}

/// Everything the page decided before entering wasm.
#[derive(Debug, Clone)]
struct Config {
    backend: String,
    canvas_id: String,
    mode: Mode,
    map_slug: Option<String>,
    map_source_url: Option<String>,
    map_url_template: String,
    physics_family: Option<String>,
    /// The `@digest16` in `?p=<family>@<digest16>`, when the link pinned one.
    physics_digest: Option<u64>,
    ghost_url: Option<String>,
    recording_url: Option<String>,
    seek_ms: u32,
}

fn field(object: &JsValue, key: &str) -> JsValue {
    Reflect::get(object, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

/// A string field, treating `null`, `undefined` and `""` alike as absent.
///
/// The empty string matters: a page building a config by template substitution
/// produces `"ghost_url": ""` for "no ghost" far more often than it remembers
/// to omit the key, and an empty URL would otherwise be fetched and fail.
fn string_field(object: &JsValue, key: &str) -> Option<String> {
    field(object, key)
        .as_string()
        .filter(|text| !text.is_empty())
}

impl Config {
    /// Read the JSON the page passed to [`start_web`].
    ///
    /// # Errors
    ///
    /// A string describing what about it could not be read. Every one of these
    /// is a refusal: a config that cannot be read cannot be honoured, and
    /// guessing at the missing half is how a link runs something it did not
    /// name.
    fn parse(json: &str) -> Result<Self, String> {
        let value = JSON::parse(json).map_err(|e| describe(&e))?;
        if !value.is_object() {
            return Err("the config is not a JSON object".to_owned());
        }

        let mode = match string_field(&value, "mode").as_deref() {
            None | Some("play") => Mode::Play,
            Some("watch") => Mode::Watch,
            Some(other) => return Err(format!("`{other}` is not a mode; play or watch")),
        };

        let map = field(&value, "map");
        let physics = field(&value, "physics");
        let physics_digest = match string_field(&physics, "digest") {
            Some(text) => Some(parse_digest16(&text)?),
            None => None,
        };

        Ok(Self {
            backend: string_field(&value, "backend").unwrap_or_default(),
            canvas_id: string_field(&value, "canvas_id")
                .unwrap_or_else(|| "straf3-canvas".to_owned()),
            mode,
            map_slug: string_field(&map, "slug"),
            map_source_url: string_field(&map, "source_url"),
            map_url_template: string_field(&map, "url_template")
                .unwrap_or_else(|| DEFAULT_MAP_URL_TEMPLATE.to_owned()),
            physics_family: string_field(&physics, "family"),
            physics_digest,
            ghost_url: string_field(&value, "ghost_url"),
            recording_url: string_field(&value, "recording_url"),
            // `as_f64` and not a string parse: JSON numbers arrive as numbers.
            // A negative or fractional seek is nonsense rather than an error
            // worth refusing a whole recording over, so it clamps to zero.
            seek_ms: field(&value, "seek_ms")
                .as_f64()
                .filter(|ms| ms.is_finite() && *ms > 0.0)
                .map_or(0, |ms| ms as u32),
        })
    }

    /// Where to fetch `slug`'s source from.
    ///
    /// `source_url` wins when the page gave one. It cannot be used in watch
    /// mode for a map the page did not know the name of in advance, which is
    /// why the template exists.
    fn map_url(&self, slug: &str) -> String {
        self.map_source_url
            .clone()
            .unwrap_or_else(|| self.map_url_template.replace("{slug}", slug))
    }
}

/// Read the 16 hex characters URLS.md §5 spells a digest as.
///
/// Both cases are accepted although §5 writes them lowercase: the comparison
/// downstream is numeric, so refusing an otherwise-correct link over the case
/// of a hex digit would be pedantry with a cost and no benefit.
fn parse_digest16(text: &str) -> Result<u64, String> {
    if text.len() != 16 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "`{text}` is not a digest: URLS.md §5 spells one as exactly 16 hex characters"
        ));
    }
    u64::from_str_radix(text, 16).map_err(|e| format!("`{text}`: {e}"))
}

// ── the entry point ─────────────────────────────────────────────────────────

/// Start straf3 in the browser.
///
/// `config_json` is the wave contract's §B object. The page has already
/// established that WebGPU is available and says so with `backend`, because
/// wgpu does not fall back from WebGPU to WebGL2 on its own — with both
/// backends compiled in and `requestAdapter()` returning null it crashes
/// inside the WebGPU backend instead of degrading (spec rev 6 §Q2). straf3
/// therefore ships WebGPU-only, and the check that this browser can run it at
/// all belongs before wasm is entered.
///
/// This returns immediately. Everything below it needs the network — a map is
/// fetched before it can be compiled — so the work is spawned onto the
/// microtask queue and the page gets its thread back.
#[wasm_bindgen]
pub fn start_web(config_json: &str) {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Info);

    match Config::parse(config_json) {
        Ok(config) => wasm_bindgen_futures::spawn_local(boot(config)),
        Err(e) => refuse(&format!("the page's start_web config could not be read: {e}")),
    }
}

thread_local! {
    /// The last finished run, in the shape [`PageRunSink`] hands to the page.
    ///
    /// Kept so that a harness — or a page that had not defined
    /// `onRunFinished` when the run finished — can still get the bytes out.
    /// Extracting a `.s3d` from a browser is the whole of requirement r6's
    /// evidence, and depending on a callback having been registered at exactly
    /// the right moment is a bad way to hold the only copy.
    static LAST_RUN: RefCell<Option<Object>> = const { RefCell::new(None) };
}

/// The last run this session finished, or `null`.
///
/// The same object `onRunFinished` was called with: `{ time_ms,
/// run_digest_hex16, sim_time_ms, command_count, s3d }`.
#[wasm_bindgen]
#[must_use]
pub fn straf3_last_run() -> JsValue {
    LAST_RUN.with(|last| {
        last.borrow()
            .as_ref()
            .map_or(JsValue::NULL, |object| object.clone().into())
    })
}

/// A snapshot of the session, for a harness driving the browser.
///
/// # Why this exists
///
/// Everything requirement r5 is about — that the view turns when the mouse
/// moves, that a key produces movement in the direction the player is
/// facing — is a fact about the *simulation state*, and on a software-only
/// host it cannot be read off a screenshot: this box's headless Chrome does
/// not hand a WebGPU layer back to `Page.captureScreenshot` at all. Without
/// this, driving the browser would mean inferring the game's state from a
/// once-a-second log line.
///
/// It is a snapshot and not a handle: nothing here can *change* the session,
/// so no test written against it can accidentally become a second way to
/// drive the game.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DebugState {
    pub tick: u32,
    pub time_ms: u32,
    pub origin: (f32, f32, f32),
    pub pitch: f32,
    pub yaw: f32,
    pub speed: f32,
    pub grounded: bool,
    pub pointer_locked: bool,
    /// `0` not started, `1` running, `2` finished.
    pub run: u8,
    pub run_ms: u32,
    pub fps: u32,
}

thread_local! {
    static DEBUG_STATE: RefCell<Option<DebugState>> = const { RefCell::new(None) };
}

/// Publish this frame's snapshot. Called once a frame from the event loop.
pub(crate) fn publish_debug_state(state: DebugState) {
    DEBUG_STATE.with(|slot| *slot.borrow_mut() = Some(state));
}

/// The session as of the last drawn frame, or `null` before the first one.
#[wasm_bindgen]
#[must_use]
pub fn straf3_debug_state() -> JsValue {
    DEBUG_STATE.with(|slot| {
        slot.borrow().map_or(JsValue::NULL, |state| {
            let object = Object::new();
            let mut set = |key: &str, value: JsValue| {
                let _ = Reflect::set(&object, &JsValue::from_str(key), &value);
            };
            let number = |v: f64| JsValue::from_f64(v);
            set("tick", number(f64::from(state.tick)));
            set("time_ms", number(f64::from(state.time_ms)));
            set("x", number(f64::from(state.origin.0)));
            set("y", number(f64::from(state.origin.1)));
            set("z", number(f64::from(state.origin.2)));
            set("pitch", number(f64::from(state.pitch)));
            set("yaw", number(f64::from(state.yaw)));
            set("speed", number(f64::from(state.speed)));
            set("grounded", JsValue::from_bool(state.grounded));
            set("pointer_locked", JsValue::from_bool(state.pointer_locked));
            set("run", number(f64::from(state.run)));
            set("run_ms", number(f64::from(state.run_ms)));
            set("fps", number(f64::from(state.fps)));
            object.into()
        })
    })
}

async fn boot(config: Config) {
    if config.backend != "webgpu" {
        refuse(&format!(
            "straf3 is WebGPU-only (spec rev 6 §Q2) and this page reported backend \
             `{}`. wgpu does not fall back to WebGL2, it crashes, so this stops here \
             rather than several frames inside the renderer.",
            config.backend
        ));
        return;
    }
    match config.mode {
        Mode::Play => boot_play(config).await,
        Mode::Watch => boot_watch(config).await,
    }
}

/// The physics the link named, or a refusal naming both digests.
///
/// The digest is **recomputed from the profile this build is about to
/// simulate with** — `straf3_replay::physics_digest`, the same function
/// `crates/straf3-replay/src/identity.rs` binds a recording with. The URL's
/// digest is compared against it and never trusted in its place.
fn resolve_physics(family: &str, pinned: Option<u64>) -> Option<PhysicsProfile> {
    let Some(profile) = crate::profile::by_name(family) else {
        refuse(&format!(
            "this build has no physics family `{family}`. It implements {}.",
            crate::profile::NAMES
        ));
        return None;
    };
    let actual = physics_digest(&profile);
    match pinned {
        Some(pinned) if pinned != actual => {
            refuse(&format!(
                "this link pins physics `{family}@{pinned:016x}` and this build's \
                 `{family}` is {actual:016x}. Refusing rather than running the nearest \
                 thing: a run made under physics the link did not name is not the run \
                 the link promised."
            ));
            None
        }
        _ => Some(profile),
    }
}

/// `/play/<map>`: the URL's map and the URL's physics, with the player in it.
async fn boot_play(config: Config) {
    // Physics first, and before a single byte is fetched. A refusal that this
    // build cannot honour a pinned digest is knowable without the network, and
    // making the player wait for a map download to be told so would be
    // gratuitous.
    let family = config
        .physics_family
        .clone()
        .unwrap_or_else(|| DEFAULT_FAMILY.to_owned());
    let Some(profile) = resolve_physics(&family, config.physics_digest) else {
        return;
    };

    let Some(slug) = config.map_slug.clone() else {
        refuse("this link names no map, and there is no default one to substitute.");
        return;
    };
    let url = config.map_url(&slug);
    status("loading", &format!("loading {slug}…"));
    let source = match get_text(&url).await {
        Ok(source) => source,
        Err(e) => {
            refuse(&format!("the map `{slug}` could not be loaded — {e}"));
            return;
        }
    };
    if !install_map(&slug, &source) {
        return;
    }

    // URLS.md §4 behaviour 4: every way this can fail leaves the map loaded
    // and the player in it.
    let ghost = match &config.ghost_url {
        Some(url) => resolve_ghost(url, &profile, &family).await,
        None => None,
    };

    start(
        config,
        Options {
            world: WorldChoice::Map,
            profile,
            profile_name: family,
            ghost,
            ..browser_defaults()
        },
    );
}

/// `/watch/<run>`: the recording's own header decides everything.
///
/// URLS.md §4 behaviour 2, and the reason a `.s3d` carries a [`WorldId`] and a
/// `PhysicsId` at all. The URL says where the *bytes* live and nothing else;
/// what they mean is read out of them. A map fetched to satisfy the header is
/// checked against the header's collision digest before it is played, so the
/// worst the URL can do is name the wrong file and be told so.
async fn boot_watch(config: Config) {
    let Some(url) = config.recording_url.clone() else {
        refuse("this is a watch link with no recording to play.");
        return;
    };
    status("loading", "loading the recording…");
    let bytes = match get_bytes(&url).await {
        Ok(bytes) => bytes,
        Err(e) => {
            refuse(&format!("the recording could not be loaded — {e}"));
            return;
        }
    };
    let recording = match Recording::from_bytes(&bytes) {
        Ok(recording) => recording,
        Err(e) => {
            refuse(&format!("{url} is not a recording this build can read: {e}"));
            return;
        }
    };

    let Some(world) = resolve_recorded_world(&config, recording.world()).await else {
        return;
    };
    // The profile's *name* comes from the header and its constants from this
    // build; `check` below then requires the two to agree. Taking the name
    // from the URL instead is exactly the substitution §4 behaviour 2 forbids.
    let family = recording.physics().name.clone();
    let Some(profile) = resolve_physics(&family, Some(recording.physics().digest)) else {
        return;
    };
    let Some(world_id) = world.world_id() else {
        refuse("this build cannot identify the world that recording was made in.");
        return;
    };
    let commands = match recording.commands_for(&world_id, &profile) {
        Ok(commands) => commands.to_vec(),
        Err(mismatch) => {
            refuse(&format!(
                "that recording cannot be played back here: {mismatch}. Playing it \
                 against different geometry would show a run that never happened."
            ));
            return;
        }
    };

    if config.seek_ms > 0 {
        // URLS.md §4 behaviour 4 for `/watch/`: a client that cannot seek
        // starts at zero and says so. Seeking means starting the simulation
        // from a state no `SimState::spawned_at` can express, which is a
        // second entry into the stepping path — see `game`'s module docs on
        // why this crate has exactly one. Not built this wave.
        notice(&format!(
            "this build cannot seek: playback starts at 0 ms, not {} ms.",
            config.seek_ms
        ));
    }

    let start_state = *recording.start();
    start(
        config,
        Options {
            world,
            profile,
            profile_name: family,
            rate: start_state.rate,
            playback: Some(Playback {
                cmds: commands,
                spawn: start_state.spawn,
                yaw: start_state.yaw,
                source: url,
            }),
            // A watched run is somebody else's; it is not recorded again and
            // it is certainly not submitted. Re-recording it would produce a
            // second file with the same digest and a different provenance.
            record: false,
            run_sink: None,
            ..browser_defaults()
        },
    );
}

/// The world a recording says it was made in, made available here.
///
/// Returns `None` having already refused.
async fn resolve_recorded_world(config: &Config, recorded: &WorldId) -> Option<WorldChoice> {
    match recorded {
        WorldId::Empty => Some(WorldChoice::Empty),
        WorldId::Flat { height_bits } => {
            if *height_bits == to_bits(s(0.0)) {
                Some(WorldChoice::Flat)
            } else {
                refuse(&format!(
                    "that recording was made on flat ground at z={}, and this build \
                     only has the plane at z=0.",
                    f32::from_bits(*height_bits)
                ));
                None
            }
        }
        WorldId::Map {
            name,
            collision_digest,
        } => {
            let url = config.map_url(name);
            let source = match get_text(&url).await {
                Ok(source) => source,
                Err(e) => {
                    refuse(&format!(
                        "that recording was made on map `{name}`, which could not be \
                         loaded — {e}"
                    ));
                    return None;
                }
            };
            if !install_map(name, &source) {
                return None;
            }
            // The check the URL cannot be trusted for. `name` says which file
            // to fetch; only the compiled geometry's digest says whether it is
            // the world the run happened in.
            let compiled = crate::scene::loaded()?.map.collision_digest();
            if compiled != *collision_digest {
                refuse(&format!(
                    "that recording was made on map `{name}` with collision digest \
                     {collision_digest:016x}, and {url} compiles to {compiled:016x}. \
                     The geometry moved, so the run in that file did not happen here."
                ));
                return None;
            }
            Some(WorldChoice::Map)
        }
    }
}

/// Compile and install a map, reporting the runtime collision digest.
///
/// The digest is logged deliberately and not incidentally: `straf3-sim` is
/// proven bit-identical across four targets, but *map compilation* under
/// `wasm32-unknown-unknown` is not covered by that gate the same way, and a
/// browser that compiled `coil.map` to different hulls would invalidate every
/// run recorded in it. This line is what makes the browser's answer readable
/// next to the native one.
fn install_map(slug: &str, source: &str) -> bool {
    match crate::scene::install(slug, source) {
        Ok(loaded) => {
            log::info!(
                "map `{slug}` compiled in wasm: collision digest {:#018x}, {} hulls, \
                 {} triggers",
                loaded.map.collision_digest(),
                loaded.map.hulls.len(),
                loaded.map.triggers.len(),
            );
            true
        }
        Err(e) => {
            refuse(&format!("the map `{slug}` did not compile: {e}"));
            false
        }
    }
}

/// Fetch and check `?ghost=<run>`, or say why the player is racing nobody.
///
/// **Every failure here returns `None` and none of them refuses the map.**
/// URLS.md §4 behaviour 4 is explicit, and it is the right call: an opponent
/// is a nice-to-have and the map is what the link was for.
async fn resolve_ghost(url: &str, profile: &PhysicsProfile, family: &str) -> Option<Recording> {
    let bytes = match get_bytes(url).await {
        Ok(bytes) => bytes,
        Err(e) => {
            notice(&format!("racing no ghost: {e}"));
            return None;
        }
    };
    let recording = match Recording::from_bytes(&bytes) {
        Ok(recording) => recording,
        Err(e) => {
            notice(&format!("racing no ghost: {url} did not decode — {e}"));
            return None;
        }
    };
    let world_id = WorldChoice::Map.world_id()?;
    if let Err(mismatch) = recording.check(&world_id, profile) {
        notice(&format!("racing no ghost: {mismatch}"));
        return None;
    }
    // Spec D2's rule, checked here as well as in `App::race` so that the
    // player is *told* rather than merely finding the ghost absent.
    if recording.physics().name != family {
        notice(&format!(
            "racing no ghost: that run was set under `{}` and this session is \
             `{family}`.",
            recording.physics().name
        ));
        return None;
    }
    Some(recording)
}

/// The options every browser session shares.
///
/// `record` is on because a run that was not recorded cannot be submitted and
/// nobody knows in advance which attempt is the good one — the same argument
/// the native build makes for turning it on whenever personal bests are.
/// `pb_dir` is off because there is no filesystem: [`PageRunSink`] is where a
/// finished run goes instead.
fn browser_defaults() -> Options {
    Options {
        record: true,
        pb_dir: None,
        run_sink: Some(Arc::new(PageRunSink)),
        ..Options::default()
    }
}

/// Hand the session to winit. Does not return.
fn start(config: Config, options: Options) {
    // Cleared here rather than by the page after `start_web` returns: winit's
    // web backend never returns normally — it throws a sentinel to unwind out
    // of `spawn_app` — so anything the host page tries to do afterwards runs
    // in a `catch` block, if at all. Whoever is running owns the status line,
    // and from this point that is us.
    status("ready", "");
    log::info!(
        "straf3 {} in the browser — {}, `{}` physics ({:016x}), {} Hz",
        env!("CARGO_PKG_VERSION"),
        options.world.name(),
        options.profile_name,
        physics_digest(&options.profile),
        options.rate.hz(),
    );
    crate::app::run(Options {
        window: straf3_platform::WindowConfig {
            canvas_id: config.canvas_id,
            ..straf3_platform::WindowConfig::straf3()
        },
        ..options
    });
}

// ── a finished run, on its way to the page ──────────────────────────────────

/// Hands a finished run to `globalThis.straf3.onRunFinished`.
#[derive(Debug)]
struct PageRunSink;

impl RunSink for PageRunSink {
    /// # Why the trace is written and a personal best's is not
    ///
    /// `to_bytes_with_checksums`, not `to_bytes`. The rolling digest alone
    /// already *detects* a disagreement — the fold is sticky, so a machine
    /// that differs on any command cannot agree about it — but it cannot
    /// *localise* one. A browser-recorded run exists to be re-simulated
    /// natively and compared, and when those two disagree the finding is the
    /// index of the first diverging command, which only the per-command trace
    /// can name. Eight bytes a command, about 18 KiB for a run.
    ///
    /// ARCHITECTURE §3.2 draws the same line: the digest detects, the trail
    /// localises, and treating the trail as the detector is the error §1.3
    /// documents.
    fn finished(&self, recording: &Recording) {
        let Some(bytes) = recording.to_bytes_with_checksums() else {
            // Only reachable for a recording loaded from a file that was
            // written without a trace, which this session's own runs never
            // are. Said out loud rather than quietly downgraded to
            // `to_bytes()`: a caller asking for evidence should be told it is
            // not available, not handed something that looks like it.
            notice(
                "a run finished, but this build could not produce a checksum trace for \
                 it — the bytes are not usable as evidence and were not emitted.",
            );
            return;
        };
        let claimed = recording.claimed();
        let run = Object::new();
        let set = |key: &str, value: JsValue| {
            let _ = Reflect::set(&run, &JsValue::from_str(key), &value);
        };
        // `null` and not `0` for a run with no time. A run that crossed the
        // finish line always has one; a zero would be a time nobody set.
        set(
            "time_ms",
            claimed
                .run_time_ms
                .map_or(JsValue::NULL, |ms| JsValue::from_f64(f64::from(ms))),
        );
        set(
            "run_digest_hex16",
            JsValue::from_str(&format!("{:016x}", claimed.digest)),
        );
        // Beyond the contract's three fields, and additive: these are what a
        // divergence report is written out of, and they are free here.
        set(
            "sim_time_ms",
            JsValue::from_f64(f64::from(claimed.sim_time_ms)),
        );
        set(
            "command_count",
            JsValue::from_f64(recording.command_count() as f64),
        );
        set("map", JsValue::from_str(&format!("{}", recording.world())));
        set(
            "physics",
            JsValue::from_str(&format!("{}", recording.physics())),
        );
        set("s3d", Uint8Array::from(bytes.as_slice()).into());

        log::info!(
            "run finished: {} ms, digest {:016x}, {} commands, {} bytes of .s3d with a \
             per-command checksum trace",
            claimed.run_time_ms.unwrap_or(0),
            claimed.digest,
            recording.command_count(),
            bytes.len(),
        );
        LAST_RUN.with(|last| *last.borrow_mut() = Some(run.clone()));
        call_page("onRunFinished", &[run.into()]);
    }
}
