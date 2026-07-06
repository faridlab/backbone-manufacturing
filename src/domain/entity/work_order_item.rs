use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;
use super::AuditMetadata;

/// Strongly-typed ID for WorkOrderItem
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkOrderItemId(pub Uuid);

impl WorkOrderItemId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for WorkOrderItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for WorkOrderItemId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for WorkOrderItemId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<WorkOrderItemId> for Uuid {
    fn from(id: WorkOrderItemId) -> Self { id.0 }
}

impl AsRef<Uuid> for WorkOrderItemId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for WorkOrderItemId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WorkOrderItem {
    pub id: Uuid,
    pub work_order_id: Uuid,
    pub item_id: Uuid,
    pub required_qty: Decimal,
    pub consumed_qty: Decimal,
    pub rate: Decimal,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl WorkOrderItem {
    /// Create a builder for WorkOrderItem
    pub fn builder() -> WorkOrderItemBuilder {
        WorkOrderItemBuilder::default()
    }

    /// Create a new WorkOrderItem with required fields
    pub fn new(work_order_id: Uuid, item_id: Uuid, required_qty: Decimal, consumed_qty: Decimal, rate: Decimal) -> Self {
        Self {
            id: Uuid::new_v4(),
            work_order_id,
            item_id,
            required_qty,
            consumed_qty,
            rate,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> WorkOrderItemId {
        WorkOrderItemId(self.id)
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


    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "work_order_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.work_order_id = v; }
                }
                "item_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.item_id = v; }
                }
                "required_qty" => {
                    if let Ok(v) = serde_json::from_value(value) { self.required_qty = v; }
                }
                "consumed_qty" => {
                    if let Ok(v) = serde_json::from_value(value) { self.consumed_qty = v; }
                }
                "rate" => {
                    if let Ok(v) = serde_json::from_value(value) { self.rate = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for WorkOrderItem {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "WorkOrderItem"
    }
}

impl backbone_core::PersistentEntity for WorkOrderItem {
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

impl backbone_orm::EntityRepoMeta for WorkOrderItem {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("work_order_id".to_string(), "uuid".to_string());
        m.insert("item_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
}

/// Builder for WorkOrderItem entity
///
/// Provides a fluent API for constructing WorkOrderItem instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct WorkOrderItemBuilder {
    work_order_id: Option<Uuid>,
    item_id: Option<Uuid>,
    required_qty: Option<Decimal>,
    consumed_qty: Option<Decimal>,
    rate: Option<Decimal>,
}

impl WorkOrderItemBuilder {
    /// Set the work_order_id field (required)
    pub fn work_order_id(mut self, value: Uuid) -> Self {
        self.work_order_id = Some(value);
        self
    }

    /// Set the item_id field (required)
    pub fn item_id(mut self, value: Uuid) -> Self {
        self.item_id = Some(value);
        self
    }

    /// Set the required_qty field (required)
    pub fn required_qty(mut self, value: Decimal) -> Self {
        self.required_qty = Some(value);
        self
    }

    /// Set the consumed_qty field (default: `Decimal::from(0)`)
    pub fn consumed_qty(mut self, value: Decimal) -> Self {
        self.consumed_qty = Some(value);
        self
    }

    /// Set the rate field (default: `Decimal::from(0)`)
    pub fn rate(mut self, value: Decimal) -> Self {
        self.rate = Some(value);
        self
    }

    /// Build the WorkOrderItem entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<WorkOrderItem, String> {
        let work_order_id = self.work_order_id.ok_or_else(|| "work_order_id is required".to_string())?;
        let item_id = self.item_id.ok_or_else(|| "item_id is required".to_string())?;
        let required_qty = self.required_qty.ok_or_else(|| "required_qty is required".to_string())?;

        Ok(WorkOrderItem {
            id: Uuid::new_v4(),
            work_order_id,
            item_id,
            required_qty,
            consumed_qty: self.consumed_qty.unwrap_or(Decimal::from(0)),
            rate: self.rate.unwrap_or(Decimal::from(0)),
            metadata: AuditMetadata::default(),
        })
    }
}
