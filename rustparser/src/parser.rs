use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::time::{Duration, Instant};

use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

use std::rc::Rc;

use crate::geometry::{extract_closed_rings, point_in_ring, Coord, Geometry, Ring};
use crate::raw_model::{RawMember, RawModel, RawNode, RawRelation, RawWay};

pub struct Amenity {
    pub id: i64,
    pub name: String,
    pub geometry: Geometry,
    pub tags: HashMap<String, String>,
}

pub struct Road {
    pub id: i64,
    pub name: String,
    pub geometry: Geometry,
    pub tags: HashMap<String, String>,
    pub child_node_ids: Vec<i64>,
}

#[derive(Default)]
pub struct OsmParser {
    pub amenities: Vec<Amenity>,
    pub roads: Vec<Road>,
    pub relation_count: u64,
    pub ref_nodes: HashSet<i64>,
    pub ref_ways: HashSet<i64>,
    pub node_map: HashMap<i64, Coord>,
    pub way_map: HashMap<i64, Geometry>,
    pub phase_timings: Vec<(String, Duration)>,
}

impl OsmParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs all four phases in order, timing each one.
    pub fn parse(&mut self, file_path: &str) -> Result<(), Box<dyn Error>> {
        let start = Instant::now();
        let raw = read_xml(file_path)?;
        self.phase_timings.push(("1-read-xml".to_string(), start.elapsed()));

        let start = Instant::now();
        self.build_nodes(&raw);
        self.phase_timings.push(("2-build-nodes".to_string(), start.elapsed()));

        let start = Instant::now();
        self.build_ways(&raw);
        self.phase_timings.push(("3-build-ways".to_string(), start.elapsed()));

        let start = Instant::now();
        self.build_relations(&raw);
        self.phase_timings.push(("4-build-relations".to_string(), start.elapsed()));

        Ok(())
    }

    // Phase 2: node geometry

    fn build_nodes(&mut self, raw: &RawModel) {
        for n in &raw.nodes {
            let coord: Coord = (n.lon, n.lat);
            self.node_map.insert(n.id, coord);

            if n.tags.contains_key("amenity") {
                self.amenities.push(Amenity {
                    id: n.id,
                    name: n.tags.get("name").cloned().unwrap_or_default(),
                    geometry: Geometry::Point(coord),
                    tags: n.tags.clone(),
                });
            }
        }
    }

    // Phase 3: way geometry

    fn build_ways(&mut self, raw: &RawModel) {
        for way in &raw.ways {
            for r in &way.node_refs {
                self.ref_nodes.insert(*r);
            }
            self.process_way(way);
        }
    }

    fn process_way(&mut self, way: &RawWay) {
        let coords: Vec<Coord> = way
            .node_refs
            .iter()
            .filter_map(|id| self.node_map.get(id).copied())
            .collect();

        if coords.is_empty() {
            return;
        }

        let coords_len = coords.len();
        let closed = coords_len > 1 && {
            let first = coords[0];
            let last = coords[coords_len - 1];
            first.0.to_bits() == last.0.to_bits() && first.1.to_bits() == last.1.to_bits()
        };
        let single_point = if coords_len == 1 { Some(coords[0]) } else { None };

        // Coordinates are allocated exactly once here. Everything below
        // (way_map, Road, Amenity) shares this same buffer via cheap Rc
        // clones instead of deep-copying the point list up to three times.
        let ring: Ring = Rc::from(coords);

        let geom = if let Some(pt) = single_point {
            Geometry::Point(pt)
        } else if closed && coords_len > 3 {
            Geometry::Polygon { outer: ring.clone(), holes: Vec::new() }
        } else {
            Geometry::LineString(ring.clone())
        };

        self.way_map.insert(way.id, geom.clone());

        if way.tags.contains_key("highway") && coords_len >= 2 {
            self.roads.push(Road {
                id: way.id,
                name: way.tags.get("name").cloned().unwrap_or_default(),
                geometry: geom.clone(),
                tags: way.tags.clone(),
                child_node_ids: way.node_refs.clone(),
            });
        }
        if way.tags.contains_key("amenity") {
            self.amenities.push(Amenity {
                id: way.id,
                name: way.tags.get("name").cloned().unwrap_or_default(),
                geometry: geom,
                tags: way.tags.clone(),
            });
        }
    }

    // Phase 4: relations (usually the expensive part)

    fn build_relations(&mut self, raw: &RawModel) {
        for rel in &raw.relations {
            self.relation_count += 1;
            for mem in &rel.members {
                if mem.member_type == "way" {
                    self.ref_ways.insert(mem.member_ref);
                } else if mem.member_type == "node" {
                    self.ref_nodes.insert(mem.member_ref);
                }
            }
            self.process_relation(rel);
        }
    }

    fn process_relation(&mut self, rel: &RawRelation) {
        let rel_type = rel.tags.get("type").map(|s| s.as_str()).unwrap_or("");

        let final_geom = match rel_type {
            "multipolygon" => make_multipolygon(rel, &self.way_map),
            "building" => make_building(rel, &self.way_map),
            _ => make_normal_relation(rel, &self.way_map),
        };

        if let Some(geom) = final_geom {
            if !geom.is_empty() {
                if rel.tags.contains_key("amenity") {
                    self.amenities.push(Amenity {
                        id: rel.id,
                        name: rel.tags.get("name").cloned().unwrap_or_default(),
                        geometry: geom,
                        tags: rel.tags.clone(),
                    });
                } else if rel.tags.contains_key("highway") {
                    let child_ids: Vec<i64> = rel
                        .members
                        .iter()
                        .filter(|m| m.member_type == "way")
                        .map(|m| m.member_ref)
                        .collect();
                    self.roads.push(Road {
                        id: rel.id,
                        name: rel.tags.get("name").cloned().unwrap_or_default(),
                        geometry: geom,
                        tags: rel.tags.clone(),
                        child_node_ids: child_ids,
                    });
                }
            }
        }
    }

    // helper counters, same semantics as the Java original

    pub fn amount_nodes(&self) -> u64 {
        self.node_map.keys().filter(|id| !self.ref_nodes.contains(*id)).count() as u64
    }

    pub fn amount_ways(&self) -> u64 {
        self.way_map.keys().filter(|id| !self.ref_ways.contains(*id)).count() as u64
    }
}

