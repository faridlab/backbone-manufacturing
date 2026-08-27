//! Zero crons / zero schedulers: the module declares NO scheduled jobs — every manufacturing
//! surface is verb-driven or read-side computed. The hooks manifest pins an EMPTY scheduled_jobs
//! map, no migration or source file references a cron, and no scheduler dependency exists.

use std::path::PathBuf;

fn manifest_dir(sub: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(sub)
}

/// ZC-1 — the hooks manifest declares `scheduled_jobs: {}` and carries no cron-shaped entry.
#[test]
fn zc1_hooks_manifest_has_no_scheduled_jobs() {
    let hooks = std::fs::read_to_string(manifest_dir("schema/hooks/manufacturing.hook.yaml"))
        .expect("hooks manifest exists");
    assert!(
        hooks.contains("scheduled_jobs"),
        "the manifest must declare the scheduled_jobs key explicitly"
    );
    assert!(
        hooks.contains("scheduled_jobs: {}"),
        "scheduled_jobs is pinned EMPTY"
    );
    for cron_shape in ["cron:", "* * *", "schedule:", "interval:"] {
        assert!(
            !hooks.contains(cron_shape),
            "no scheduler entry may exist (found '{cron_shape}')"
        );
    }
}

/// ZC-2 — no source file wires a cron/tokio-interval scheduler. (Prose comments SAY "no cron";
/// the needles below only match real wiring shapes, never prose.)
#[test]
fn zc2_no_scheduler_in_source() {
    fn walk(dir: &PathBuf, hits: &mut Vec<String>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                    if name != "target" {
                        walk(&p, hits);
                    }
                } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
                    if let Ok(src) = std::fs::read_to_string(&p) {
                        for needle in ["cron:", "cron_", "interval(", "JobScheduler", "crontab", "tokio_cron"] {
                            if src.contains(needle) {
                                hits.push(format!("{}: {}", p.display(), needle));
                            }
                        }
                    }
                }
            }
        }
    }
    let mut hits = Vec::new();
    walk(&manifest_dir("src"), &mut hits);
    assert!(hits.is_empty(), "scheduler wiring found: {hits:?}");
}

/// ZC-3 — the dependency graph carries no scheduler crate.
#[test]
fn zc3_no_scheduler_dependency() {
    let cargo = std::fs::read_to_string(manifest_dir("Cargo.toml")).expect("Cargo.toml exists");
    for dep in ["cron", "tokio-cron", "croner", "clokwerk", "job_scheduler"] {
        assert!(
            !cargo.contains(dep),
            "scheduler dependency '{dep}' must not exist"
        );
    }
}

/// ZC-4 — no migration registers a scheduled anything.
#[test]
fn zc4_no_scheduled_rows_in_migrations() {
    fn walk(dir: &PathBuf, hits: &mut Vec<String>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, hits);
                } else if p.extension().map(|x| x == "sql").unwrap_or(false) {
                    if let Ok(sql) = std::fs::read_to_string(&p) {
                        for needle in ["cron", "schedule"] {
                            if sql.to_lowercase().contains(needle) {
                                hits.push(format!("{}: {}", p.display(), needle));
                            }
                        }
                    }
                }
            }
        }
    }
    let mut hits = Vec::new();
    walk(&manifest_dir("migrations"), &mut hits);
    assert!(hits.is_empty(), "scheduler rows seeded in migrations: {hits:?}");
}
