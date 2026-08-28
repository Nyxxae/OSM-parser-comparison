package osmparser;

import org.locationtech.jts.geom.*;
import org.locationtech.jts.operation.linemerge.LineMerger;

import javax.xml.stream.XMLInputFactory;
import javax.xml.stream.XMLStreamConstants;
import javax.xml.stream.XMLStreamReader;
import java.io.FileInputStream;
import java.io.InputStream;
import java.util.*;

import osmparser.RawModel.RawNode;
import osmparser.RawModel.RawWay;
import osmparser.RawModel.RawRelation;
import osmparser.RawModel.RawRelationMember;

/*
  Same logic as the original project's OsmParser, but split into
  independently timed phases so it can be seen whether time is spent
  reading XML or building JTS geometry. That split is the whole point
  of this standalone version - it tells you what a Rust rewrite would
  actually speed up.
 
  Phase 1 - readXml:        pure StAX tokenizing into plain data (RawModel). No JTS involved.
  Phase 2 - buildNodes:     turn raw lat/lon into JTS Points, collect node amenities.
  Phase 3 - buildWays:      turn node-id chains into LineString/Polygon geometry, collect way amenities/roads.
  Phase 4 - buildRelations: multipolygon/building/normal relation assembly (LineMerger, covers() checks - usually the heaviest part).
*/
public class OsmParser
{
    private final List<Amenity> amenities = new ArrayList<>();
    private final List<Road> roads = new ArrayList<>();

    private long relationCount = 0;
    private final Set<Long> refNodes = new HashSet<>();
    private final Set<Long> refWays = new HashSet<>();
    private final GeometryFactory geometryFactory = new GeometryFactory();

    private final Map<Long, Point> nodeMap = new HashMap<>();
    private final Map<Long, Geometry> wayMap = new HashMap<>();

    private final LinkedHashMap<String, Long> phaseTimingsNanos = new LinkedHashMap<>();

    public List<Amenity> getAmenities() { return amenities; }
    public List<Road> getRoads() { return roads; }
    public long getRelationCount() { return relationCount; }
    public Map<Long, Point> getNodeMap() { return nodeMap; }
    public Map<Long, Geometry> getWayMap() { return wayMap; }
    public Map<String, Long> getPhaseTimingsNanos() { return phaseTimingsNanos; }

    /*
    Runs all phases in order and records how long each one took.
    File I/O for reading the OSM file is included inside Phase 1,
    since streaming XML reading and file reading aren't meaningfully
    separable with StAX (it pulls bytes as it parses).
     */
    public void parse(String filePath) throws Exception
    {
        RawModel raw = timed("1-read-xml", () -> readXml(filePath));
        timed("2-build-nodes", () -> { buildNodes(raw); return null; });
        timed("3-build-ways", () -> { buildWays(raw); return null; });
        timed("4-build-relations", () -> { buildRelations(raw); return null; });
    }

    private interface ThrowingSupplier<T> { T get() throws Exception; }

    private <T> T timed(String phaseName, ThrowingSupplier<T> block) throws Exception
    {
        long start = System.nanoTime();
        T result = block.get();
        long elapsed = System.nanoTime() - start;
        phaseTimingsNanos.put(phaseName, elapsed);
        return result;
    }

    // Phase 1: pure XML reading, no geometry

