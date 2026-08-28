package osmparser;

import java.util.*;

/*
 Standalone entry point.
 
 Usage:
 java -jar osm-parser.jar <path-to.osm> [iterations] [warmupIterations]
 
 Example:
 java -jar osm-parser.jar warsaw.osm 10 3
 
 A fresh OsmParser instance is created on every iteration on purpose -
 the original class accumulates state in instance fields, so reusing
 one instance across iterations would double-count data.
 
 This is a simple manual benchmark, not a replacement for JMH. It's
 enough to get a first read on where time goes (XML reading vs
 geometry building vs relation processing) before you invest in a
 proper JMH or Criterion.rs setup.
*/
public class Main
{
    public static void main(String[] args) throws Exception
    {
        if (args.length < 1)
        {
            System.err.println("Usage: java -jar osm-parser.jar <path-to.osm> [iterations] [warmupIterations]");
            System.exit(1);
        }

        String filePath = args[0];
        int iterations = args.length >= 2 ? Integer.parseInt(args[1]) : 5;
        int warmup = args.length >= 3 ? Integer.parseInt(args[2]) : 2;

        System.out.println("File:       " + filePath);
        System.out.println("Warmup:     " + warmup + " iteration(s)");
        System.out.println("Measured:   " + iterations + " iteration(s)");
        System.out.println();

        // Warmup - lets the JIT compile hot paths before we start recording numbers.
        for (int i = 0; i < warmup; i++)
        {
            OsmParser parser = new OsmParser();
            parser.parse(filePath);
            System.out.println("Warmup " + (i + 1) + "/" + warmup + " done.");
        }

        List<Map<String, Long>> allTimings = new ArrayList<>();
        OsmParser lastParser = null;

        for (int i = 0; i < iterations; i++)
        {
            OsmParser parser = new OsmParser();
            long start = System.nanoTime();
            parser.parse(filePath);
            long total = System.nanoTime() - start;

            Map<String, Long> timings = new LinkedHashMap<>(parser.getPhaseTimingsNanos());
            timings.put("TOTAL", total);
            allTimings.add(timings);
            lastParser = parser;

            System.out.printf("Run %d/%d: total=%.2f ms%n", i + 1, iterations, total / 1_000_000.0);
        }

        System.out.println();
        printReport(allTimings);

        System.out.println();
        printResultCounts(lastParser);
    }

    private static void printReport(List<Map<String, Long>> allTimings)
    {
        System.out.println("=== Phase timing (ms), averaged over " + allTimings.size() + " run(s) ===");

        LinkedHashSet<String> phaseNames = new LinkedHashSet<>();
        for (Map<String, Long> run : allTimings)
            phaseNames.addAll(run.keySet());

        for (String phase : phaseNames)
        {
            List<Long> values = new ArrayList<>();
            for (Map<String, Long> run : allTimings)
                if (run.containsKey(phase))
                    values.add(run.get(phase));

            double avgMs = values.stream().mapToLong(Long::longValue).average().orElse(0) / 1_000_000.0;
            long minMs = values.stream().mapToLong(Long::longValue).min().orElse(0) / 1_000_000;
            long maxMs = values.stream().mapToLong(Long::longValue).max().orElse(0) / 1_000_000;

            System.out.printf("  %-20s avg=%.2f ms   min=%d ms   max=%d ms%n", phase, avgMs, minMs, maxMs);
        }
    }

    private static void printResultCounts(OsmParser parser)
    {
        System.out.println("=== Parsed data (last run) ===");
        System.out.println("  Nodes (raw):     " + parser.getNodeMap().size());
        System.out.println("  Ways (raw):      " + parser.getWayMap().size());
        System.out.println("  Relations:       " + parser.getRelationCount());
        System.out.println("  Amenities:       " + parser.getAmenities().size());
        System.out.println("  Roads:           " + parser.getRoads().size());
        System.out.println("  Standalone nodes:" + parser.amountNodes());
        System.out.println("  Standalone ways: " + parser.amountWays());
    }
}
