# Straf3 — Vision, Scope, and Development Goals

## Governing thesis

> **The whole product is the quality of the movement.**

This is the decision rule above every feature list, technology choice, content plan, and business model. If a choice weakens movement feel, mastery, responsiveness, clarity, or competitive integrity, it is wrong for Straf3—even if it would make another part of the product easier to build or market.

## Vision

Straf3 is a modern competitive movement sport for native desktop and the web. Players learn a compact but extraordinarily deep movement language, discover routes through abstract futuristic arenas, and pursue per-map world records whose evidence can be replayed, raced, analyzed, and preserved.

The game combines three first-class modes:

1. **Verified asynchronous competition** — the central record sport and the source of its highest prestige.
2. **Live multiplayer racing and practice** — head-to-head races, shared free-practice spaces, tricking, spectating, and events.
3. **Training and unranked play** — a permanent mastery system that teaches movement primitives, then combinations, then the open-ended application of both in real maps.

Straf3 is also the foundation of a full ecosystem. A map is not merely a file: it has a page, versions, records, ghosts, analysis, servers, editing and remixing entry points, and a durable competitive history. A player should be able to follow a link and immediately play a map, race a ghost, watch a record, open an editor, or join a server.

The native client and browser client are peers. They share the same simulation truth, content, competitive access, and live multiplayer ecosystem. The custom Rust engine exists to serve this product first; reusable engine systems are extracted as the needs of Straf3 prove them.

## The decisive proof

The earliest decisive proof that Straf3 works is not feature count, visual spectacle, or a large audience:

> **Movement experts voluntarily grind the same small map because improving their time remains compelling.**

This is the first proof of product truth. Every broader system amplifies that truth; none can substitute for it.

## Product identity

### What Straf3 is

- A competitive movement sport.
- A deep first-person mastery game.
- A native-and-browser game built around permanent, verifiable runs.
- A map, training, replay, analysis, and community platform.
- A creator environment with first-class AI assistance and an eventual RL-driven map-development loop.
- A flagship game built on a custom, modular Rust engine.

### Who it is for

The initial audience spans:

- movement enthusiasts whose expectations have been modernized by contemporary games;
- speedrunners and competitive runners;
- existing Defrag and arena-FPS veterans;
- broader FPS players who already enjoy advanced movement and want a path into deeper mastery.

The product should respect experts without requiring prior Defrag knowledge. Accessibility comes primarily from excellent teaching, feedback, practice structure, and course design—not by removing the depth experts value.

### World and tone

Straf3 uses pure or near-pure abstract arenas with minimal fiction. Its visual identity is a modern, stylized, light-themed, minimal science-fiction aesthetic, drawing broad inspiration from games such as Diabotical and Reflex Arena and from strong contemporary Defrag mapping.

The art exists to make space, motion, collision, speed, routes, and competitive action beautiful and legible. Extensive narrative, lore, characters, and worldbuilding are not current priorities.

## The player promise

A first session should deliver a quick taste of flow followed by a visible path toward mastery.

An excellent run should support all of these experiences:

- **Flow:** movement becomes rhythmic, musical, and apparently effortless.
- **Mastery:** improvement is earned, understandable, and attributable to player decisions.
- **Adrenaline:** a record attempt or live race carries pressure and spectacle.
- **Discovery:** routes, transitions, techniques, and combinations feel inventable rather than exhausted.

The player should always understand that a better run is possible and have useful evidence about how to pursue it.

## Movement vision

### Straf3 owns its ruleset

Quake 3, VQ3, CPM, and Defrag are foundations and sources of accumulated design knowledge; they are not preservation constraints. Straf3 will develop a distinct canonical movement ruleset through prototyping, measurement, and expert playtesting.

The intended vocabulary includes:

- strafejumping and circle jumping;
- ramp interaction and boosts;
- steps, clipping, overbounce, and other valuable emergent collision interactions;
- CPM-style air control, double jumps, and related extensions;
- crouch slides, dashes, wall interaction, and other modern movement ideas that survive testing;
- modular experimental abilities outside the primary ranked ruleset.

The final canonical set is not predetermined. Mechanics earn their place through the quality and depth of the movement they create.

### Compact input, combinatorial depth

Players use a compact, universal input language. Advanced behavior should emerge from timing, context, geometry, speed, and the interaction of mechanics rather than from a large ability bar or many isolated buttons.

Every canonical mechanic should aim to be:

