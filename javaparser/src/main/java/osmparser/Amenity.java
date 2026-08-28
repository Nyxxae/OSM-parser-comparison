package osmparser;

import org.locationtech.jts.geom.Geometry;

import java.util.Map;

public class Amenity
{
    private final long id;
    private final String name;
    private final Geometry geometry;
    private final Map<String, String> tags;

    public Amenity(long id, String name, Geometry geometry, Map<String, String> tags)
    {
        this.id = id;
        this.name = name;
        this.geometry = geometry;
        this.tags = tags;
    }

    public long getId() { return id; }
    public String getName() { return name; }
    public Geometry getGeometry() { return geometry; }
    public Map<String, String> getTags() { return tags; }
}
