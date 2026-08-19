# Straf3 — Vision, Product Principles, and Development Direction

**Status:** Working vision
**Purpose:** Define the intended identity, player experience, product direction, and development principles for Straf3. This document is intended to align both humans and AI agents working on the game.

## 1. North star

**Straf3 is a next-generation competitive first-person movement game, strongly inspired by Quake 3 Defrag but built to establish its own identity for the modern era.**

Movement is the center of the game.

Movement in Straf3 is both **an art and a science**.

It is a science because speed, acceleration, angles, timing, collision, routes, inputs, and execution can be understood, measured, analyzed, and optimized.

It is an art because great movement should also contain flow, creativity, improvisation, style, discovery, and personal expression.

The fundamental design goal is:

> **Easy to learn. Difficult to master. Deep enough that mastery continues to reveal new possibilities.**

Straf3 should give new players an understandable path into advanced movement while preserving the depth required for expert players to spend hundreds or thousands of hours improving.

The quality of the movement is the most important property of the game. Technology, graphics, maps, multiplayer systems, AI, web infrastructure, and other features exist to support that experience.

---

## 2. Product identity

Straf3 is:

* a competitive first-person movement game;
* a multiplayer game that can also be played entirely alone;
* a game about movement mastery, execution, route discovery, flow, and competition;
* a modern successor in spirit to the type of experience pioneered by Quake 3 Defrag, without being constrained to reproduce Defrag exactly;
* a game with one canonical competitive movement ruleset;
* a game where additional movement profiles and experimental rulesets can be explored without fragmenting the primary competitive identity;
* a native game with a major ambition to also make the complete game playable directly through the web;
* a game connected to a first-class web portal that acts as its launcher, competitive hub, content browser, replay viewer, server browser, and community platform;
* a flagship game built using a custom modular Rust engine;
* a game where maps are critical to the quality of the experience;
* a game whose development process makes deep use of AI agents, tools, automation, and reinforcement learning.

Straf3 should respect its Quake and Defrag heritage while being willing to change, extend, modernize, and improve upon it.

The objective is not preservation.

The objective is to create **Straf3 movement**.

---

## 3. The player promise

A new player should be able to begin moving, understand why something worked or failed, experience moments of flow early, and see a clear path toward deeper mastery.

An expert player should continue finding reasons to improve.

A great Straf3 run can contain several qualities at once:

### Flow

Movement becomes rhythmic and continuous. Individual inputs disappear into a larger sequence that feels almost effortless when performed correctly.

### Mastery

Improvement comes from understanding and execution. Better players should be able to explain, demonstrate, and reproduce why they are faster.

### Precision

Small differences in timing, angle, positioning, speed, and technique can matter.

### Creativity

Players should be able to combine movement mechanics and map geometry in expressive ways.

### Discovery

Maps should leave room for new routes, transitions, techniques, optimizations, and combinations to emerge.

### Competition

Records and live races should create pressure, rivalry, spectacle, and meaningful achievements.

The game should balance **execution and discovery**.

A map should neither reduce entirely to executing one prescribed solution nor become so unconstrained that deliberate course design loses meaning.

---

## 4. Movement

### 4.1 Quake and Defrag are the foundation, not the boundary

Straf3 begins from the accumulated knowledge of Quake movement, Defrag, VQ3, CPM, and related movement games.

Important concepts include, among others:

* strafejumping;
* circle jumping;
* air acceleration and air control;
* ramp interaction;
* speed preservation;
* steps and collision interaction;
* double jumps and related chained techniques;
* overbounces and other valuable emergent interactions where they create understandable depth;
* crouch sliding;
* carefully designed new movement mechanics.

The exact canonical movement system must be discovered through implementation, experimentation, measurement, map design, and expert playtesting.

No mechanic is included merely because it existed in an older game.

Likewise, new mechanics should not be added merely because they are modern.

Every mechanic must improve the movement language.

### 4.2 Compact input, deep results

The input vocabulary should remain relatively compact.

Depth should emerge primarily through:

* timing;
* direction;
* geometry;
* speed;
* momentum;
* positioning;
* sequencing;
* transitions;
* mechanical interactions;
* route decisions.