- simple to invoke but difficult to master;
- meaningfully composable with several other mechanics;
- deterministic and clearly attributable;
- capable of producing route choices, not merely one mandatory execution;
- visually and audibly readable to players and spectators.

### Movement anti-goals

Straf3 should resist:

- automation that replaces execution or timing;
- opaque exploits that cannot be learned through feedback;
- hard speed caps or excessive forced slowdown;
- cooldown rotations that replace momentum mastery;
- mechanic overload that weakens the coherence of the shared movement language.

### Assists are an experiment, not a present priority

Training assists and persistent movement assists may be explored later. A ranked assist would at minimum need to be deterministic, transparent, equally available, and compatible with the ultimate skill ceiling. The architecture should allow controlled experimentation without requiring the product vision to settle the rules now.

## Game modes

### 1. Verified asynchronous competition

This is the backbone of the sport.

- Players cross a start line, complete a map, and pursue a personal or world-best time.
- Per-map world records carry the highest competitive prestige.
- Runs are verified continuously rather than trusted solely because their final state matches.
- Any published record can become a replay, ghost, comparison target, and subject of analysis.
- Map and ruleset changes create new versioned leaderboards; the old record books remain intact.

Seasonal circuits, events, and broader ratings may be added, but they do not replace the clarity of the per-map record.

### 2. Live multiplayer

The full scope includes:

- official regional matchmaking and race servers;
- live head-to-head races;
- shared free-practice and tricking servers;
- community-hosted public servers with link-based joining;
- private friend, team, and practice sessions;
- persistent practice spaces with records and trick challenges;
- live spectating and tournament broadcast sessions.

Live competition is a distinct discipline alongside asynchronous records, not a replacement for them.

### 3. Training and unranked play

Training is a full mode rather than a disposable tutorial.

- Authored courses progress from fundamentals to elite techniques.
- Individual trick jumps teach the primitives of the movement language.
- Combination courses teach players to connect primitives into fluent sequences.
- Real maps then ask players to discover and tailor their own combinations.
- Adaptive exercises and personalized progression respond to a player's demonstrated needs.
- Unranked play provides room to practice, explore, experiment, and use provisional mechanics.

The long-term training vision may include AI/RL demonstrations and deeper coaching, but the confirmed center is structured skill acquisition followed by open-ended composition.

## Maps and content

### Required map disciplines

The official portfolio should deliberately support:

- fast flow and sustained speed;
- technical precision and difficult trick jumps;
- route discovery, shortcuts, and emergent techniques;
- long-form endurance and full-map movement combinations.

Different maps may emphasize one discipline, but the total game should support all of them.

### Content trust tiers

Maps belong to one of three broad trust tiers:

1. **Official** — curated as part of the authoritative competitive core.
2. **Verified community** — community-created content that passes defined technical and quality gates.
3. **Experimental** — provisional content, mechanics, and experiences with fewer competitive guarantees.

The official core is governed through studio curation while community tools and servers remain open. The precise outer boundary of creator scripting, custom rules, and deep modding is deliberately unresolved.

### Map publication gates

A verified or official map should demonstrate:

- deterministic collision and identical gameplay behavior across platforms;
- performance and visual readability on target hardware;
- human-completable routes with declared skill and mechanic coverage;
- RL-assisted analysis of routes, exploits, difficulty, and flow;
- human curation for quality, originality, and competitive value.

Automation informs curation; it does not replace human judgment.

## Map editor, AI, and RL

### One editor core

Native and browser editing share one editor core, with interfaces adapted to each platform. The web is not a read-only portal: creating, publishing, and community participation are part of the browser product.

### First-class AI copilot

The AI copilot should eventually be able to:

- create and modify geometry, materials, entities, and routes from intent;
- generate training obstacles for named mechanics and skill levels;
- analyze routes, exploits, difficulty, flow, readability, and performance;
- iterate with RL agents and propose validated revisions;
- expose editor operations through MCP and other automation interfaces.

The near-term identity is **copilot**, not autonomous replacement: a creator remains able to direct, inspect, refine, and accept changes.

### RL development path

The intended progression is:

1. teach agents individual movement primitives;
2. compose primitives into movement combinations;
3. connect learned combinations into complete map runs;
4. use agents to assess solvability, route diversity, difficulty, exploits, and flow;
5. use the results to generate and refine training content and full maps for specified skill levels;
6. eventually support more autonomous map generation, still subject to technical and human curation gates.

AI and RL are both product capabilities and internal content-development accelerators.

## Competition, records, and sporting history

### A record is evidence, not just a time