    private RawModel readXml(String filePath) throws Exception
    {
        RawModel raw = new RawModel();

        try (InputStream in = new FileInputStream(filePath))
        {
            XMLInputFactory factory = XMLInputFactory.newInstance();
            XMLStreamReader reader = factory.createXMLStreamReader(in);

            long currentId = -1;
            Map<String, String> currentTags = null;
            double currentLat = 0, currentLon = 0;
            List<Long> currentWayNodeIds = null;
            List<RawRelationMember> currentRelationMembers = null;

            while (reader.hasNext())
            {
                int event = reader.next();

                if (event == XMLStreamConstants.START_ELEMENT)
                {
                    String tagName = reader.getLocalName();
                    switch (tagName)
                    {
                        case "node":
                            currentId = Long.parseLong(reader.getAttributeValue(null, "id"));
                            currentLat = Double.parseDouble(reader.getAttributeValue(null, "lat"));
                            currentLon = Double.parseDouble(reader.getAttributeValue(null, "lon"));
                            currentTags = new HashMap<>();
                            break;

                        case "way":
                            currentId = Long.parseLong(reader.getAttributeValue(null, "id"));
                            currentTags = new HashMap<>();
                            currentWayNodeIds = new ArrayList<>();
                            break;

                        case "relation":
                            currentId = Long.parseLong(reader.getAttributeValue(null, "id"));
                            currentTags = new HashMap<>();
                            currentRelationMembers = new ArrayList<>();
                            break;

                        case "tag":
                            if (currentTags != null)
                                currentTags.put(reader.getAttributeValue(null, "k"), reader.getAttributeValue(null, "v"));
                            break;

                        case "nd":
                            if (currentWayNodeIds != null)
                                currentWayNodeIds.add(Long.parseLong(reader.getAttributeValue(null, "ref")));
                            break;

                        case "member":
                            if (currentRelationMembers != null)
                            {
                                String type = reader.getAttributeValue(null, "type");
                                String role = reader.getAttributeValue(null, "role");
                                long ref = Long.parseLong(reader.getAttributeValue(null, "ref"));
                                currentRelationMembers.add(new RawRelationMember(type, role, ref));
                            }
                            break;
                    }
                }
                else if (event == XMLStreamConstants.END_ELEMENT)
                {
                    String tagName = reader.getLocalName();

                    if ("node".equals(tagName))
                        raw.nodes.add(new RawNode(currentId, currentLat, currentLon, currentTags));
                    else if ("way".equals(tagName))
                        raw.ways.add(new RawWay(currentId, currentTags, currentWayNodeIds));
                    else if ("relation".equals(tagName))
                        raw.relations.add(new RawRelation(currentId, currentTags, currentRelationMembers));
                }
            }
            reader.close();
        }

        return raw;
    }

    // Phase 2: node geometry

    private void buildNodes(RawModel raw)
    {
        for (RawNode n : raw.nodes)
        {
            Point point = geometryFactory.createPoint(new Coordinate(n.lon, n.lat));
            nodeMap.put(n.id, point);

            if (n.tags.containsKey("amenity"))
                amenities.add(new Amenity(n.id, n.tags.getOrDefault("name", ""), point, n.tags));
        }
    }

    // Phase 3: way geometry

    private void buildWays(RawModel raw)
    {
        for (RawWay way : raw.ways)
        {
            for (Long ref : way.nodeRefs)
                refNodes.add(ref);

            processWay(way.id, way.tags, way.nodeRefs);
        }
    }

    private void processWay(long id, Map<String, String> tags, List<Long> nodeIds)
    {
        Coordinate[] cords = extractCoordinates(nodeIds);
        if (cords.length == 0)
            return;

        List<Long> childIdsCopy = new ArrayList<>(nodeIds);
        Geometry wayGeom;
        if (cords.length == 1)
            wayGeom = geometryFactory.createPoint(cords[0]);
        else
        {
            boolean isClosed = cords[0].equals(cords[cords.length - 1]);
            if (isClosed && cords.length > 3)
            {
                LinearRing ring = geometryFactory.createLinearRing(cords);
                wayGeom = geometryFactory.createPolygon(ring);
            }
            else
            {
                wayGeom = geometryFactory.createLineString(cords);
            }
        }
        wayMap.put(id, wayGeom);

        if (tags.containsKey("highway") && cords.length >= 2)
            roads.add(new Road(id, tags.getOrDefault("name", ""), wayGeom, tags, childIdsCopy));
        if (tags.containsKey("amenity"))
            amenities.add(new Amenity(id, tags.getOrDefault("name", ""), wayGeom, tags));
    }

    private Coordinate[] extractCoordinates(List<Long> nodeIds)
    {
        List<Coordinate> cordsList = new ArrayList<>();
        if (nodeIds == null)
            return new Coordinate[0];
        for (Long id : nodeIds)
        {
            Point pt = nodeMap.get(id);
            if (pt != null)
                cordsList.add(pt.getCoordinate());
        }
        return cordsList.toArray(new Coordinate[0]);
    }

    // Phase 4: relations (usually the expensive part)

    private void buildRelations(RawModel raw)
    {
        for (RawRelation rel : raw.relations)
        {
            relationCount++;
            for (RawRelationMember mem : rel.members)
            {
                if ("way".equals(mem.type))
                    refWays.add(mem.ref);
                else if ("node".equals(mem.type))
                    refNodes.add(mem.ref);
            }
            processRelation(rel.id, rel.tags, rel.members);
        }
    }

