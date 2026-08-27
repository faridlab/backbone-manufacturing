//! Workcenter: productivity recording + the on-demand OEE report (hand-authored, user-owned).
//!
//! An `impl ManufacturingWriteService` chunk over the vocabulary in
//! [`super::manufacturing_write_service`]. Productivity rows are the OEE ledger: each row books a
//! stretch of time on a workstation against a named loss reason (classified productive /
//! availability / performance / quality by the reason's `loss_type`).
//!
//! Duration is READ-SIDE ONLY — `date_end − date_start` at query time; no stored duration column
//! exists to drift, and NO cron ever aggregates anything (the module runs zero scheduled jobs).
//! The report is computed on demand over whatever window the caller asks for, with the window
//! CLAMPING each row's contribution (a stretch that began before the window counts only the part
//! inside it; an open stretch counts up to now).
//!
//! Per the module's 4-layer rule this file holds no SQL — the statements live on
//! [`WorkcenterRepository`].

use rust_decimal::{Decimal, prelude::FromPrimitive};

use uuid::Uuid;

use crate::infrastructure::persistence::{NewWorkstationLossRow, NewWorkstationProductivityRow};

use super::manufacturing_write_service::{
    ManufacturingError, ManufacturingWriteService, NewProductivity, NewWorkstationLoss, OeeReport,
};

impl ManufacturingWriteService {
    /// Author a loss reason. `company_id: None` authors it as SHARED master data (visible to every
    /// company — the shared_blank fence, ADR-0014); a company id keeps it tenant-owned.
    /// A duplicate name is a LOUD refusal, never an overwrite.
    pub async fn create_workstation_loss(&self, l: NewWorkstationLoss) -> Result<Uuid, ManufacturingError> {
        if l.name.trim().is_empty() {
            return Err(ManufacturingError::Invalid("loss name must not be empty".into()));
        }
        let id = Uuid::new_v4();
        // RLS scope (ADR-0008): for shared master data (company None) the insert must ride an
        // UNFENCED (system) scope — a company-scoped session cannot write the shared row.
        let r = match l.company_id {
            Some(company) => {
                backbone_orm::company_scope::with_company_scope(
                    Some(company),
                    self.workcenter.insert_loss(&self.pool, &NewWorkstationLossRow {
                        id,
                        company_id: Some(company),
                        name: &l.name,
                        loss_type: l.loss_type,
                    }),
                )
                .await
            }
            None => {
                self.workcenter
                    .insert_loss(&self.pool, &NewWorkstationLossRow {
                        id,
                        company_id: None,
                        name: &l.name,
                        loss_type: l.loss_type,
                    })
                    .await
            }
        };
        if let Err(e) = r {
            return Err(if super::manufacturing_write_service::is_dup(&e) {
                ManufacturingError::Invalid(format!("loss reason '{}' already exists", l.name))
            } else {
                e.into()
            });
        }
        Ok(id)
    }

    /// Book a stretch of workstation time against a named loss reason. `date_end` may be None —
    /// an open stretch (a station still down) counts up to NOW() on the read side.
    ///
    /// The loss reason must already exist (company-owned or shared); an unknown name is LOUD —
    /// reasons are master data with a classification, not free text.
    pub async fn record_productivity(&self, p: NewProductivity) -> Result<Uuid, ManufacturingError> {
        if let Some(end) = p.date_end {
            if end <= p.date_start {
                return Err(ManufacturingError::Invalid("productivity stretch must end after it starts".into()));
            }
        }
        // RLS scope (ADR-0008): the company is on the DTO — scope the lookup + insert.
        let loss_id = backbone_orm::company_scope::with_company_scope(
            Some(p.company_id),
            self.workcenter.find_loss_by_name(&self.pool, p.company_id, &p.loss_name),
        )
        .await?
        .ok_or_else(|| ManufacturingError::Invalid(format!("unknown loss reason '{}'", p.loss_name)))?;

        let id = Uuid::new_v4();
        backbone_orm::company_scope::with_company_scope(
            Some(p.company_id),
            self.workcenter.insert_productivity(&self.pool, &NewWorkstationProductivityRow {
                id,
                company_id: p.company_id,
                workstation_id: p.workstation_id,
                job_card_id: p.job_card_id,
                loss_id,
                date_start: p.date_start,
                date_end: p.date_end,
                description: p.description,
            }),
        )
        .await?;
        Ok(id)
    }

    /// The on-demand OEE report for one workstation over [from, to]. Every component is a RATIO
    /// in [0, 1]: `(total − respective losses) / total`, with `oee = availability × performance ×
    /// quality`. Pure read — no stored aggregate, no cron, nothing written.
    ///
    /// A window with ZERO booked seconds reports all ratios as 0 with `total_seconds = 0`: nothing
    /// was measured, and reporting a vacuous perfect score would overstate the station.
    pub async fn workstation_oee(
        &self,
        company_id: Uuid,
        workstation_id: Uuid,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
    ) -> Result<OeeReport, ManufacturingError> {
        if to <= from {
            return Err(ManufacturingError::Invalid("OEE window must end after it starts".into()));
        }
        let buckets = backbone_orm::company_scope::with_company_scope(
            Some(company_id),
            self.workcenter.oee_buckets(&self.pool, company_id, workstation_id, from, to),
        )
        .await?;

        let mut productive = 0.0_f64;
        let mut availability = 0.0_f64;
        let mut performance = 0.0_f64;
        let mut quality = 0.0_f64;
        for b in &buckets {
            match b.loss_type.as_str() {
                "productive" => productive += b.seconds,
                "availability" => availability += b.seconds,
                "performance" => performance += b.seconds,
                "quality" => quality += b.seconds,
                _ => {}
            }
        }
        let total = productive + availability + performance + quality;
        let ratio = |loss: f64| -> Decimal {
            if total <= 0.0 {
                Decimal::ZERO
            } else {
                Decimal::from_f64((total - loss) / total).unwrap_or(Decimal::ZERO)
            }
        };
        let a = ratio(availability);
        let p = ratio(performance);
        let q = ratio(quality);
        Ok(OeeReport {
            availability: a,
            performance: p,
            quality: q,
            oee: a * p * q,
            total_seconds: Decimal::from_f64(total).unwrap_or(Decimal::ZERO),
        })
    }
}