Advanced movement should come from combining understandable primitives rather than accumulating a large collection of unrelated abilities.

A strong mechanic should generally be:

* understandable at a basic level;
* difficult to perfect;
* responsive;
* deterministic;
* composable with other mechanics;
* useful in multiple situations;
* capable of supporting player expression;
* readable through visual, audio, and diagnostic feedback.

### 4.3 One canonical competitive ruleset

Straf3 should eventually establish one authoritative movement ruleset that defines the primary competitive game.

This ruleset is what it means to play canonical Straf3.

Development should still support experimentation with:

* alternate movement profiles;
* provisional mechanics;
* modified physics;
* training experiments;
* unusual map concepts;
* research-oriented rulesets.

Experiments should be easy to conduct without prematurely incorporating them into ranked play.

A mechanic enters the canonical ruleset because testing demonstrates that it improves Straf3.

### 4.4 Movement anti-goals

Avoid mechanics or systems that:

* automate execution that should belong to player skill;
* make important outcomes opaque or impossible to understand;
* impose arbitrary speed limits to compensate for poor design;
* replace momentum mastery with cooldown rotations;
* introduce complexity without increasing meaningful depth;
* reduce movement to memorizing ability sequences;
* weaken responsiveness for visual spectacle;
* make the canonical movement language incoherent.

---

## 5. Core game modes

The initial game is organized around three primary forms of play.

Other modes may be explored later.

### 5.1 Ranked time attack

Players run maps individually and attempt to achieve the fastest possible completion time.

This is the primary record-oriented mode.

Players compete against:

* their own personal best;
* friends;
* other players;
* map leaderboards;
* world records;
* replay ghosts.

Runs should be recordable, replayable, analyzable, and verifiable.

Per-map records are an important part of Straf3's competitive identity.

### 5.2 Live competitive racing

Players compete against each other live.

The exact formats can evolve, but the foundation is direct multiplayer racing using the same movement system that powers solo record play.

Live competition should preserve:

* movement responsiveness;
* competitive integrity;
* clarity;
* fairness;
* spectatability;
* connection to the same maps and movement language used elsewhere.

Live multiplayer is not a secondary version of the game. It is one of the core ways Straf3 is played.

### 5.3 Training and unranked play

Players can run maps without ranked consequences.

This supports:

* learning;
* practice;
* experimentation;
* route exploration;
* mechanical experimentation;
* warmup;
* casual play;
* training maps.

Training should be a permanent part of the game rather than a disposable tutorial.

---

## 6. Training and movement primitives

Straf3 should treat movement as a language that can be decomposed into **movement primitives**.

Training maps should isolate and teach these primitives.

Examples may include:

* initial acceleration;
* circle jumps;
* strafejump timing;
* air-control changes;
* ramp interactions;
* precision landings;
* crouch slides;
* chained transitions;
* combinations of several mechanics.

Training should then progress from isolated primitives toward combinations.

Eventually, players leave controlled training scenarios and apply those techniques creatively inside real maps.

The intended learning progression is approximately:

**primitive → controlled repetition → combination → route segment → complete map → optimization → personal expression**

Importantly, these training environments are not intended only for human players.

**Humans and reinforcement-learning agents should train using the same fundamental movement environments.**

This creates a shared language between:

* game design;
* player training;
* movement analysis;
* RL development;
* map evaluation;
* difficulty estimation.

---

## 7. Maps

Maps are one of the most important components of Straf3.

Movement mechanics cannot be evaluated independently from the spaces in which those mechanics are used.

Great maps should expose different aspects of movement.

The first-party map portfolio should deliberately include different emphases such as:

* sustained flow and speed;
* technical execution;
* precision;
* difficult movement combinations;
* route choice;
* shortcuts;
* discovery;
* experimental geometry;
* long-form endurance;
* transitions between different movement styles.

Some maps may emphasize pure execution.

Others may emphasize discovering better routes.

The game as a whole should support both.

### First-party maps first

The immediate focus is building excellent Straf3 maps ourselves.

Community-created maps, extensive modding ecosystems, creator economies, and similar systems should not dictate the early architecture or product roadmap.