Every ranked world record should expose:

- a downloadable replay and raceable ghost;
- versioned map, movement rules, engine, and platform metadata;
- a public verification result with continuous run-integrity evidence;
- input, speed, route, and comparison analysis;
- moderation, challenge, and adjudication history when a result is disputed.

### Permanent, versioned history

- A material map or movement update creates a new leaderboard version.
- Old record books remain browsable and meaningful.
- Old runs remain replayable through versioned simulation runtimes.
- Content and simulation identities are explicit; a record never silently changes meaning underneath the player.

Replay preservation is a product promise and an engineering responsibility.

## Browser, native, and the web platform

### Browser from the beginning

The browser is not a later port or companion-only experience. The first meaningful public release targets:

- full competitive play;
- live multiplayer;
- training and unranked play;
- timed runs, ghosts, verification, and leaderboards;
- editing, publishing, and community participation;
- equivalent visual features and graphical fidelity, within platform-appropriate quality scaling.

### Native and browser parity

The non-negotiable shared truth is:

- identical simulation outcomes;
- identical input interpretation at the simulation boundary;
- identical maps and collision;
- identical replay results;
- access to the same ranked competition and live multiplayer.

The implementations may use platform-specific shells and optimizations, but the competitive game must not fork into browser and native variants.

### The web as connective tissue

A map page should allow a player to:

- launch the map in browser or native;
- browse records and race any available ghost;
- watch, analyze, and compare runs;
- open, fork, and remix the map in the editor;
- start or join a live practice or racing server.

Similar durable links should exist for players, records, replays, events, and servers. Joining a community should feel as direct as following a link.

## Technical and production vision

### A custom Rust engine serving Straf3

The engine is the abstract technical core of the product, but Straf3 is developed first. Reusable systems are extracted from demonstrated game needs rather than designed as a separate general-purpose engine in advance.

The engine should:

- be modular and explicit about subsystem boundaries;
- use established libraries, patterns, and algorithms where they meet the requirements;
- avoid reinvention for its own sake;
- support native and browser targets from the beginning;
- make simulation, rendering, tools, networking, content, and services independently testable;
- support the iteration speed required by a small team heavily augmented by AI.

### Patterns serve constraints

ECS, ability systems, data-oriented design, and other proven game-development patterns should be applied selectively.

- The deterministic simulation remains purpose-built, explicit, and testable.
- Advanced movement can be described and composed through ability-system concepts and data.
- No pattern is required to own a subsystem when profiling, prototypes, or correctness constraints show a better design.
- "Best practice" means demonstrated fitness for Straf3, not architectural fashion.

### The seam remains a principle, not necessarily today's crate graph

The lasting architectural idea is a hard dependency boundary:

- **Below the seam:** deterministic simulation truth, collision, map semantics, and replay/verification logic. No filesystem, wall clock, GPU, nondeterministic randomness, or platform-owned behavior.
- **Above the seam:** platform input, rendering, audio, networking transport, UI, editor presentation, storage, services, and orchestration.

The exact crates and internal layout may change. The property to preserve is that the competitive result is a pure, reproducible consequence of versioned state and commands.

## Quality as correctness

### 1. Movement feel

Movement must remain responsive, expressive, learnable, compositional, and worth repeating. Expert playtesting is a core development instrument, not a late polish pass.

### 2. Determinism

Competitive simulation must be bit-identical across supported native, browser, headless, and verification targets. Determinism applies continuously across a run, not only to the final checksum.

### 3. Frame pacing and latency

Performance is judged through measurement, not average frame-rate claims.

- Desktop development targets the 240 Hz class for competitive play.
- Browser development targets the 120 Hz class on capable hardware.
- Frame pacing, refresh behavior, and end-to-end input latency have published budgets and regression tests.
- Visual settings may scale, but competitive timing and movement truth do not.

### 4. AAA production quality

For Straf3, AAA quality means:

- exceptional response, latency, pacing, and movement feel;
- cohesive high-end art, lighting, effects, animation, and audio;
- deeply polished maps, training, UI, editor, and creator workflows;
- reliable multiplayer, records, sharing, and community services.

It does not primarily mean photorealism, narrative scale, or feature volume.

### 5. Built-in diagnostics

Developer-grade truth should be available as product-grade tooling:

- frame pacing, refresh, and end-to-end latency inspection;
- determinism checksums and local replay verification;
- input, velocity, angle, collision, and route visualization;
- cross-run comparison and regression analysis;
- exportable diagnostics for bug reports and competitive disputes.

