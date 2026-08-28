# Site evidence — what these screenshots are, and what they are not

Captured from a real Chrome against a real `web/dev/serve.mjs` on
`http://127.0.0.1:8787`, each on a **cold load** — the URL typed into the
address bar, not clicked to from inside an already-loaded page. That
distinction is the whole of URLS.md §7 and it is why they were taken this way.

**Every `/v1` value visible in these images is fabricated.** They were produced
against `node web/dev/serve.mjs --fixtures`, whose canned answers live in
`web/dev/fixtures.mjs` and which prints a banner saying it is not the records
service. No time here was set by anyone, and no digest here was derived from a
`PhysicsProfile`. The fixtures exist because three of the four states below
cannot be produced from a correctly-working seeded database: a working database
contains no failures, and a fresh one contains no times.

The pages rendering them are the real pages. Nothing here is a mockup or a
gallery of example components — each image is `web/site/app/pages/map.js` and
`web/site/app/board.js` doing their ordinary job on a response.

## r9 — the four kinds of "no rows"

Requirement r9: the site must distinguish "no rows because nobody has set a
time" from "no rows because the service could not answer". An empty `<tbody>`
renders identically for both and they are completely different facts.

| file | URL | what the service said | how it renders |
|---|---|---|---|
| `r9-1-populated.png` | `/m/coil/cpm` | 200, four entries | the board. Note ranks 3 and 3 — the service ties them and the site does not renumber. |
| `r9-2-empty.png` | `/m/coil/vq3` | 200, `{"entries": [], "total": 0}` | **"Nobody has set a time here yet."** Neutral. Nothing is wrong; the board is new. |
| `r9-3-unanswerable.png` | `/m/void/cpm` | 503 `database_unavailable` | **"The records service could not answer."** Amber, carrying the detail, and saying explicitly that this is not an empty board. |
| `r9-4-unknown-pin.png` | `/m/coil/cpm@ffffffffffffffff` | 404 `unknown_physics_digest` | **"Unknown physics."** Its own third treatment. No rows, and the current board is offered as an explicitly *different* question rather than shown in its place. |
| `r9-5-no-service.png` | `/m/coil/cpm`, server started `--no-api` | 503 `no_records_service` | the same amber "could not answer", with the reason that there is no service configured at all. |

The fourth is the one URLS.md §3 is strict about: a pinned board whose digest
the service does not know renders as **unknown** — "not as empty and never as
the current board".

Finding it took two bugs out of the code, both of which had produced a
*plausible* page:

1. `api.js` sent the category as `profile=cpm` plus a separate
   `profile_digest=…`, and the service reads one `profile=cpm@<digest16>`. The
   pinned request therefore arrived as a bare family and got answered with the
   **current** board — the substitution ARCHITECTURE §7.2 step 2 forbids,
   delivered by a query parameter nobody was reading.
2. The mismatch check caught it, printed a warning, and rendered the rows
   underneath anyway. A pinned URL showing the current board with a caption is
   still showing the current board.

## Routing

| file | shows |
|---|---|
| `home.png` | `/` with the map index. |
| `home-no-service.png` | `/` with the service unreachable: the list is stated as unknown, and the page still gets you into the game, because playing needs the map and the physics and not the service. |
| `record.png` | `/r/<digest16>` — reached by cold-loading the **UUID** form, which resolved and then replaced the address with the digest (URLS.md §5). Ranked time labelled server-computed; `client_time_ms` labelled as the client's claim and never as the time. |
| `record-local-file.png` | the same page for a run only a local `.s3d` has. Amber "from a local file — not a verified record". The content digest and the 600-checksum trace fold were both recomputed in the browser, so those two are facts this page established rather than claims it was handed — and it still has no verdict, because nothing verified it. |
| `uppercase-not-found.png` | `/r/0123456789ABCDEF`. Not found, **not redirected**: the lowercase form is offered as a link the reader may follow. Two spellings of one record is how a cache ends up with two copies of a page. |

## The stage pages

These were driven with `--client-dir web/dev/client-stub` and `?backend=stub`.
**The stub is not the browser client.** It has no physics, no renderer and no
recorder; it draws a canvas that says so. It exists to exercise the site half of
the JS↔wasm contract (§B) before the wasm half exists.

| file | shows |
|---|---|
| `play-refused.png` | a pinned digest the build cannot honour. Refused, both digests named, and explicitly not running the nearest thing (r3). |
| `play-ghost-degraded.png` | a ghost that will not resolve. The map plays anyway (URLS.md §4 behaviour 4) — a missing ghost is not a reason to refuse a map, and it is a different thing from a physics mismatch. |
| `play-run-finished.png` | a finished run. The run digest is on screen, in `data-run-digest`, on `globalThis.straf3.lastRun`, and on a console line — four channels, none of them the `.s3d` header, which is the point (r6). |
| `play-no-client.png` | no client built, or no WebGPU adapter. The routing half of the page is still correct and is shown: which map, which source URL, which physics, pinned or not. |
| `watch.png` | `/watch/<digest16>?t=9000`. The config handed to the client carries `recording_url` and `seek_ms` and **no map and no physics** — those come from the recording's own header, never from the URL (URLS.md §4 behaviour 2). The bar shows the header as decoded by the site's own `s3d.js`. |

## What these do not show

- **No run was re-simulated.** The stub simulates nothing, and the fixture
  `.s3d` served at `/v1/runs/:id/demo` is structurally valid and deliberately
  empty — zero commands. `/watch/<run>` genuinely re-simulating a recording
  needs the real client, and that is `client`'s w6–w9 and `loop`'s w15.
- **No timing number appears here and none may be taken from here.** This is a
  WSL2 host with a software-only GPU.
- **The site has not been run against the real records service.** Every
  response above came from `--fixtures` or from a proxy failure. Pointing
  `--api` at `straf3-records-api` on 8788 is `loop`'s w13.
