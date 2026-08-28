package osmparser;

import java.util.*;

/*
    Plain data holders filled during Phase 1 (pure XML reading).
    No JTS geometry is touched here on purpose - this lets us time
    "just parsing the XML" separately from "building geometry".
*/
final class RawModel
{
    static final class RawNode
    {
        final long id;
        final double lat;
        final double lon;
        final Map<String, String> tags;

        RawNode(long id, double lat, double lon, Map<String, String> tags)
        {
            this.id = id;
            this.lat = lat;
            this.lon = lon;
            this.tags = tags;
        }
    }

    static final class RawWay
    {
        final long id;
        final Map<String, String> tags;
        final List<Long> nodeRefs;

        RawWay(long id, Map<String, String> tags, List<Long> nodeRefs)
        {
            this.id = id;
            this.tags = tags;
            this.nodeRefs = nodeRefs;
        }
    }

    static final class RawRelationMember
    {
        final String type;
        final String role;
        final long ref;

        RawRelationMember(String type, String role, long ref)
        {
            this.type = type;
            this.role = role;
            this.ref = ref;
        }
    }

    static final class RawRelation
    {
        final long id;
        final Map<String, String> tags;
        final List<RawRelationMember> members;

        RawRelation(long id, Map<String, String> tags, List<RawRelationMember> members)
        {
            this.id = id;
            this.tags = tags;
            this.members = members;
        }
    }

    final List<RawNode> nodes = new ArrayList<>();
    final List<RawWay> ways = new ArrayList<>();
    final List<RawRelation> relations = new ArrayList<>();
}
