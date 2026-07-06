use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;

use super::WorkOrderStatus;
use super::AuditMetadata;

/// Strongly-typed ID for WorkOrder
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkOrderId(pub Uuid);

impl WorkOrderId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for WorkOrderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for WorkOrderId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for WorkOrderId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<WorkOrderId> for Uuid {
    fn from(id: WorkOrderId) -> Self { id.0 }
}

impl AsRef<Uuid> for WorkOrderId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for WorkOrderId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WorkOrder {
    pub id: Uuid,
    pub company_id: Uuid,
    pub work_order_number: String,
    pub item_id: Uuid,
    pub bom_id: Uuid,
    pub quantity: Decimal,
    pub produced_qty: Decimal,
    pub status: WorkOrderStatus,
    pub raw_material_cost: Decimal,
    pub operating_cost: Decimal,
    pub wip_warehouse_id: Option<Uuid>,
    pub fg_warehouse_id: Option<Uuid>,
    pub wip_account_id: Option<Uuid>,
    pub fg_account_id: Option<Uuid>,
    pub raw_material_account_id: Option<Uuid>,
    pub conversion_cost_account_id: Option<Uuid>,
    pub planned_start_date: Option<DateTime<Utc>>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl WorkOrder {
    /// Create a builder for WorkOrder
    pub fn builder() -> WorkOrderBuilder {
        WorkOrderBuilder::default()
    }

    /// Create a new WorkOrder with required fields
    pub fn new(company_id: Uuid, work_order_number: String, item_id: Uuid, bom_id: Uuid, quantity: Decimal, produced_qty: Decimal, status: WorkOrderStatus, raw_material_cost: Decimal, operating_cost: Decimal) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            work_order_number,
            item_id,
            bom_id,
            quantity,
            produced_qty,
            status,
            raw_material_cost,
            operating_cost,
            wip_warehouse_id: None,
            fg_warehouse_id: None,
            wip_account_id: None,
            fg_account_id: None,
            raw_material_account_id: None,
            conversion_cost_account_id: None,
            planned_start_date: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> WorkOrderId {
        WorkOrderId(self.id)
    }

    /// Get when this entity was created
    pub fn created_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.created_at.as_ref()
    }

    /// Get when this entity was last updated
    pub fn updated_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.updated_at.as_ref()
    }

    /// Check if this entity is soft deleted
    pub fn is_deleted(&self) -> bool {
        self.metadata.deleted_at.is_some()
    }

    /// Check if this entity is active (not deleted)
    pub fn is_active(&self) -> bool {
        self.metadata.deleted_at.is_none()
    }

    /// Get when this entity was deleted
    pub fn deleted_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.deleted_at.as_ref()
    }

    /// Get who created this entity
    pub fn created_by(&self) -> Option<&Uuid> {
        self.metadata.created_by.as_ref()
    }

    /// Get who last updated this entity
    pub fn updated_by(&self) -> Option<&Uuid> {
        self.metadata.updated_by.as_ref()
    }

    /// Get who deleted this entity
    pub fn deleted_by(&self) -> Option<&Uuid> {
        self.metadata.deleted_by.as_ref()
    }

    /// Get the current status
    pub fn status(&self) -> &WorkOrderStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the wip_warehouse_id field (chainable)
    pub fn with_wip_warehouse_id(mut self, value: Uuid) -> Self {
        self.wip_warehouse_id = Some(value);
        self
    }

    /// Set the fg_warehouse_id field (chainable)
    pub fn with_fg_warehouse_id(mut self, value: Uuid) -> Self {
        self.fg_warehouse_id = Some(value);
        self
    }

    /// Set the wip_account_id field (chainable)
    pub fn with_wip_account_id(mut self, value: Uuid) -> Self {
        self.wip_account_id = Some(value);
        self
    }

    /// Set the fg_account_id field (chainable)
    pub fn with_fg_account_id(mut self, value: Uuid) -> Self {
        self.fg_account_id = Some(value);
        self
    }

    /// Set the raw_material_account_id field (chainable)
    pub fn with_raw_material_account_id(mut self, value: Uuid) -> Self {
        self.raw_material_account_id = Some(value);
        self
    }

    /// Set the conversion_cost_account_id field (chainable)
    pub fn with_conversion_cost_account_id(mut self, value: Uuid) -> Self {
        self.conversion_cost_account_id = Some(value);
        self
    }

    /// Set the planned_start_date field (chainable)
    pub fn with_planned_start_date(mut self, value: DateTime<Utc>) -> Self {
        self.planned_start_date = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "work_order_number" => {
                    if let Ok(v) = serde_json::from_value(value) { self.work_order_number = v; }
                }
                "item_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.item_id = v; }
                }
                "bom_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.bom_id = v; }
                }
                "quantity" => {
                    if let Ok(v) = serde_json::from_value(value) { self.quantity = v; }
                }
                "produced_qty" => {
                    if let Ok(v) = serde_json::from_value(value) { self.produced_qty = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "raw_material_cost" => {
                    if let Ok(v) = serde_json::from_value(value) { self.raw_material_cost = v; }
                }
                "operating_cost" => {
                    if let Ok(v) = serde_json::from_value(value) { self.operating_cost = v; }
                }
                "wip_warehouse_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.wip_warehouse_id = v; }
                }
                "fg_warehouse_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.fg_warehouse_id = v; }
                }
                "wip_account_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.wip_account_id = v; }
                }
                "fg_account_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.fg_account_id = v; }
                }
                "raw_material_account_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.raw_material_account_id = v; }
                }
                "conversion_cost_account_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.conversion_cost_account_id = v; }
                }
                "planned_start_date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.planned_start_date = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for WorkOrder {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "WorkOrder"
    }
}

