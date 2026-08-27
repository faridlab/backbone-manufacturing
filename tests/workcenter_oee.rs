//! OEE read side: the four-bucket ratio math, window clamping at both edges, the open-stretch
//! (date_end NULL) rule, the zero-booked window, and the unknown-loss-reason refusal. Pure read —
//! no stored aggregate, no cron.

mod common;

use backbone_manufacturing::application::service::manufacturing_write_service::{
    ManufacturingError, ManufacturingWriteService, NewProductivity, NewWorkstationLoss,
};
use backbone_manufacturing::domain::entity::LossType;
use common::*;
use uuid::Uuid;

/// Seed a workstation + the four loss reasons; returns (svc, pool, company, workstation).
async fn station() -> (ManufacturingWriteService, sqlx::PgPool, Uuid, Uuid) {
    let pool = pool().await;
    let svc = ManufacturingWriteService::new(pool.clone());
    let company = Uuid::new_v4();
    let ws = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO manufacturing.workstations (id, company_id, workstation_name, hour_rate)
           VALUES ($1, $2, 'OEE-probe', 60)"#,
    )
    .bind(ws)
    .bind(company)
    .execute(&pool)
    .await
    .unwrap();
    for (name, lt) in [
        ("run", LossType::Productive),
        ("breakdown", LossType::Availability),
        ("slow-cycle", LossType::Performance),
        ("scrap-rework", LossType::Quality),
    ] {
        svc.create_workstation_loss(NewWorkstationLoss {
            company_id: Some(company),
            name: name.into(),
            loss_type: lt,
        })
        .await
        .unwrap();
    }
    (svc, pool, company, ws)
}

/// A fixture timestamp at `minute_of_day` on a FIXED anchor day: the UTC day BEFORE today,
/// captured once per process. Anchoring to a strictly-past day keeps every window and open
/// stretch behind NOW() no matter when in the day the suite runs (the read side clamps open
/// stretches with NOW()), and the once-per-process capture keeps all timestamps on one day even
/// if the run crosses UTC midnight.
fn at(minute_of_day: i64) -> chrono::DateTime<chrono::Utc> {
    static ANCHOR: std::sync::OnceLock<chrono::NaiveDate> = std::sync::OnceLock::new();
    let day = ANCHOR.get_or_init(|| chrono::Utc::now().date_naive() - chrono::Duration::days(1));
    day.and_hms_opt(0, 0, 0).unwrap().and_utc() + chrono::Duration::minutes(minute_of_day)
}

/// WOEE-1 — the bucket math: run 3,600 · breakdown 600 · slow 300 · scrap 300 of 4,800 total →
/// A = 0.875, P = 0.9375, Q = 0.9375, OEE = A×P×Q.
#[tokio::test]
async fn woee1_bucket_ratios() {
    let (svc, _pool, company, ws) = station().await;
    let base = 600; // 10:00 — every stretch below is offset from midnight for stability
    for (name, mins) in [("run", 60), ("breakdown", 10), ("slow-cycle", 5), ("scrap-rework", 5)] {
        svc.record_productivity(NewProductivity {
            company_id: company,
            workstation_id: ws,
            job_card_id: None,
            loss_name: name.into(),
            date_start: at(base),
            date_end: Some(at(base + mins)),
            description: None,
        })
        .await
        .unwrap();
    }
    let report = svc
        .workstation_oee(company, ws, at(0), at(1439))
        .await
        .unwrap();
    assert_eq!(report.total_seconds, dec("4800"));
    assert_eq!(report.availability, dec("0.875"), "(4800-600)/4800");
    assert_eq!(report.performance, dec("0.9375"), "(4800-300)/4800");
    assert_eq!(report.quality, dec("0.9375"));
    assert_eq!(report.oee, dec("0.875") * dec("0.9375") * dec("0.9375"));
}

