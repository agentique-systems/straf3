# The straf3 URL scheme

**Status: fixed.** This document is a contract between three seats. The browser
client must honour the launch and replay routes, the records service must be
able to name a run the way a link names it, and the site must not change these
shapes once anything has been sent to anyone. Changing a URL after it has been
pasted somewhere costs three times: once in the site, once in the client, once
in every link already in the wild.

Governing requirement: **acceptance criterion 15 — "a map URL launches that map
in the browser client; a record URL plays that record back."** Vision goal 9 is
that a shared link turns intent into action. The two routes that satisfy it are
[`/play/<map>`](#play) and [`/watch/<run>`](#watch), and they were designed
first; everything else on this page is the structure that grew around them.

---

## 1. The whole scheme

```
/                          home — the map index
/m/<map>                   a map's record book, default category
/m/<map>/<category>        one category's board
/r/<run>                   a record: the run as evidence
/play/<map>                LAUNCH that map in the browser client      ← criterion 15
/watch/<run>               PLAY that record back in the browser client ← criterion 15
```

**Every route's first path segment is a fixed keyword** — `m`, `r`, `play`,
`watch`. A map slug or a run id never appears in first position. That is
deliberate: it means no map can ever be named such that its page shadows an
action route, and a router can dispatch on segment 0 with no lookahead and no
ambiguity. It is also why the action routes are verb-first rather than
`/m/coil/play`: `play` sitting where a category goes would be a reserved word
inside an open namespace, and reserved words inside open namespaces are how
URL schemes rot.

Resources are nouns (`/m/…`, `/r/…`). Actions are verbs (`/play/…`,
`/watch/…`). A link you send someone to *look at something* and a link you send
them to *do something* are different links, and the difference is visible in the
URL before they click it.

---

## 2. Grammar

```
<map>       := [a-z0-9][a-z0-9-]{0,63}          a `maps.slug`
<run>       := <digest16> | <uuid>              see §5
<digest16>  := [0-9a-f]{16}                     a u64, lowercase hex, zero-padded
<uuid>      := 8-4-4-4-12 lowercase hex with dashes
<category>  := <family> [ "@" <digest16> ]
<family>    := [a-z0-9]{1,16}                   a `physics_profiles.kind`: `vq3`, `cpm`
```

`@` is a legal path character (RFC 3986 `pchar` includes `@`), needs no
escaping, and no map slug or profile family can contain one.

All identifiers in a URL are **lowercase**. A uppercase digest is a 404, not a
redirect — two spellings of one record is how a cache ends up holding two
copies of the same page and a "share" button ends up producing a link that does
not match the one in the address bar.

---

## 3. The category key, and why it is in the path

`docs/web/ARCHITECTURE.md` §5.2: a leaderboard category is **(map, physics
profile)**. §5.4 is the part most leaderboard schemas get wrong — when the
physics constants change, every stored time was produced by a game that no
longer exists. The scheme therefore distinguishes two different things a person
can mean by "the CPM board for coil":

| URL | Means | Stability |
|---|---|---|
| `/m/coil/cpm` | **the current** cpm board | *Moves.* Tuning cpm changes what this page shows. |
| `/m/coil/cpm@a1b2c3d4e5f60718` | the board under **exactly those constants** | *Frozen forever.* |

The digest is `PhysicsId::digest` — the FNV-1a fold over the bits of every field
of the `PhysicsProfile` that was in effect
(`crates/straf3-replay/src/identity.rs`). It is derived, never declared, so a
pinned URL cannot claim physics the simulation will not honour. It is the same
u64 that a `.s3d` header carries and that `physics_profiles.digest` stores.

Consequences that are load-bearing:

- **Old record books stay browsable and stay meaningful.** A pinned board is
  not "an archive view"; it is the same page with a fixed category key, and it
  keeps working when the family label is retired.
- **Every link the site generates from a record is pinned.** A record is bound
  to a physics digest, so a link *out of* a record page to its board names that
  digest. Only navigation that means "current" emits the unpinned form.
- **A pinned board whose digest is unknown to the service renders as unknown**,
  not as empty and not as the current board. Silently substituting the current
  profile is the failure §7.2 step 2 forbids the verifier from making, and the
  site does not get to make it either.

`/m/<map>` with no category resolves to the map's default category and
**canonicalises** — the address bar ends up on the explicit form. A bare map URL
is a convenience for typing, never a stored link.

Tick rate is displayed but is not part of the key, per §5.2 and subject to its
caveat. It is not in the URL.

---

## 4. The two routes criterion 15 is about

### <a id="play"></a>`/play/<map>` — a map URL launches that map

```
/play/coil
/play/coil?p=cpm
/play/coil?p=cpm@a1b2c3d4e5f60718
/play/coil?p=cpm&ghost=0123456789abcdef
```

| Param | Meaning | Default |
|---|---|---|
| `p` | the category to play under: `<family>` or `<family>@<digest16>` | the map's default category |
| `ghost` | a `<run>` to race against | none |

Behaviour the client and the site agree on:

1. The page loads the browser client, hands it the map and the physics the URL
   names, and puts the player in it. No menu step between the link and the
   game — that is the whole point of the criterion.
2. **Pointer lock is not taken on load.** A page that grabs the mouse the
   instant it opens is hostile, and a browser will refuse the request anyway
   without a user gesture. The canvas is armed and a single click enters.
3. **If `p` pins a digest the loaded build does not implement, the client
   refuses and says so.** It does not run the nearest thing. This is §7.4's
   discipline applied to the client: a run produced under physics the URL did
   not name is not the run the link promised.
4. `ghost` failing to resolve degrades to playing without a ghost, with the
   failure stated on screen. A missing ghost is not a reason to refuse the map.

### <a id="watch"></a>`/watch/<run>` — a record URL plays that record back

```
/watch/0123456789abcdef
/watch/0123456789abcdef?t=12500
```

| Param | Meaning | Default |
|---|---|---|
| `t` | seek to this many **milliseconds** of simulation time | 0 |

Behaviour:

1. The page resolves the run, fetches its `.s3d`, loads the client, and plays
   the recording back — the *recording*, re-simulated, not a stored path.
2. The map and physics come **from the recording's own header**, never from the
   URL and never from the current defaults. A `.s3d` carries a `WorldId` and a
   `PhysicsId` precisely so this cannot go wrong
   (`crates/straf3-replay/src/identity.rs`).
3. If the loaded build cannot honour that world or physics identity, playback
   is refused with the mismatch named. A ghost replayed against geometry that
   moved shows a run that never happened.
4. `t` is milliseconds, like every other duration in this platform (§5.1). It
   is a seek hint; a client that cannot seek starts at zero and says so.

---

## 5. How a run is named in a URL

`<run>` accepts two spellings, distinguishable by shape and never by lookup:

- **`<digest16>` — the run digest.** 16 lowercase hex characters. This is the
  *identity of the run*: the rolling digest folded over every command's state
  checksum, carried in the `.s3d` header, and the column §5.1 puts a **global**
  unique index on. It is computable from the file alone, with no service and no
  database, which is what makes it the durable name.
- **`<uuid>` — the `runs.id`.** Accepted, because §7.5's API is written in terms
  of it and a service-generated link will have it to hand.

**The site canonicalises to the digest.** A UUID URL resolves and then replaces
the address with the digest form. Rationale: a digest link survives a database
restore, a re-import, and a service that has not been written yet — a UUID link
survives none of those, and this session has a real `.s3d` on disk and no
running service, which is exactly the case the digest form handles and the UUID
form cannot.

### What this asks of the records service

One lookup, and it is nearly free because the index already exists:

```
GET /v1/runs/by-digest/:digest16     → the same body as GET /v1/runs/:id
```

`runs.run_digest` already carries a global unique index (§5.1). If this endpoint
does not exist, the site degrades to UUID-only links against the service and
keeps digest links working for locally-loaded files; nothing breaks, but the
durable name stops being durable across a redeploy, which is the property
criterion 15 is asking for.

---

## 6. Where the site ends and the API begins

The site's routes above are **pages**. The records service's routes are
**data**, they live under `/v1/…` (§7.5), and the two namespaces do not
overlap — `/v1` is not a valid site route and never becomes one. A deployment
serves both from one origin; the dev server proxies `/v1` to the service, so a
page never learns whether the service is same-process or elsewhere.

Reserved first segments, permanently, so that a future map slug or run id can
never take one: `v1`, `m`, `r`, `play`, `watch`, `assets`, `app`, `client`,
`dev`, `health`.

---

## 7. Serving requirement: these are real paths

Every route above must serve the shell **on a cold load**, because that is what
"durable link" means — pasted into a fresh tab, opened from a chat client,
followed from a bookmark six months later. Not just reachable by clicking
around inside an already-loaded page.

So the rule for any server, dev or production, is:

> A request for a path with no file extension that does not match a real file
> serves `/index.html` with status **200**, and the client router reads
> `location.pathname`. A request for a missing *file* (something with an
> extension) is a **404** and stays one.

The 200 matters. A fallback served as 404 renders correctly and tells every
crawler, cache and link checker that the page does not exist.

The extension rule matters too: without it, a missing `.wasm` returns HTML with
a 200, and the failure surfaces as a wasm magic-word error three layers away
from the missing file.

`web/dev/serve.mjs` implements exactly this. The production equivalent is
nginx `try_files $uri /index.html;` or the hosting platform's SPA-fallback
setting, with the extension carve-out.

**No hash routing.** `#/r/…` would make the fallback problem disappear, and it
would also make every one of these links invisible to the server, uncacheable
per-page, and unindexable. The fallback rule is four lines of server config; it
is the cheaper of the two costs.

---

## 8. Examples, end to end

```
/                                          the map index
/m/coil                                    coil's book → canonicalises to its default category
/m/coil/cpm                                coil, current CPM
/m/coil/cpm@a1b2c3d4e5f60718               coil, CPM as it was — frozen, forever
/m/coil/vq3                                coil, current VQ3 — a different game, a different board
/r/0123456789abcdef                        one record, as evidence
/watch/0123456789abcdef                    ...played back                        [criterion 15]
/watch/0123456789abcdef?t=9000             ...from nine seconds in
/play/coil                                 play coil                             [criterion 15]
/play/coil?p=cpm@a1b2c3d4e5f60718          play coil under exactly that physics
/play/coil?p=cpm&ghost=0123456789abcdef    play coil racing that record's ghost
```

The last line is the shape the whole surface exists for: a record page hands
you a link that puts you in the map, under the physics that record was set
under, with that record running beside you.