fn make_multipolygon(rel: &RawRelation, way_map: &HashMap<i64, Geometry>) -> Option<Geometry> {
    let mut outer_lines = Vec::new();
    let mut inner_lines = Vec::new();

    for mem in &rel.members {
        if mem.member_type != "way" {
            continue;
        }
        let geo = match way_map.get(&mem.member_ref) {
            Some(g) => g,
            None => continue,
        };
        let line: Vec<Coord> = match geo {
            Geometry::Polygon { outer, .. } => outer.to_vec(),
            Geometry::LineString(l) => l.to_vec(),
            _ => continue,
        };

        if mem.role == "outer" || mem.role.is_empty() {
            outer_lines.push(line);
        } else if mem.role == "inner" {
            inner_lines.push(line);
        }
    }

    let outer_rings = extract_closed_rings(outer_lines);
    let inner_rings = extract_closed_rings(inner_lines);

    if outer_rings.is_empty() {
        return None;
    }

    let mut polygons: Vec<(Ring, Vec<Ring>)> = outer_rings
        .into_iter()
        .map(|r| (Ring::from(r), Vec::new()))
        .collect();

    for hole in inner_rings {
        let hole_ring: Ring = Ring::from(hole);
        for (outer, holes) in polygons.iter_mut() {
            if point_in_ring(hole_ring[0], outer) {
                holes.push(hole_ring.clone());
                break;
            }
        }
    }

    Some(Geometry::Collection(vec![Geometry::MultiPolygon(polygons)]))
}

fn make_building(rel: &RawRelation, way_map: &HashMap<i64, Geometry>) -> Option<Geometry> {
    let parts: Vec<Geometry> = rel
        .members
        .iter()
        .filter(|m| m.member_type == "way" && m.role == "outline")
        .filter_map(|m| way_map.get(&m.member_ref).cloned())
        .collect();
    Some(Geometry::Collection(parts))
}

fn make_normal_relation(rel: &RawRelation, way_map: &HashMap<i64, Geometry>) -> Option<Geometry> {
    let parts: Vec<Geometry> = rel
        .members
        .iter()
        .filter(|m| m.member_type == "way")
        .filter_map(|m| way_map.get(&m.member_ref).cloned())
        .collect();
    Some(Geometry::Collection(parts))
}

