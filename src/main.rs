use std::collections::BTreeMap;
use std::env;
use std::io;
use std::path::PathBuf;
use std::process::{self, Command};
use std::thread;
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

const MIB: u64 = 1024 * 1024;

#[derive(Clone, Debug)]
struct ProcessGroup {
    name: String,
    count: usize,
    bytes: u64,
}

struct Snapshot {
    total: u64,
    available: u64,
    groups: Vec<ProcessGroup>,
    own_bytes: u64,
}

fn take_snapshot(system: &mut System) -> Snapshot {
    system.refresh_memory();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );

    let mut groups: BTreeMap<String, ProcessGroup> = BTreeMap::new();
    let mut own_bytes = 0;
    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy().into_owned();
        let entry = groups.entry(name.clone()).or_insert_with(|| ProcessGroup {
            name,
            count: 0,
            bytes: 0,
        });
        entry.count += 1;
        entry.bytes += process.memory();
        if pid.as_u32() == process::id() {
            own_bytes = process.memory();
        }
    }

    let mut groups: Vec<_> = groups.into_values().collect();
    groups.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    Snapshot {
        total: system.total_memory(),
        available: system.available_memory(),
        groups,
        own_bytes,
    }
}

fn named_bytes(groups: &[ProcessGroup], names: &[&str]) -> u64 {
    groups
        .iter()
        .filter(|group| {
            names
                .iter()
                .any(|name| group.name.eq_ignore_ascii_case(name))
        })
        .map(|group| group.bytes)
        .sum()
}

fn print_snapshot(snapshot: &Snapshot) {
    println!(
        "Available: {} MiB / {} MiB | Monitor: {} MiB",
        snapshot.available / MIB,
        snapshot.total / MIB,
        snapshot.own_bytes / MIB
    );
    println!(
        "Brave: {} MiB | ChatGPT/Codex: {} MiB",
        named_bytes(&snapshot.groups, &["brave.exe", "brave"]) / MIB,
        named_bytes(
            &snapshot.groups,
            &["ChatGPT.exe", "ChatGPT", "codex.exe", "codex"]
        ) / MIB
    );
    println!("{:<30} {:>6} {:>10}", "Process", "Count", "MiB*");
    for group in snapshot.groups.iter().take(18) {
        println!(
            "{:<30} {:>6} {:>10}",
            group.name,
            group.count,
            group.bytes / MIB
        );
    }
    println!("* Summed working sets can count shared pages more than once.");
}

fn tracked_bytes(snapshot: &Snapshot) -> [u64; 4] {
    [
        named_bytes(&snapshot.groups, &["brave.exe", "brave"]),
        named_bytes(&snapshot.groups, &["chrome.exe", "chrome"]),
        named_bytes(&snapshot.groups, &["opera.exe", "opera"]),
        named_bytes(
            &snapshot.groups,
            &["ChatGPT.exe", "ChatGPT", "codex.exe", "codex"],
        ),
    ]
}

fn print_delta(label: &str, before: u64, after: u64) {
    let delta = (after as i128 - before as i128) / MIB as i128;
    println!(
        "{label:<16} {:>8} {:>8} {delta:>+8} MiB",
        before / MIB,
        after / MIB
    );
}

fn compare_snapshots(before: &Snapshot, after: &Snapshot) {
    println!(
        "{:<16} {:>8} {:>8} {:>12}",
        "Metric", "Before", "After", "Change"
    );
    print_delta("Available", before.available, after.available);
    for (index, label) in ["Brave", "Chrome", "Opera", "ChatGPT/Codex"]
        .iter()
        .enumerate()
    {
        print_delta(
            label,
            tracked_bytes(before)[index],
            tracked_bytes(after)[index],
        );
    }
    print_delta("Monitor", before.own_bytes, after.own_bytes);
    println!("Working sets may include shared pages; background activity changes these figures.");
}

fn open_brave_settings() -> Result<(), String> {
    let mut candidates = Vec::<PathBuf>::new();
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(base) = env::var_os(key) {
            candidates.push(
                PathBuf::from(base).join("BraveSoftware/Brave-Browser/Application/brave.exe"),
            );
        }
    }
    if let Some(path) = candidates.into_iter().find(|path| path.is_file()) {
        Command::new(path)
            .arg("brave://settings/performance")
            .spawn()
            .map_err(|error| format!("Could not open Brave: {error}"))?;
        Ok(())
    } else {
        Err("Brave was not found in Program Files.".into())
    }
}

fn main() {
    let args: Vec<_> = env::args().collect();
    match args.get(1).map(String::as_str).unwrap_or("watch") {
        "snapshot" => {
            let mut system = System::new().expect("Cannot initialize system monitor");
            print_snapshot(&take_snapshot(&mut system));
        }
        "watch" => {
            let mut system = System::new().expect("Cannot initialize system monitor");
            loop {
                print!("\x1b[2J\x1b[H");
                print_snapshot(&take_snapshot(&mut system));
                thread::sleep(Duration::from_secs(5));
            }
        }
        "compare" => {
            let mut system = System::new().expect("Cannot initialize system monitor");
            println!("Taking baseline snapshot...");
            let before = take_snapshot(&mut system);
            println!("Apply one change, then press Enter to measure again.");
            let mut input = String::new();
            if let Err(error) = io::stdin().read_line(&mut input) {
                eprintln!("Could not read input: {error}");
                process::exit(1);
            }
            let after = take_snapshot(&mut system);
            compare_snapshots(&before, &after);
        }
        "brave-settings" => {
            if let Err(error) = open_brave_settings() {
                eprintln!("{error}");
                process::exit(1);
            }
        }
        _ => {
            eprintln!("Usage: memory-pilot [snapshot|watch|compare|brave-settings]");
            process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_only_matching_processes() {
        let groups = vec![
            ProcessGroup {
                name: "brave.exe".into(),
                count: 2,
                bytes: 100,
            },
            ProcessGroup {
                name: "ChatGPT.exe".into(),
                count: 1,
                bytes: 200,
            },
        ];
        assert_eq!(named_bytes(&groups, &["brave.exe"]), 100);
        assert_eq!(named_bytes(&groups, &["codex.exe"]), 0);
    }

    #[test]
    fn tracks_browsers_and_agents_separately() {
        let snapshot = Snapshot {
            total: 0,
            available: 0,
            groups: vec![
                ProcessGroup {
                    name: "brave.exe".into(),
                    count: 1,
                    bytes: 10,
                },
                ProcessGroup {
                    name: "chrome.exe".into(),
                    count: 1,
                    bytes: 20,
                },
                ProcessGroup {
                    name: "opera.exe".into(),
                    count: 1,
                    bytes: 30,
                },
                ProcessGroup {
                    name: "codex.exe".into(),
                    count: 1,
                    bytes: 40,
                },
            ],
            own_bytes: 0,
        };
        assert_eq!(tracked_bytes(&snapshot), [10, 20, 30, 40]);
    }
}