The editor should nevertheless be designed as a serious internal production tool and should avoid unnecessary assumptions that would prevent broader use later.

Quality comes before content volume.

A small number of exceptional maps is more valuable than a large number of mediocre ones.

---

## 8. Competition, records, ghosts, and replays

Competitive history remains a major part of Straf3.

A record should be more than a leaderboard number.

A competitive run should be capable of becoming:

* a replay;
* a ghost;
* a comparison target;
* an analysis target;
* evidence for a record;
* material for learning;
* material for spectating.

Players should be able to understand how another player moved faster.

Useful analysis may include:

* position;
* velocity;
* acceleration;
* view angle;
* inputs;
* movement state;
* route;
* split times;
* differences against another run.

### Competitive integrity

Ranked results should be reproducible and verifiable.

The simulation should be deterministic enough that authoritative run data can be replayed and independently checked.

Verification should consider the complete run rather than trusting only its final result.

Map, engine, or canonical movement changes that materially affect competition should be versioned.

Historical records should not silently change meaning when the game evolves.

Where technically practical, old competitive runs should remain replayable and understandable.

---

## 9. Native game and browser game

### Native is uncompromised

The native Straf3 client is the uncompromised reference experience.

It should prioritize:

* very high refresh rates;
* low input latency;
* stable frame pacing;
* high visual quality;
* competitive reliability.

Desktop competitive play should be engineered for high-refresh hardware, including the 240 Hz class where hardware permits it.

### Full browser play is a major goal

We want the actual game to be playable directly in a modern browser if performance, latency, platform capabilities, and smoothness can meet an acceptable quality bar.

The browser version should aim to look visually equivalent to the native game, with platform-appropriate optimizations where necessary.

The important concern is not intentionally reducing the browser experience. The concern is whether browser limitations allow sufficiently smooth competitive gameplay.

Development should therefore pursue browser gameplay aggressively but evaluate it based on measured reality rather than ideology.

Browser performance targets should aim toward high-refresh play, including roughly the 120 Hz class on capable systems where practical.

If complete competitive browser play proves unsuitable in some environments, the web platform must still provide first-class access to the rest of the Straf3 ecosystem.

At minimum, the browser should be excellent for:

* discovering maps;
* viewing maps;
* browsing leaderboards;
* viewing profiles;
* watching replays;
* analyzing runs;
* browsing servers;
* joining servers;
* sharing links;
* launching content into the native client when necessary.

Native and browser simulation should share the same underlying gameplay truth wherever the browser game is supported.

---

## 10. The Straf3 web portal

Straf3 should not require a traditional launcher separate from its web ecosystem.

The **web portal itself can serve as the launcher**.

It should become the primary connective layer around the game.

Important entities include:

* maps;
* players;
* profiles;
* runs;
* records;
* leaderboards;
* replays;
* ghosts;
* servers;
* events.

These should have durable web identities and links.

A map page should eventually allow a player to:

* understand the map;
* launch it;
* play it in-browser where supported;
* open it in the native game;
* inspect its records;
* watch the world record;
* race a ghost;
* analyze runs;
* find active servers running it;
* join a server.

A server link should lead naturally toward joining that server.

A replay link should lead naturally toward watching or analyzing that replay.

A map link should lead naturally toward playing that map.

The boundary between website and game should feel thin.

---

## 11. Multiplayer and servers

Straf3 is fundamentally multiplayer even though much of the game can be enjoyed alone.

The multiplayer ecosystem should eventually support:

* live competitive races;
* practice sessions;
* private sessions;
* official servers;
* community-hosted servers;
* spectators;
* events;
* link-based joining.

Players practicing independently and players racing live should still inhabit the same broader game ecosystem.

Networking should never be allowed to compromise the quality of local movement.

Server architecture, prediction, reconciliation, and race rules must be designed around the requirements of the movement system rather than forcing the movement system to fit generic multiplayer assumptions.

---

## 12. AI-first map creation

The map editor is strategically important.

The current concept is **AI-first**, but this direction is intentionally still exploratory.

AI-first does not mean that humans disappear from the process.

It means the primary production workflow may increasingly look like:

**human intent → AI agent → tools → editor → rendered result → inspection → iteration**

Agents should eventually be capable of operating map-development systems using whatever interfaces prove appropriate, potentially including:

* structured tools;
* editor APIs;
* MCP;
* command-line interfaces;
* skills;
* scripts;
* screenshots;
* rendered views;
* scene information;
* game telemetry;
* automated playtests.

The exact interface is not predetermined.

The editor should expose operations in ways that agents can reason about and manipulate reliably.

At the same time, a human developer or designer should be able to visually follow what is happening inside the editor, inspect the map, intervene, and direct the agent.

The goal is not an invisible autonomous content generator.

The goal is a development environment in which humans and increasingly capable AI agents can build and iterate on maps together.

This area requires experimentation before its final architecture is decided.

---

## 13. Reinforcement learning and movement agents

RL agents are a major research and development direction for Straf3.

The initial objective is to create agents capable of learning the same movement language that human players learn.

The intended development progression is approximately:

1. learn individual movement primitives;
2. perform primitives reliably;
3. combine primitives;
4. solve movement sequences;
5. construct complete routes;
6. complete maps;
7. optimize routes and execution;
8. analyze alternative routes;
9. assess difficulty and movement requirements;
10. contribute to map testing and generation.

### Skill as a variable

The objective is not merely to create the strongest possible agent.

Agents should be able to represent different levels of player capability.

Skill level should eventually become an input or controllable property.

This could include differences in:

* timing precision;
* reaction;
* route knowledge;
* mechanical vocabulary;
* consistency;
* optimization ability;
* execution errors.

This makes it possible to ask questions such as:

* Can a beginner complete this?
* Where does an intermediate player struggle?
* Which route would an advanced player discover?
* Is an expert route substantially different?
* Is this jump appropriate for the intended skill level?
* Does this map accidentally require techniques above its target level?

### RL-assisted map development

The long-term loop is:

**learn primitives → learn combinations → solve routes → model skill levels → test maps → analyze results → propose or generate changes → test again**

Agents may eventually help identify:

* impossible routes;
* unintended shortcuts;
* alternative routes;
* difficulty spikes;
* weak sections;
* flow problems;
* redundant geometry;
* interesting emergent movement;
* opportunities for better combinations.

Eventually, the same systems may help generate or modify maps for specified skill levels.

Human judgment remains the final measure of whether a map is actually good.

An agent solving a map does not prove that the map is fun.

---

## 14. Custom modular Rust engine

Straf3 should be built on a custom modular Rust engine.

The engine exists first to serve Straf3.

It should not become an independent general-purpose engine project that competes with development of the game.

Reusable systems should emerge from real Straf3 requirements.

The engine should favor:

* clear subsystem boundaries;
* modularity;
* testability;
* deterministic simulation;
* high performance;
* native and web targets;
* strong tooling;
* automation;
* agent-accessible interfaces;
* rapid iteration.

Established libraries and algorithms should be used when they solve the problem well.

We should not reinvent technology merely because the engine is custom.

### Deterministic simulation boundary

The competitive movement simulation should have a strong architectural boundary from platform-dependent systems.

Simulation truth should not depend on things such as:

* wall-clock timing;
* GPU state;
* filesystem behavior;
* nondeterministic randomness;
* platform-specific side effects.

Rendering, audio, UI, networking transport, storage, web integration, and operating-system interfaces can remain outside this deterministic core.

The exact crate graph and architecture may evolve.

The important property is that a competitive movement result is a reproducible consequence of known state, inputs, content, and rules.

---

## 15. Performance and responsiveness

Movement quality depends directly on technical quality.

Frame pacing, latency, simulation behavior, input processing, and rendering are therefore game-design concerns.

Performance should be measured rather than inferred from average FPS.

Important metrics include:

* input-to-simulation latency;
* simulation-to-display latency;
* frame-time distribution;
* missed frame deadlines;
* refresh synchronization;
* input sampling behavior;
* network effects during live play.

Performance regressions that damage movement feel should be treated as gameplay regressions.

Native Straf3 should aim at extremely high competitive responsiveness.

Browser Straf3 should aim as close to that standard as the platform allows.

