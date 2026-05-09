use std::collections::HashMap;
use std::fmt;
use std::process::Command;

#[derive(Debug, Clone)]
struct ZoneStats {
    name: String,
    _file: String,
    _line: u32,
    count: u64,
    total_ns: i64,
    mean_ns: f64,
    _min_ns: i64,
    _max_ns: i64,
    _std_ns: f64,
}

#[derive(Debug, Clone)]
struct ZoneEvent {
    exec_ns: i64,
}

fn run_csvexport(path: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("tracy-csvexport")
        .args(args)
        .arg(path)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "tracy-csvexport not found. Install Tracy or add it to PATH.\n\
                 Build from https://github.com/wolfpld/tracy or install via your package manager."
                    .to_string()
            } else {
                format!("failed to run tracy-csvexport: {e}")
            }
        })?;

    if !output.status.success() {
        return Err(format!(
            "tracy-csvexport failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_zone_stats(csv: &str) -> Vec<ZoneStats> {
    csv.lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() < 10 {
                return None;
            }
            Some(ZoneStats {
                name: f[0].to_string(),
                _file: f[1].to_string(),
                _line: f[2].parse().unwrap_or(0),
                count: f[5].parse().unwrap_or(0),
                total_ns: f[3].parse().unwrap_or(0),
                mean_ns: f[6].parse().unwrap_or(0.0),
                _min_ns: f[7].parse().unwrap_or(0),
                _max_ns: f[8].parse().unwrap_or(0),
                _std_ns: f[9].parse().unwrap_or(0.0),
            })
        })
        .collect()
}

fn parse_zone_events(csv: &str) -> Vec<ZoneEvent> {
    csv.lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() < 5 {
                return None;
            }
            Some(ZoneEvent {
                exec_ns: f[4].parse().unwrap_or(0),
            })
        })
        .collect()
}

fn percentile(sorted: &[i64], p: f64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p / 100.0).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn median(sorted: &[i64]) -> i64 {
    percentile(sorted, 50.0)
}

fn std_dev(vals: &[i64]) -> f64 {
    if vals.len() < 2 {
        return 0.0;
    }
    let mean = vals.iter().sum::<i64>() as f64 / vals.len() as f64;
    let var =
        vals.iter().map(|v| (*v as f64 - mean).powi(2)).sum::<f64>() / (vals.len() - 1) as f64;
    var.sqrt()
}

fn fmt_ns(ns: f64) -> String {
    if ns.abs() >= 1_000_000.0 {
        format!("{:.2} ms", ns / 1_000_000.0)
    } else if ns.abs() >= 1_000.0 {
        format!("{:.1} µs", ns / 1_000.0)
    } else {
        format!("{:.0} ns", ns)
    }
}

fn fmt_delta(before: f64, after: f64) -> String {
    if before == 0.0 {
        return "n/a".into();
    }
    let pct = (after - before) / before * 100.0;
    let sign = if pct >= 0.0 { "+" } else { "" };
    format!("{sign}{pct:.1}%")
}

enum Color {
    Red,
    Green,
    Yellow,
    Bold,
    Dim,
    Reset,
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !USE_COLOR.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        match self {
            Color::Red => write!(f, "\x1b[31m"),
            Color::Green => write!(f, "\x1b[32m"),
            Color::Yellow => write!(f, "\x1b[33m"),
            Color::Bold => write!(f, "\x1b[1m"),
            Color::Dim => write!(f, "\x1b[2m"),
            Color::Reset => write!(f, "\x1b[0m"),
        }
    }
}

static USE_COLOR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn delta_colored(before: f64, after: f64) -> String {
    if before == 0.0 {
        return "n/a".into();
    }
    let pct = (after - before) / before * 100.0;
    let sign = if pct >= 0.0 { "+" } else { "" };
    let color = if pct.abs() < 5.0 {
        Color::Dim
    } else if pct > 0.0 {
        Color::Red
    } else {
        Color::Green
    };
    format!("{color}{sign}{pct:.1}%{}", Color::Reset)
}

struct TraceData {
    zones: Vec<ZoneStats>,
    self_zones: Vec<ZoneStats>,
}

fn load_trace(path: &str) -> Result<TraceData, String> {
    let csv = run_csvexport(path, &[])?;
    let zones = parse_zone_stats(&csv);
    let self_csv = run_csvexport(path, &["-e"])?;
    let self_zones = parse_zone_stats(&self_csv);
    Ok(TraceData { zones, self_zones })
}

