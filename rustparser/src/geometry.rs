use std::rc::Rc;

/// (lon, lat), matching the (x, y) order JTS uses for `Coordinate(lon, lat)`
/// in the original parser.
pub type Coord = (f64, f64);

/// A ring/line's coordinates, shared via `Rc` instead of owned outright.
///
/// A single way's geometry can end up stored in up to three places at
/// once (`way_map`, and possibly a `Road` and/or `Amenity` built from
/// that same way). Storing coordinates as `Vec<Coord>` meant every one
/// of those was a full deep copy of the point list. `Rc<[Coord]>` makes
/// `Geometry::clone()` an O(1) refcount bump - the actual coordinate
/// data is allocated once, in `process_way`.
pub type Ring = Rc<[Coord]>;

#[derive(Clone)]
pub enum Geometry {
    Point(Coord),
    LineString(Ring),
    Polygon {
        outer: Ring,
        holes: Vec<Ring>,
    },
    /// list of (outer_ring, holes) pairs
    MultiPolygon(Vec<(Ring, Vec<Ring>)>),
    Collection(Vec<Geometry>),
}

impl Geometry {
    /// Mirrors JTS's Geometry.isEmpty(): a collection is empty if it has
    /// no elements, or every element in it is itself empty.
    pub fn is_empty(&self) -> bool {
        match self {
            Geometry::Point(_) => false,
            Geometry::LineString(coords) => coords.is_empty(),
            Geometry::Polygon { outer, .. } => outer.is_empty(),
            Geometry::MultiPolygon(polys) => polys.is_empty(),
            Geometry::Collection(parts) => parts.is_empty() || parts.iter().all(|g| g.is_empty()),
        }
    }
}

#[inline]
fn coord_eq(a: &Coord, b: &Coord) -> bool {
    // Coordinates here always originate from the same shared node, so
    // exact float comparison is safe (no accumulated floating point drift).
    a.0.to_bits() == b.0.to_bits() && a.1.to_bits() == b.1.to_bits()
}

/// Approximate equivalent of JTS's `LineMerger`: stitches line segments
/// that share an endpoint into longer chains, in any order/direction.
///
/// This is O(n^2) in the number of input lines, same complexity class as
/// what LineMerger effectively does for small-to-medium relations. For
/// relations with thousands of member ways this would need a proper
/// endpoint index; real-world OSM multipolygons rarely have that many
/// members per relation.
pub fn merge_lines(lines: Vec<Vec<Coord>>) -> Vec<Vec<Coord>> {
    let n = lines.len();
    let mut used = vec![false; n];
    let mut result = Vec::new();

    for start in 0..n {
        if used[start] || lines[start].len() < 2 {
            continue;
        }
        used[start] = true;
        let mut chain = lines[start].clone();

        loop {
            let mut extended = false;

            for i in 0..n {
                if used[i] || lines[i].len() < 2 {
                    continue;
                }
                let line = &lines[i];
                let c_first = chain[0];
                let c_last = chain[chain.len() - 1];
                let l_first = line[0];
                let l_last = line[line.len() - 1];

                if coord_eq(&c_last, &l_first) {
                    chain.extend(line[1..].iter().copied());
                    used[i] = true;
                    extended = true;
                    break;
                } else if coord_eq(&c_last, &l_last) {
                    chain.extend(line[..line.len() - 1].iter().rev().copied());
                    used[i] = true;
                    extended = true;
                    break;
                } else if coord_eq(&c_first, &l_last) {
                    let mut new_chain = line[..line.len() - 1].to_vec();
                    new_chain.extend(chain);
                    chain = new_chain;
                    used[i] = true;
                    extended = true;
                    break;
                } else if coord_eq(&c_first, &l_first) {
                    let mut new_chain: Vec<Coord> = line[1..].iter().rev().copied().collect();
                    new_chain.extend(chain);
                    chain = new_chain;
                    used[i] = true;
                    extended = true;
                    break;
                }
            }

            if !extended {
                break;
            }
        }

        result.push(chain);
    }

    result
}

/// Merges lines, then keeps only chains that closed up into a ring
/// (first == last coordinate) with at least 4 points - same threshold
/// the original Java code uses.
pub fn extract_closed_rings(lines: Vec<Vec<Coord>>) -> Vec<Vec<Coord>> {
    merge_lines(lines)
        .into_iter()
        .filter(|c| c.len() >= 4 && coord_eq(&c[0], &c[c.len() - 1]))
        .collect()
}

/// Ray-casting point-in-polygon test. Used as a cheap stand-in for JTS's
/// exact `Polygon.covers()` when deciding which outer ring a hole belongs
/// to. Good enough to assign holes correctly in the vast majority of
/// real-world cases, but it is not an equivalent of JTS's
/// robust geometric predicate - edge cases (a hole touching the outer
/// boundary exactly) can differ. It's the one place
/// where Rust output might diverge slightly from the Java version's counts.
pub fn point_in_ring(pt: Coord, ring: &[Coord]) -> bool {
    let (x, y) = pt;
    let mut inside = false;
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = ring[i];
        let (xj, yj) = ring[j];
        if (yi > y) != (yj > y) {
            let x_intersect = (xj - xi) * (y - yi) / (yj - yi) + xi;
            if x < x_intersect {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}