impl backbone_core::PersistentEntity for WorkOrder {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
    fn set_entity_id(&mut self, id: String) {
        if let Ok(uuid) = uuid::Uuid::parse_str(&id) {
            self.id = uuid;
        }
    }
    fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.created_at
    }
    fn set_created_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.created_at = Some(ts);
    }
    fn updated_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.updated_at
    }
    fn set_updated_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.updated_at = Some(ts);
    }
    fn deleted_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.deleted_at
    }
    fn set_deleted_at(&mut self, ts: Option<chrono::DateTime<chrono::Utc>>) {
        self.metadata.deleted_at = ts;
    }
}

impl backbone_orm::EntityRepoMeta for WorkOrder {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("item_id".to_string(), "uuid".to_string());
        m.insert("bom_id".to_string(), "uuid".to_string());
        m.insert("wip_warehouse_id".to_string(), "uuid".to_string());
        m.insert("fg_warehouse_id".to_string(), "uuid".to_string());
        m.insert("wip_account_id".to_string(), "uuid".to_string());
        m.insert("fg_account_id".to_string(), "uuid".to_string());
        m.insert("raw_material_account_id".to_string(), "uuid".to_string());
        m.insert("conversion_cost_account_id".to_string(), "uuid".to_string());
        m.insert("status".to_string(), "work_order_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["work_order_number"]
    }
}

/// Builder for WorkOrder entity
///
/// Provides a fluent API for constructing WorkOrder instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct WorkOrderBuilder {
    company_id: Option<Uuid>,
    work_order_number: Option<String>,
    item_id: Option<Uuid>,
    bom_id: Option<Uuid>,
    quantity: Option<Decimal>,
    produced_qty: Option<Decimal>,
    status: Option<WorkOrderStatus>,
    raw_material_cost: Option<Decimal>,
    operating_cost: Option<Decimal>,
    wip_warehouse_id: Option<Uuid>,
    fg_warehouse_id: Option<Uuid>,
    wip_account_id: Option<Uuid>,
    fg_account_id: Option<Uuid>,
    raw_material_account_id: Option<Uuid>,
    conversion_cost_account_id: Option<Uuid>,
    planned_start_date: Option<DateTime<Utc>>,
}