    private void processRelation(long id, Map<String, String> tags, List<RawRelationMember> members)
    {
        String type = tags.get("type");
        GeometryCollection finalCollection;

        if ("multipolygon".equals(type))
            finalCollection = makeStrangePolygon(members);
        else if ("building".equals(type))
            finalCollection = buildABuilding(members);
        else
            finalCollection = makeNormalRelation(members);

        if (finalCollection != null && !finalCollection.isEmpty())
        {
            if (tags.containsKey("amenity"))
                amenities.add(new Amenity(id, tags.getOrDefault("name", ""), finalCollection, tags));
            else if (tags.containsKey("highway"))
            {
                List<Long> childIds = new ArrayList<>();
                for (RawRelationMember mem : members)
                    if ("way".equals(mem.type))
                        childIds.add(mem.ref);
                roads.add(new Road(id, tags.getOrDefault("name", ""), finalCollection, tags, childIds));
            }
        }
    }

    private GeometryCollection makeStrangePolygon(List<RawRelationMember> members)
    {
        List<LineString> outerLines = new ArrayList<>();
        List<LineString> innerLines = new ArrayList<>();

        for (RawRelationMember mem : members)
        {
            if (!"way".equals(mem.type))
                continue;
            Geometry geo = wayMap.get(mem.ref);
            if (geo == null)
                continue;

            LineString line = null;
            if (geo instanceof Polygon)
                line = ((Polygon) geo).getExteriorRing();
            else if (geo instanceof LineString)
                line = (LineString) geo;
            if (line == null)
                continue;

            if ("outer".equals(mem.role) || "".equals(mem.role))
                outerLines.add(line);
            else if ("inner".equals(mem.role))
                innerLines.add(line);
        }

        List<LinearRing> outerRings = makeRings(outerLines);
        List<LinearRing> innerRings = makeRings(innerLines);

        if (outerRings.isEmpty())
            return null;

        Map<LinearRing, List<LinearRing>> contourWithInners = new HashMap<>();
        for (LinearRing cont : outerRings)
            contourWithInners.put(cont, new ArrayList<>());

        for (LinearRing hole : innerRings)
        {
            Polygon holePolygon = geometryFactory.createPolygon(hole);
            for (LinearRing cont : outerRings)
            {
                Polygon contour = geometryFactory.createPolygon(cont);
                if (contour.covers(holePolygon))
                {
                    contourWithInners.get(cont).add(hole);
                    break;
                }
            }
        }

        List<Polygon> polygons = new ArrayList<>();
        for (Map.Entry<LinearRing, List<LinearRing>> entry : contourWithInners.entrySet())
        {
            LinearRing cont = entry.getKey();
            LinearRing[] holesArr = entry.getValue().toArray(new LinearRing[0]);
            polygons.add(geometryFactory.createPolygon(cont, holesArr));
        }

        MultiPolygon multiPolygon = geometryFactory.createMultiPolygon(polygons.toArray(new Polygon[0]));
        return new GeometryCollection(new Geometry[]{multiPolygon}, geometryFactory);
    }

    private GeometryCollection buildABuilding(List<RawRelationMember> members)
    {
        List<Geometry> parts = new ArrayList<>();
        for (RawRelationMember mem : members)
        {
            if (!"way".equals(mem.type))
                continue;
            if ("outline".equals(mem.role))
            {
                Geometry geo = wayMap.get(mem.ref);
                if (geo != null)
                    parts.add(geo);
            }
        }
        return new GeometryCollection(parts.toArray(new Geometry[0]), geometryFactory);
    }

    private GeometryCollection makeNormalRelation(List<RawRelationMember> members)
    {
        List<Geometry> parts = new ArrayList<>();
        for (RawRelationMember mem : members)
        {
            if ("way".equals(mem.type))
            {
                Geometry geo = wayMap.get(mem.ref);
                if (geo != null)
                    parts.add(geo);
            }
        }
        return new GeometryCollection(parts.toArray(new Geometry[0]), geometryFactory);
    }

    private List<LinearRing> makeRings(List<LineString> lines)
    {
        List<LinearRing> rings = new ArrayList<>();
        if (lines.isEmpty())
            return rings;

        LineMerger merger = new LineMerger();
        for (LineString line : lines)
            merger.add(line);

        Collection<Geometry> merged = merger.getMergedLineStrings();
        for (Geometry geo : merged)
        {
            if (geo instanceof LineString)
            {
                LineString line = (LineString) geo;
                if (line.isClosed() && line.getNumPoints() >= 4)
                    rings.add(geometryFactory.createLinearRing(line.getCoordinates()));
            }
        }
        return rings;
    }

    // helper counters, same semantics as the original

    public long amountNodes()
    {
        long autonomous = 0;
        for (Long id : nodeMap.keySet())
            if (!refNodes.contains(id))
                autonomous++;
        return autonomous;
    }

    public long amountWays()
    {
        long autonomous = 0;
        for (Long id : wayMap.keySet())
            if (!refWays.contains(id))
                autonomous++;
        return autonomous;
    }
}
