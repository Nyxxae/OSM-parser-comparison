package osmparser;

import org.locationtech.jts.geom.Geometry;

import java.util.List;
import java.util.Map;

public class Road
{
    private final long id;
    private final String name;
    private final Geometry geometry;
    private final Map<String, String> tags;
    private final List<Long> childNodeIds;

    public Road(long id, String name, Geometry geometry, Map<String, String> tags, List<Long> childNodeIds)
    {
        this.id = id;
        this.name = name;
        this.geometry = geometry;
        this.tags = tags;
        this.childNodeIds = childNodeIds;
    }

    public long getId() { return id; }
    public String getName() { return name; }
    public Geometry getGeometry() { return geometry; }
    public Map<String, String> getTags() { return tags; }
    public List<Long> getChildNodeIds() { return childNodeIds; }
}