fn load_zone_events(path: &str, zone: &str) -> Result<Vec<ZoneEvent>, String> {
    let csv = run_csvexport(path, &["-u", "-f", zone])?;
    Ok(parse_zone_events(&csv))
}

fn print_header(a_path: &str, b_path: &str) {
    println!(
        "\n{}tracy-diff{} — comparing traces\n",
        Color::Bold,
        Color::Reset
    );
    println!("  {}A (before):{} {a_path}", Color::Dim, Color::Reset);
    println!("  {}B (after): {} {b_path}", Color::Dim, Color::Reset);
}

fn print_zone_table(a: &TraceData, b: &TraceData) {
    println!("\n{}Zone summary{}\n", Color::Bold, Color::Reset);

    let a_map: HashMap<&str, &ZoneStats> = a.zones.iter().map(|z| (z.name.as_str(), z)).collect();
    let b_map: HashMap<&str, &ZoneStats> = b.zones.iter().map(|z| (z.name.as_str(), z)).collect();
    let a_self: HashMap<&str, &ZoneStats> =
        a.self_zones.iter().map(|z| (z.name.as_str(), z)).collect();
    let b_self: HashMap<&str, &ZoneStats> =
        b.self_zones.iter().map(|z| (z.name.as_str(), z)).collect();

    let mut names: Vec<&str> = a_map.keys().chain(b_map.keys()).copied().collect();
    names.sort();
    names.dedup();
    names.sort_by(|x, y| {
        let bx = b_map.get(x).map(|z| z.total_ns).unwrap_or(0);
        let by = b_map.get(y).map(|z| z.total_ns).unwrap_or(0);
        by.cmp(&bx)
    });

    println!(
        "  {d}{:<30} {:>10} {:>10} {:>8}  {:>10} {:>10} {:>8}{r}",
        "zone",
        "A mean",
        "B mean",
        "Δ mean",
        "A self",
        "B self",
        "Δ self",
        d = Color::Dim,
        r = Color::Reset
    );
    println!("  {}", "─".repeat(94));

    for name in &names {
        let a_mean = a_map.get(name).map(|z| z.mean_ns).unwrap_or(0.0);
        let b_mean = b_map.get(name).map(|z| z.mean_ns).unwrap_or(0.0);
        let a_self_mean = a_self.get(name).map(|z| z.mean_ns).unwrap_or(0.0);
        let b_self_mean = b_self.get(name).map(|z| z.mean_ns).unwrap_or(0.0);

        println!(
            "  {:<30} {:>10} {:>10} {:>8}  {:>10} {:>10} {:>8}",
            name,
            fmt_ns(a_mean),
            fmt_ns(b_mean),
            delta_colored(a_mean, b_mean),
            fmt_ns(a_self_mean),
            fmt_ns(b_self_mean),
            delta_colored(a_self_mean, b_self_mean),
        );
    }

    let a_count = a.zones.first().map(|z| z.count).unwrap_or(0);
    let b_count = b.zones.first().map(|z| z.count).unwrap_or(0);
    println!(
        "\n  {}Frames: A={}, B={}{}",
        Color::Dim,
        a_count,
        b_count,
        Color::Reset
    );
}

fn print_zone_detail(a_path: &str, b_path: &str, zone: &str) {
    println!("\n{}Zone detail: {zone}{}\n", Color::Bold, Color::Reset);

    let a_events = match load_zone_events(a_path, zone) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("  warning: could not load A events: {e}");
            return;
        }
    };
    let b_events = match load_zone_events(b_path, zone) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("  warning: could not load B events: {e}");
            return;
        }
    };

    if a_events.is_empty() && b_events.is_empty() {
        println!("  no events found for zone '{zone}'");
        return;
    }

    let stats = |events: &[ZoneEvent]| -> (f64, i64, i64, i64, i64, f64) {
        let mut times: Vec<i64> = events.iter().map(|e| e.exec_ns).collect();
        times.sort();
        let mean = if times.is_empty() {
            0.0
        } else {
            times.iter().sum::<i64>() as f64 / times.len() as f64
        };
        (
            mean,
            median(&times),
            percentile(&times, 95.0),
            percentile(&times, 99.0),
            *times.first().unwrap_or(&0),
            std_dev(&times),
        )
    };

    let (a_mean, a_med, a_p95, a_p99, a_min, a_std) = stats(&a_events);
    let (b_mean, b_med, b_p95, b_p99, b_min, b_std) = stats(&b_events);

    println!(
        "  {d}{:<12} {:>12} {:>12} {:>10}{r}",
        "",
        "A",
        "B",
        "delta",
        d = Color::Dim,
        r = Color::Reset
    );
    println!("  {}", "─".repeat(50));

    let rows: &[(&str, f64, f64, bool)] = &[
        ("calls", a_events.len() as f64, b_events.len() as f64, true),
        ("mean", a_mean, b_mean, false),
        ("median", a_med as f64, b_med as f64, false),
        ("p95", a_p95 as f64, b_p95 as f64, false),
        ("p99", a_p99 as f64, b_p99 as f64, false),
        ("min", a_min as f64, b_min as f64, false),
        ("std dev", a_std, b_std, false),
    ];

    for (label, av, bv, is_count) in rows {
        if *is_count {
            println!(
                "  {:<12} {:>12} {:>12} {:>10}",
                label,
                *av as u64,
                *bv as u64,
                fmt_delta(*av, *bv)
            );
        } else {
            println!(
                "  {:<12} {:>12} {:>12} {:>10}",
                label,
                fmt_ns(*av),
                fmt_ns(*bv),
                delta_colored(*av, *bv)
            );
        }
    }
}