## Development model

### Team assumption

The planning assumption is a small core team heavily augmented by AI.

### Parallel workstreams

Development proceeds across parallel but continuously integrated workstreams:

1. **Movement and deterministic simulation**
2. **Engine, rendering, audio, and platform runtime**
3. **Game modes, training, UI, and player experience**
4. **Maps, editor, content pipeline, AI, and RL**
5. **Web platform, identity, records, verification, and analysis**
6. **Networking, live servers, spectating, and community hosting**
7. **Devtools, testing, performance, and release infrastructure**

Parallel work does not mean waiting until the end to connect independent systems. Each workstream should contribute to frequent integrated proofs using the same maps, commands, versions, identities, and quality budgets.

### Recommended integration proofs

These are evidence gates, not a proposal to postpone the other workstreams:

#### Proof A — Movement truth

- A small test arena supports the intended foundational vocabulary.
- Native, browser, and headless results are bit-identical.
- Pacing and latency are measured under intentionally hostile frame schedules.
- Invited Defrag and movement experts voluntarily repeat runs to improve.

#### Proof B — Complete personal-best loop

- Training leads into a timed map.
- A run records, verifies, saves, replays, and becomes a ghost.
- The player can compare inputs, route, and movement against the ghost.
- The result is linkable and playable in native and browser clients.

#### Proof C — Connected competitive loop

- A versioned map has an auditable leaderboard and permanent record history.
- Players can race records asynchronously and challenge them through supported processes.
- Browser and native players compete in the same record book.

#### Proof D — Live movement sport

- A link joins an official or community live race/practice server.
- High-refresh movement remains responsive and fair under real network conditions.
- Spectators can follow and understand the action.

#### Proof E — Creator loop

- A creator edits, validates, publishes, shares, and revises a map.
- The same map behaves identically in native, browser, server, and verification environments.
- AI assistance operates through inspectable editor actions.

#### Proof F — AI/RL content loop

- Agents demonstrate primitives and combinations.
- Analysis finds useful route, difficulty, exploit, or flow information.
- A proposed map revision improves a declared target and passes human review.

#### Proof G — Production scale

- Art, audio, content, tools, services, and operations meet explicit quality budgets.
- The team can add maps, mechanics, training content, and platform features without destabilizing competitive history.

## Development goals

### Goal 1 — Establish the best movement in the category

Create a coherent canonical ruleset that experts want to grind and newcomers can learn. Treat feel, response, compositional depth, route diversity, and readable feedback as one inseparable design problem.

**Primary evidence:** repeated expert play on limited content; continued discovery; understandable improvement; no pressure to add rewards merely to make repetition tolerable.

### Goal 2 — Make every run reproducible and permanent

Ensure that every competitive result has a versioned simulation, map, command stream, continuous integrity evidence, and a future playback path.

**Primary evidence:** bit-identical results on all verification targets; old-version replay tests; versioned leaderboards that never reinterpret past records.

### Goal 3 — Deliver competitive responsiveness everywhere

Build native and browser clients around measured high-refresh pacing and latency rather than treating the browser as a reduced product.

**Primary evidence:** explicit 240 Hz-class desktop and 120 Hz-class browser budgets; repeatable latency measurements; stable pacing under hostile scheduling and real play.

### Goal 4 — Build the complete mastery loop

Connect training primitives, combination courses, real maps, personal bests, ghosts, analysis, and records into one continuous player journey.

**Primary evidence:** a new player reaches flow quickly, understands a next improvement, and graduates from authored instruction to self-directed route and technique work.

### Goal 5 — Establish the per-map record sport

Make each map's world record a prestigious, inspectable, raceable, and permanent achievement.

**Primary evidence:** a record page contains sufficient evidence to trust, learn from, race, compare, moderate, and preserve the run.

### Goal 6 — Add live racing without weakening the asynchronous core

Support official and community live servers, racing, shared practice, tricking, private sessions, and spectating through link-based access.

**Primary evidence:** native and browser users join the same session easily; live competition remains readable, responsive, and compatible with the canonical movement rules.

### Goal 7 — Make maps a programmable product surface

Create a shared editor and publication pipeline in which a map carries content, validation, versions, trust tier, records, analysis, and community context.

**Primary evidence:** one map moves from creation to validation, publication, play, record competition, revision, and historical preservation without ad hoc steps.

### Goal 8 — Make AI and RL genuine creative leverage

Use AI first as an inspectable editor copilot, then use trained movement agents for analysis, coaching, validation, iteration, and eventually autonomous generation.