/// WOEE-2 — window clamping: a stretch STRADDLING the window edge contributes only its inside
/// part. A 60-minute run centered on the window edge contributes 30 minutes to the window.
#[tokio::test]
async fn woee2_window_clamping() {
    let (svc, _pool, company, ws) = station().await;
    let from = 600;
    let to = 660; // a 60-minute window
    // Runs 12:29:30→13:30:30? Use whole minutes: 09:30→10:30 straddles 10:00 by 30 minutes inside.
    svc.record_productivity(NewProductivity {
        company_id: company,
        workstation_id: ws,
        job_card_id: None,
        loss_name: "run".into(),
        date_start: at(570), // 09:30
        date_end: Some(at(630)), // 10:30 — 30 min inside the window
        description: None,
    })
    .await
    .unwrap();
    let report = svc.workstation_oee(company, ws, at(from), at(to)).await.unwrap();
    assert_eq!(report.total_seconds, dec("1800"), "only the inside half counts");
    assert_eq!(report.availability, dec("1"), "no availability loss inside");
    assert_eq!(report.oee, dec("1"));
}

/// WOEE-3 — an OPEN stretch (date_end NULL, station still down) counts up to the window edge.
#[tokio::test]
async fn woee3_open_stretch_counts_to_edge() {
    let (svc, _pool, company, ws) = station().await;
    let from = 700;
    let to = 710; // 10 minutes
    svc.record_productivity(NewProductivity {
        company_id: company,
        workstation_id: ws,
        job_card_id: None,
        loss_name: "breakdown".into(),
        date_start: at(695), // 09:55 — 5 min before the window opens
        date_end: None,      // still down
        description: None,
    })
    .await
    .unwrap();
    let report = svc.workstation_oee(company, ws, at(from), at(to)).await.unwrap();
    assert_eq!(report.total_seconds, dec("600"), "clamped to [from, to]");
    assert_eq!(report.availability, dec("0"), "the whole window was breakdown");
}

/// WOEE-4 — a window with ZERO booked seconds reports all ratios 0 (a vacuous 1.0 would
/// overstate an unmeasured station), and an inverted window is refused.
#[tokio::test]
async fn woee4_zero_booked_and_inverted_window() {
    let (svc, _pool, company, ws) = station().await;
    let report = svc.workstation_oee(company, ws, at(0), at(30)).await.unwrap();
    assert_eq!(report.total_seconds, dec("0"));
    assert_eq!(report.availability, dec("0"));
    assert_eq!(report.performance, dec("0"));
    assert_eq!(report.quality, dec("0"));
    assert_eq!(report.oee, dec("0"));
    let err = svc.workstation_oee(company, ws, at(30), at(0)).await.unwrap_err();
    assert!(matches!(err, ManufacturingError::Invalid(_)), "inverted window refused");
}

/// WOEE-5 — loss reasons are master data: an unknown name is refused LOUDLY, and a shared
/// (company-NULL) reason resolves for a company session.
#[tokio::test]
async fn woee5_loss_reasons_are_master_data() {
    let (svc, pool, company, ws) = station().await;
    let err = svc
        .record_productivity(NewProductivity {
            company_id: company,
            workstation_id: ws,
            job_card_id: None,
            loss_name: "no-such-reason".into(),
            date_start: at(600),
            date_end: Some(at(610)),
            description: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ManufacturingError::Invalid(_)), "unknown loss reason is LOUD");
    // A shared reason (NULL company, seeded directly) resolves for the company's bookings. The
    // seed is idempotent: the shared (NULL, name) slot survives across runs under the
    // NULLS NOT DISTINCT unique, so a rerun must not collide on it.
    sqlx::query(
        r#"INSERT INTO manufacturing.workstation_losses (id, company_id, name, loss_type)
           SELECT $1, NULL, 'planned-shared', 'availability'::loss_type
           WHERE NOT EXISTS (
               SELECT 1 FROM manufacturing.workstation_losses
                WHERE company_id IS NULL AND name = 'planned-shared'
           )"#,
    )
    .bind(Uuid::new_v4())
    .execute(&pool)
    .await
    .unwrap();
    svc.record_productivity(NewProductivity {
        company_id: company,
        workstation_id: ws,
        job_card_id: None,
        loss_name: "planned-shared".into(),
        date_start: at(600),
        date_end: Some(at(615)),
        description: None,
    })
    .await
    .expect("shared master data resolves");
}
