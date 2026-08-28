# OSM Parser: Java vs Rust speed comparison

A small experiment comparing the parsing speed of an OpenStreetMap
(`.osm` XML) parser written in Java against a Rust port of the same
logic, run on the same input file.

## Where this came from

The original parser (`OsmParser.java`) is part of an existing personal
backend project: a Spring Boot REST API/middleware that serves data
derived from OpenStreetMap extracts — location search, radius-from-point
and bounding-box queries, reviews, ratings, and related lookups. Parsing
the raw `.osm` XML into usable geometry is a one-time (or periodic)
ingestion step that feeds that API, not something done on every request,
but it runs against fairly large regional extracts, so its speed still
matters for how often you can afford to refresh the data.

`OsmParser` reads OSM XML using Java's StAX (`javax.xml.stream`) and
turns it into structured geometry (amenities, roads, building outlines)
using the [JTS Topology Suite](https://locationtech.github.io/jts/) for
the actual geometric work — merging way segments into polygon rings,
assembling multipolygon relations with holes, etc. That output is what
the REST layer subsequently indexes and queries against.

To compare it fairly against a Rust rewrite, two standalone projects
were built, independent of the original backend project:

- **`osm-benchmark-parser`** (Java, Maven) — the original parsing logic,
  pulled out of the backend project (Lombok and project-specific types
  removed), split into four separately-timed phases, packaged as a
  runnable CLI jar.
- **`osm-parser-rust`** (Rust, Cargo) — same four phases, same overall
  logic, reimplemented in Rust with `quick-xml` for XML reading and
  rewritten geometry code in place of JTS.

## The four phases

Both versions split parsing into the same stages so the comparison
isn't just one opaque number:

1. **read-xml** — pure XML tokenizing into plain in-memory structs. No
   geometry touched. This is the most apples-to-apples comparison of
   raw XML parsing speed between the two languages.
2. **build-nodes** — turns raw lat/lon into point geometry, collects
   node-based amenities (cafes, shops, etc. tagged directly on a point).
3. **build-ways** — turns a way's list of node references into a
   LineString or Polygon, collects way-based amenities and roads.
4. **build-relations** — assembles `multipolygon`/`building`/generic
   relations out of their member ways (e.g. a lake with an island hole
   in it). Normally the most geometrically complex phase.

## What was changed along the way

- **Java**: removed the original's silent `catch (Exception e) {
  e.getMessage(); }`, which could hide a parse failure and produce a
  falsely "fast" partial result. `parse()` now throws instead.
- **Rust**: JTS's `LineMerger` (stitches way segments into closed
  rings) and `Polygon.covers()` (decides which ring a hole belongs to)
  don't have off-the-shelf Rust equivalents, so both were rewritten:
  a simple endpoint-stitching merge, and a ray-casting point-in-polygon
  test instead of JTS's exact geometric predicate.
- **Rust optimization pass**: the first version of `build-ways` cloned
  a way's coordinate list up to three times (once each for the internal
  way lookup table, a `Road`, and an `Amenity`). Coordinates were
  switched to `Rc<[Coord]>` so those are now cheap reference-count
  bumps instead of full copies, to keep the comparison fairer.

## Important clarification: this is not an apples-to-apples algorithm

The Rust version's `multipolygon` handling is an *approximation* of
what JTS does, not a byte-for-byte port:

- Ring merging: a straightforward O(n²) endpoint-stitching pass instead
  of JTS's indexed `LineMerger`.
- Hole assignment: a single-point ray-casting test instead of JTS's
  exact geometric `covers()` predicate.

On the test file used here, both produced **identical output counts**
(relations, amenities, roads, standalone node/way counts all matched),
which is reassuring, but the algorithms are not guaranteed to agree on
every possible OSM file, particularly relations with holes that touch
the outer boundary. Because of this, `build-relations` timings between
the two versions reflect differences in *how much work each is actually
doing*, not a pure "same algorithm, different language" comparison —
treat that phase's speedup with more skepticism than the others.

## Methodology

Both binaries use the same manual benchmark shape: a few warmup runs
(JIT warmup on the Java side) followed by several measured runs, each
starting from a fresh parser instance so no state leaks between runs.
No `criterion`/`JMH` statistical rigor was layered on top — these are
plain wall-clock numbers from `System.nanoTime()` / `Instant::now()`,
averaged with min/max shown. Good enough to see the shape of the
result, not a substitute for a proper statistical benchmark if this
number needs to go in front of someone who'll push back on it.

```bash
# Java
java -jar target/osm-parser.jar styria.osm 10 3

# Rust
./target/release/osm-parser-rust styria.osm 10 3
```

## Results

Test file: `styria.osm` — 596,370 nodes, 67,316 ways, 892
relations. 3 warmup + 10 measured runs on both sides, same machine.

**Output counts (identical on both sides):**

| Metric | Count |
|---|---|
| Amenities | 17,324 |
| Roads | 46,726 |
| Relations | 892 |
| Standalone nodes | 15,110 |
| Standalone ways | 63,225 |

**Per-phase timing, averaged over 10 runs (ms):**

| Phase | Java avg (min–max) | Rust avg (min–max) | Speedup |
|---|---|---|---|
| 1. read-xml | 1763.97 (1591–2139) | 1055.50 (1040–1084) | ~1.67× |
| 2. build-nodes | 147.17 (83–300) | 86.49 (79–111) | ~1.70× |
| 3. build-ways | 246.90 (198–367) | 274.69 (260–293) | Rust ~1.11× *slower* |
| 4. build-relations | 547.47 (379–705) | 5.63 (4–7) | ~97× (see clarification above) |
| **TOTAL** | **2705.59 (2494–3249)** | **1482.56 (1446–1525)** | **~1.82×** |

## Reading these numbers

- **read-xml and build-nodes** are the cleanest comparison of "the
  language itself": same logic, similar allocation patterns. Rust wins
  by roughly the same ~1.7× margin on both, plausibly from avoiding JVM
  GC pauses and heap-boxed `Point`/`Coordinate` objects.
- **build-ways being slightly slower in Rust** was investigated: it's
  not really about the coordinate cloning (fixed via `Rc`, only ~2.5%
  gain) — most of the phase's cost is `HashMap<i64, Coord>` lookups per
  node reference, on both sides. This phase currently doesn't show a
  clear language advantage either way.
- **build-relations' ~97× gap is not a fair language comparison** —
  it's comparing a full JTS geometry pipeline against a much simpler
  approximation, as noted in the warning above. Don't count
  this number as "Rust is 97x faster at relation processing" without
  that context.
- **The Rust run's min/max spread is much tighter** (79ms across all
  runs, 1446–1525) than Java's (755ms, 2494–3249) — likely reflecting
  JIT warmup variance and GC pauses on the Java side even after 3
  warmup iterations. That predictability is arguably as interesting a
  result as the raw average.
- **Bottom line**: excluding the relations phase (which isn't really a fair
  comparison), Rust comes out roughly 1.5–1.7× faster on this workload and
  this file. That's the number worth quoting, with the phase-level
  breakdown available as backup if anyone asks "how did you measure
  this."