**Primary evidence:** AI/RL improves creator throughput or map quality against a declared measure while human creators retain control and final judgment.

### Goal 9 — Build the ecosystem around durable links

Connect maps, players, runs, ghosts, servers, editing, and spectating through a coherent web platform.

**Primary evidence:** a shared link turns intent into action—play, watch, race, edit, or join—with minimal friction.

### Goal 10 — Grow a reusable engine through the game

Build a modular Rust engine that provides the abstract core Straf3 needs, extracting reusable systems only after game requirements demonstrate the abstraction.

**Primary evidence:** engine modularity accelerates new Straf3 features and platforms; it does not create a separate roadmap that delays the game.

## Decision hierarchy

When goals conflict, use this order:

1. Movement quality and mastery
2. Deterministic competitive integrity
3. Measured responsiveness, pacing, and latency
4. Coherence and fairness of the sport
5. Native/browser competitive parity
6. Creation, community, and link-based access
7. Visual and production ambition
8. Feature breadth, reuse, and business-model optimization

This hierarchy does not make the lower items optional. It identifies what they must serve.

## Confirmed anti-goals

- No pay-to-win or sale of movement advantages.
- No automation that substitutes for skillful execution.
- No opaque movement outcomes that players cannot investigate and learn.
- No arbitrary speed suppression used to compensate for weak course or mechanic design.
- No cooldown-centric ability rotation replacing momentum, geometry, timing, and route mastery.
- No architectural pattern applied merely because it is fashionable or common in large studios.
- No browser product that silently becomes a mechanically different or competitively secondary game.

## Deliberately deferred decisions

These questions should remain visible without blocking present game development:

- the final business model; the current provisional direction is a free core game, possibly with subscriptions for leaderboards or other services;
- the exact subscription boundaries and operating economics;
- source-code, protocol, SDK, and server licensing;
- the deepest limits of creator scripting, custom rulesets, modding, and commercial content;
- whether creator rulesets can ever enter the official competitive canon;
- the precise rules for training assists or ranked movement assists;
- console, mobile, and other platform roles beyond Windows, Linux, macOS, and the browser;
- the final mechanic set, input mapping, simulation frequency, and network model;
- exact content counts, release dates, and staffing plans.

Deferred does not mean ignored. The engine and data model should avoid needless barriers to later experimentation, but speculative flexibility must not weaken current correctness or delay the core game.

## Working risks to validate

No formal existential-risk list was selected during the interview. The following are therefore working risks inferred from the scope, not settled conclusions:

- parallel engine, game, browser, web, tools, online, and AI development may exceed the integration capacity of a small team;
- expanding beyond Quake-derived movement may weaken coherence unless every mechanic meets the shared design doctrine;
- browser/native competitive and visual parity may conflict with AAA presentation and high-refresh performance targets;
- permanent replay execution may become expensive as maps, rulesets, compilers, platforms, and services evolve;
- AI/RL systems may generate volume or metrics without producing maps that expert humans consider excellent;
- a general-engine roadmap could compete with the game despite the stated game-first extraction model;
- service operation, moderation, verification, and community hosting may demand substantially different expertise from movement and engine development.

These risks should be tested with integrated proofs rather than addressed only through up-front architecture.

## Open questions for later interviews

1. What exact competitive rules determine whether a new mechanic enters the canonical profile?
2. How are map trust tiers awarded, reviewed, appealed, and revoked?
3. What parts of the creator sandbox can affect ranked play?
4. What long-term operating model funds verification, storage, matchmaking, moderation, and permanent replay support?
5. What is the source and protocol openness policy?
6. What accessibility capabilities are required beyond teaching and training?
7. What privacy, identity, moderation, and child-safety model fits the community platform?
8. What spectator presentation makes high-level movement understandable to new viewers?
9. What content and quality threshold defines alpha, beta, 1.0, and post-launch operation?
10. Which current implementation choices should be retained, rewritten, or replaced after the vision is accepted?

## One-paragraph north star

Straf3 should become the definitive modern movement sport: a native-and-browser game whose compact controls produce near-limitless mastery across beautiful abstract arenas; whose movement feels exceptional at competitive refresh rates; whose records are bit-identical, auditable, raceable, and permanently replayable; whose training turns primitives into personal expression; whose live servers and web pages make play instantly shareable; and whose shared editor, AI copilot, and RL agents help a small team and its community create an enduring supply of excellent maps without compromising the canonical sport.