Competitive simulation behavior must not change merely because graphical settings change.

---

## 16. Visual and audio direction

Straf3 should have AAA production quality without pursuing photorealism or unnecessary content scale.

The visual identity is:

* stylized;
* modern;
* minimal;
* science-fiction;
* abstract or near-abstract;
* clean;
* highly readable in motion.

The world exists primarily to make movement, geometry, speed, routes, and competition beautiful and understandable.

Art should support gameplay.

Lighting should support gameplay.

Effects should support gameplay.

Animation should support gameplay.

Audio should support gameplay.

Visual complexity that damages spatial understanding or competitive readability should be avoided.

AAA quality for Straf3 means exceptional execution across:

* movement feel;
* responsiveness;
* rendering;
* lighting;
* effects;
* animation;
* sound;
* music where appropriate;
* UI;
* maps;
* networking;
* tooling;
* replay presentation;
* web presentation;
* overall polish.

It does not require photorealism, cinematic storytelling, enormous world scale, or feature count for its own sake.

---

## 17. Diagnostics and analysis

Developer-grade truth should be accessible throughout development.

Useful tooling includes:

* frame pacing inspection;
* latency inspection;
* simulation checksums;
* replay verification;
* velocity visualization;
* acceleration visualization;
* input visualization;
* view-angle visualization;
* collision inspection;
* movement-state inspection;
* route visualization;
* cross-run comparison;
* profiling;
* automated regression tests.

These systems are useful for developers, AI agents, competitive verification, map design, player learning, and dispute investigation.

Where appropriate, development diagnostics should become polished player-facing analysis features.

---

## 18. Development model

The project assumes a relatively small core team making extensive use of AI.

AI agents working on Straf3 should treat this vision as a statement of product intent rather than merely a list of features.

When implementation choices conflict, agents should reason from the intended player experience.

Development will naturally involve parallel work across areas such as:

1. movement and deterministic simulation;
2. engine and rendering;
3. audio and presentation;
4. maps and training;
5. multiplayer and networking;
6. records, replays, and verification;
7. web portal and services;
8. editor and map-production systems;
9. AI tooling;
10. reinforcement learning;
11. testing, profiling, diagnostics, and release infrastructure.

These systems should be integrated continuously.

The engine should not be developed for years before proving the game.

The web platform should not be built in isolation from real maps, records, and servers.

RL research should not drift away from the actual movement system.

The editor should be tested by actually building Straf3 maps.

Each major technical system should repeatedly reconnect to the playable game.

---

## 19. Development priorities

When priorities conflict, use approximately this order:

1. **Movement quality**
2. **Responsiveness and feel**
3. **Depth, mastery, and movement coherence**
4. **Competitive integrity and determinism**
5. **Map quality**
6. **Multiplayer quality**
7. **Records, replays, ghosts, and competitive infrastructure**
8. **Native production quality**
9. **Browser accessibility and integration**
10. **Web ecosystem**
11. **AI-assisted development**
12. **RL-assisted development**
13. **Architectural reuse**
14. **Feature breadth**

This order does not mean lower items are unimportant.

It means they should support the items above them.

A sophisticated AI map pipeline is not useful if the generated maps are poor.

A perfect web platform is not useful if movement is uninteresting.

A beautiful renderer is not useful if input latency damages the game.

A reusable engine abstraction is not valuable if building it delays proving Straf3.

---

## 20. Core development proofs

Several concrete proofs should guide development.

### Proof 1 — Movement is compelling

A small number of maps and mechanics are enough for skilled players to continue replaying them voluntarily because improving feels rewarding.

### Proof 2 — Movement is understandable

Players can learn primitives, combine them, understand failures, and deliberately improve.

### Proof 3 — Movement survives the technology

The game retains excellent responsiveness under real rendering workloads, high-refresh displays, multiplayer conditions, and browser constraints.

### Proof 4 — Ranked competition is trustworthy

Runs can be recorded, replayed, verified, compared, and preserved.

### Proof 5 — Live racing works

Players can compete directly without networking destroying movement quality or fairness.

### Proof 6 — Training transfers to real maps

Skills learned in training maps become useful tools that players creatively apply elsewhere.

