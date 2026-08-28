use osm_parser_rust::parser::OsmParser;
use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: osm-parser-rust <path-to.osm> [iterations] [warmupIterations]");
        std::process::exit(1);
    }

    let file_path = &args[1];
    let iterations: usize = args.get(2).map(|s| s.parse()).transpose()?.unwrap_or(5);
    let warmup: usize = args.get(3).map(|s| s.parse()).transpose()?.unwrap_or(2);

    println!("File:       {}", file_path);
    println!("Warmup:     {} iteration(s)", warmup);
    println!("Measured:   {} iteration(s)", iterations);
    println!();

    for i in 0..warmup {
        let mut parser = OsmParser::new();
        parser.parse(file_path)?;
        println!("Warmup {}/{} done.", i + 1, warmup);
    }

    let mut all_timings: Vec<Vec<(String, Duration)>> = Vec::new();
    let mut last_parser: Option<OsmParser> = None;

    for i in 0..iterations {
        let mut parser = OsmParser::new();
        let start = Instant::now();
        parser.parse(file_path)?;
        let total = start.elapsed();

        let mut timings = parser.phase_timings.clone();
        timings.push(("TOTAL".to_string(), total));
        all_timings.push(timings);

        println!("Run {}/{}: total={:.2} ms", i + 1, iterations, total.as_secs_f64() * 1000.0);
        last_parser = Some(parser);
    }

    println!();
    print_report(&all_timings);

    println!();
    if let Some(parser) = last_parser {
        print_result_counts(&parser);
    }

    Ok(())
}

fn print_report(all_timings: &[Vec<(String, Duration)>]) {
    println!("=== Phase timing (ms), averaged over {} run(s) ===", all_timings.len());

    // BTreeMap keeps phase names in a stable, sorted order (1-, 2-, 3-, 4-, TOTAL).
    let mut by_phase: BTreeMap<String, Vec<Duration>> = BTreeMap::new();
    for run in all_timings {
        for (name, dur) in run {
            by_phase.entry(name.clone()).or_default().push(*dur);
        }
    }

    for (phase, values) in &by_phase {
        let total_nanos: u128 = values.iter().map(|d| d.as_nanos()).sum();
        let avg_ms = (total_nanos as f64 / values.len() as f64) / 1_000_000.0;
        let min_ms = values.iter().map(|d| d.as_millis()).min().unwrap_or(0);
        let max_ms = values.iter().map(|d| d.as_millis()).max().unwrap_or(0);

        println!("  {:<20} avg={:.2} ms   min={} ms   max={} ms", phase, avg_ms, min_ms, max_ms);
    }
}

fn print_result_counts(parser: &OsmParser) {
    println!("=== Parsed data (last run) ===");
    println!("  Nodes (raw):     {}", parser.node_map.len());
    println!("  Ways (raw):      {}", parser.way_map.len());
    println!("  Relations:       {}", parser.relation_count);
    println!("  Amenities:       {}", parser.amenities.len());
    println!("  Roads:           {}", parser.roads.len());
    println!("  Standalone nodes:{}", parser.amount_nodes());
    println!("  Standalone ways: {}", parser.amount_ways());
}