fn print_usage() {
    eprintln!("tracy-diff — compare two Tracy profiler traces\n");
    eprintln!("usage: tracy-diff <before.tracy> <after.tracy> [options]\n");
    eprintln!("options:");
    eprintln!("  -z, --zone <name>    detailed stats for a specific zone");
    eprintln!("  --no-color           disable colored output");
    eprintln!("  -h, --help           show this help");
    eprintln!("\nexample:");
    eprintln!("  tracy-diff pre.tracy after.tracy -z layout::write_back");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut positional = Vec::new();
    let mut zone_filter: Option<String> = None;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage();
                return;
            }
            "--no-color" => {
                USE_COLOR.store(false, std::sync::atomic::Ordering::Relaxed);
            }
            "-z" | "--zone" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: --zone requires an argument");
                    std::process::exit(1);
                }
                zone_filter = Some(args[i].clone());
            }
            a if a.starts_with('-') => {
                eprintln!("unknown option: {a}");
                print_usage();
                std::process::exit(1);
            }
            _ => positional.push(args[i].clone()),
        }
        i += 1;
    }

    if positional.len() != 2 {
        print_usage();
        std::process::exit(1);
    }

    let (a_path, b_path) = (&positional[0], &positional[1]);

    for p in [a_path, b_path] {
        if !std::path::Path::new(p).exists() {
            eprintln!("error: file not found: {p}");
            std::process::exit(1);
        }
    }

    let a = match load_trace(a_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error loading {a_path}: {e}");
            std::process::exit(1);
        }
    };
    let b = match load_trace(b_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error loading {b_path}: {e}");
            std::process::exit(1);
        }
    };

    print_header(a_path, b_path);
    print_zone_table(&a, &b);

    if let Some(zone) = &zone_filter {
        print_zone_detail(a_path, b_path, zone);
    } else {
        // Auto-detect regressions
        let a_self: HashMap<&str, f64> = a
            .self_zones
            .iter()
            .map(|z| (z.name.as_str(), z.mean_ns))
            .collect();
        let mut regressions: Vec<(&str, f64, f64)> = b
            .self_zones
            .iter()
            .filter_map(|z| {
                let a_mean = a_self.get(z.name.as_str()).copied().unwrap_or(0.0);
                if a_mean > 100.0 && z.mean_ns > a_mean * 1.1 {
                    Some((z.name.as_str(), a_mean, z.mean_ns))
                } else {
                    None
                }
            })
            .collect();
        regressions.sort_by(|a, b| {
            let ratio_b = b.2 / b.1;
            let ratio_a = a.2 / a.1;
            ratio_b
                .partial_cmp(&ratio_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if !regressions.is_empty() {
            println!(
                "\n  {}Regressions detected (>10% self-time increase):{}",
                Color::Yellow,
                Color::Reset
            );
            for (name, a_mean, b_mean) in &regressions {
                println!(
                    "    {name}  {} → {} ({})",
                    fmt_ns(*a_mean),
                    fmt_ns(*b_mean),
                    delta_colored(*a_mean, *b_mean)
                );
            }
            println!(
                "\n  {}Run with -z <zone> for per-event breakdown.{}",
                Color::Dim,
                Color::Reset
            );
        }
    }

    println!();
}