### Proof 7 — RL learns the same movement language

Agents successfully progress from primitives to combinations to routes using the same fundamental environments as human training.

### Proof 8 — RL provides useful map information

Agents find route, difficulty, exploit, flow, or skill-level information that materially helps human map development.

### Proof 9 — AI materially improves map production

Human-directed agents can operate the map-development toolchain and produce useful, inspectable improvements.

### Proof 10 — Web and game feel like one ecosystem

A link to a map, replay, record, profile, or server naturally turns into the corresponding action.

---

## 21. Confirmed anti-goals

Straf3 should not become:

* a generic ability shooter with movement mechanics attached;
* a simplified Defrag clone;
* a museum preserving Quake mechanics exactly as they were;
* a game where automation replaces execution;
* a game where excessive complexity substitutes for depth;
* a game where arbitrary speed caps solve map-design problems;
* a cooldown-rotation game;
* a game whose rendering ambition damages movement readability;
* a game whose networking requirements weaken local movement;
* a web platform with a mediocre game attached;
* an engine project that happens to contain Straf3;
* an AI research project that happens to contain a game;
* a content-generation machine optimized for map quantity rather than map quality;
* a project that prioritizes community-created map infrastructure before establishing an excellent first-party game.

There should be no pay-to-win system or sale of competitive movement advantages.

---

## 22. Areas intentionally left open

Several important questions should remain open until experimentation produces evidence.

These include:

* the final canonical movement mechanic set;
* exact crouch-slide behavior;
* experimental movement profiles;
* the criteria through which a new mechanic becomes canonical;
* simulation frequency;
* detailed multiplayer networking architecture;
* browser performance limits;
* exact native/browser graphical differences if any become necessary;
* the precise AI-editor architecture;
* MCP versus other agent interfaces;
* how agents consume screenshots and rendered views;
* the exact RL algorithms and training infrastructure;
* representation of player skill for RL agents;
* eventual community map support;
* eventual modding scope;
* business model;
* server-hosting model;
* source-code and protocol openness;
* exact launch content;
* release schedule;
* staffing.

These questions should remain visible without forcing premature answers.

Architecture should allow sensible experimentation where doing so is cheap.

Speculative flexibility should not damage the clarity or quality of the current game.

---

## 23. Guidance for AI agents working on Straf3

AI agents should not interpret this document as permission to maximize every stated ambition simultaneously.

When making implementation decisions:

1. Determine how the work affects the actual Straf3 player experience.
2. Protect movement responsiveness and correctness first.
3. Prefer simple systems that allow rapid experimentation.
4. Measure performance-sensitive assumptions.
5. Preserve deterministic competitive behavior where required.
6. Avoid introducing generic abstractions before Straf3 demonstrates a need for them.
7. Keep experimental movement mechanics isolated from the canonical ruleset until validated.
8. Treat maps as essential game design, not test geometry.
9. Treat browser support as an ambitious engineering goal rather than justification for weakening native play.
10. Treat AI and RL as tools for making Straf3 better, not goals independent of the game.
11. Do not assume community-created maps are an immediate requirement.
12. When uncertain about a product decision, optimize for the movement experience described in this document rather than inventing a new product direction.

---

## 24. One-paragraph vision

**Straf3 is a next-generation competitive first-person movement game inspired by Quake 3 Defrag but built to establish its own modern identity. Movement is both an art and a science: easy to begin learning, extraordinarily difficult to master, mechanically understandable yet expressive enough for flow, creativity, route discovery, precision, and competition. Players train, pursue records, race each other live, analyze replays, and move through stylized minimal science-fiction maps built to AAA standards of quality and responsiveness. A custom modular Rust engine powers an uncompromised native experience while making full browser play a major goal, connected through a web portal that acts as the launcher and hub for maps, records, replays, profiles, leaderboards, and servers. The same movement primitives used to teach human players also train RL agents, eventually allowing agents with different skill levels to analyze, test, and help create maps. AI agents should become first-class participants in the map-development workflow while humans remain able to direct, inspect, and judge the work. Every technical system exists ultimately to serve the movement, the maps, and the quality of the game.**