impl WorkOrderBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the work_order_number field (required)
    pub fn work_order_number(mut self, value: String) -> Self {
        self.work_order_number = Some(value);
        self
    }

    /// Set the item_id field (required)
    pub fn item_id(mut self, value: Uuid) -> Self {
        self.item_id = Some(value);
        self
    }

    /// Set the bom_id field (required)
    pub fn bom_id(mut self, value: Uuid) -> Self {
        self.bom_id = Some(value);
        self
    }

    /// Set the quantity field (required)
    pub fn quantity(mut self, value: Decimal) -> Self {
        self.quantity = Some(value);
        self
    }

    /// Set the produced_qty field (default: `Decimal::from(0)`)
    pub fn produced_qty(mut self, value: Decimal) -> Self {
        self.produced_qty = Some(value);
        self
    }

    /// Set the status field (default: `WorkOrderStatus::default()`)
    pub fn status(mut self, value: WorkOrderStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the raw_material_cost field (default: `Decimal::from(0)`)
    pub fn raw_material_cost(mut self, value: Decimal) -> Self {
        self.raw_material_cost = Some(value);
        self
    }

    /// Set the operating_cost field (default: `Decimal::from(0)`)
    pub fn operating_cost(mut self, value: Decimal) -> Self {
        self.operating_cost = Some(value);
        self
    }

    /// Set the wip_warehouse_id field (optional)
    pub fn wip_warehouse_id(mut self, value: Uuid) -> Self {
        self.wip_warehouse_id = Some(value);
        self
    }

    /// Set the fg_warehouse_id field (optional)
    pub fn fg_warehouse_id(mut self, value: Uuid) -> Self {
        self.fg_warehouse_id = Some(value);
        self
    }

    /// Set the wip_account_id field (optional)
    pub fn wip_account_id(mut self, value: Uuid) -> Self {
        self.wip_account_id = Some(value);
        self
    }

    /// Set the fg_account_id field (optional)
    pub fn fg_account_id(mut self, value: Uuid) -> Self {
        self.fg_account_id = Some(value);
        self
    }

    /// Set the raw_material_account_id field (optional)
    pub fn raw_material_account_id(mut self, value: Uuid) -> Self {
        self.raw_material_account_id = Some(value);
        self
    }

    /// Set the conversion_cost_account_id field (optional)
    pub fn conversion_cost_account_id(mut self, value: Uuid) -> Self {
        self.conversion_cost_account_id = Some(value);
        self
    }

    /// Set the planned_start_date field (optional)
    pub fn planned_start_date(mut self, value: DateTime<Utc>) -> Self {
        self.planned_start_date = Some(value);
        self
    }

    /// Build the WorkOrder entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<WorkOrder, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let work_order_number = self.work_order_number.ok_or_else(|| "work_order_number is required".to_string())?;
        let item_id = self.item_id.ok_or_else(|| "item_id is required".to_string())?;
        let bom_id = self.bom_id.ok_or_else(|| "bom_id is required".to_string())?;
        let quantity = self.quantity.ok_or_else(|| "quantity is required".to_string())?;

        Ok(WorkOrder {
            id: Uuid::new_v4(),
            company_id,
            work_order_number,
            item_id,
            bom_id,
            quantity,
            produced_qty: self.produced_qty.unwrap_or(Decimal::from(0)),
            status: self.status.unwrap_or(WorkOrderStatus::default()),
            raw_material_cost: self.raw_material_cost.unwrap_or(Decimal::from(0)),
            operating_cost: self.operating_cost.unwrap_or(Decimal::from(0)),
            wip_warehouse_id: self.wip_warehouse_id,
            fg_warehouse_id: self.fg_warehouse_id,
            wip_account_id: self.wip_account_id,
            fg_account_id: self.fg_account_id,
            raw_material_account_id: self.raw_material_account_id,
            conversion_cost_account_id: self.conversion_cost_account_id,
            planned_start_date: self.planned_start_date,
            metadata: AuditMetadata::default(),
        })
    }
}