// Phase 1: pure XML reading, no geometry
/// Holds "current element" state while streaming through the XML, plus the
/// output being accumulated. Grouped into a struct so open/close handlers
/// don't need a handful of loose `&mut` parameters each.
#[derive(Default)]
struct ParseState {
    raw: RawModel,
    current_id: i64,
    current_lat: f64,
    current_lon: f64,
    current_tags: Option<HashMap<String, String>>,
    current_way_node_ids: Option<Vec<i64>>,
    current_relation_members: Option<Vec<RawMember>>,
}

fn read_xml(file_path: &str) -> Result<RawModel, Box<dyn Error>> {
    let file = File::open(file_path)?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    reader.config_mut().trim_text(true);

    let mut state = ParseState::default();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) => {
                open_element(&e, &mut state)?;
            }
            Event::Empty(e) => {
                // Self-closing tag, e.g. <node .../>, <tag .../>, <nd .../>.
                // StAX (used on the Java side) synthesizes START+END for
                // these; quick-xml gives us one Empty event, so we run
                // both the "open" and "close" logic back to back.
                let name = local_name_owned(&e);
                open_element(&e, &mut state)?;
                close_element(&name, &mut state);
            }
            Event::End(e) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
                close_element(&name, &mut state);
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(state.raw)
}

fn local_name_owned(e: &BytesStart) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).into_owned()
}

fn attr_str(e: &BytesStart, name: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .map(|a| a.unescape_value().unwrap_or_default().into_owned())
}

fn open_element(e: &BytesStart, state: &mut ParseState) -> Result<(), Box<dyn Error>> {
    let name = local_name_owned(e);

    match name.as_str() {
        "node" => {
            state.current_id = attr_str(e, b"id").ok_or("node missing id")?.parse()?;
            state.current_lat = attr_str(e, b"lat").ok_or("node missing lat")?.parse()?;
            state.current_lon = attr_str(e, b"lon").ok_or("node missing lon")?.parse()?;
            state.current_tags = Some(HashMap::new());
        }
        "way" => {
            state.current_id = attr_str(e, b"id").ok_or("way missing id")?.parse()?;
            state.current_tags = Some(HashMap::new());
            state.current_way_node_ids = Some(Vec::new());
        }
        "relation" => {
            state.current_id = attr_str(e, b"id").ok_or("relation missing id")?.parse()?;
            state.current_tags = Some(HashMap::new());
            state.current_relation_members = Some(Vec::new());
        }
        "tag" => {
            if let Some(tags) = state.current_tags.as_mut() {
                let k = attr_str(e, b"k").unwrap_or_default();
                let v = attr_str(e, b"v").unwrap_or_default();
                tags.insert(k, v);
            }
        }
        "nd" => {
            if let Some(ids) = state.current_way_node_ids.as_mut() {
                let r: i64 = attr_str(e, b"ref").ok_or("nd missing ref")?.parse()?;
                ids.push(r);
            }
        }
        "member" => {
            if let Some(members) = state.current_relation_members.as_mut() {
                let member_type = attr_str(e, b"type").unwrap_or_default();
                let role = attr_str(e, b"role").unwrap_or_default();
                let member_ref: i64 = attr_str(e, b"ref").ok_or("member missing ref")?.parse()?;
                members.push(RawMember { member_type, role, member_ref });
            }
        }
        _ => {}
    }

    Ok(())
}

fn close_element(name: &str, state: &mut ParseState) {
    match name {
        "node" => {
            if let Some(tags) = state.current_tags.take() {
                state.raw.nodes.push(RawNode {
                    id: state.current_id,
                    lat: state.current_lat,
                    lon: state.current_lon,
                    tags,
                });
            }
        }
        "way" => {
            if let (Some(tags), Some(node_ids)) =
                (state.current_tags.take(), state.current_way_node_ids.take())
            {
                state.raw.ways.push(RawWay { id: state.current_id, tags, node_refs: node_ids });
            }
        }
        "relation" => {
            if let (Some(tags), Some(members)) =
                (state.current_tags.take(), state.current_relation_members.take())
            {
                state.raw.relations.push(RawRelation { id: state.current_id, tags, members });
            }
        }
        _ => {}
    }
}
