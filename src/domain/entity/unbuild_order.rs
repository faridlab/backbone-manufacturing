use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;

use super::UnbuildStatus;
use super::AuditMetadata;

/// Strongly-typed ID for UnbuildOrder
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnbuildOrderId(pub Uuid);

impl UnbuildOrderId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for UnbuildOrderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for UnbuildOrderId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for UnbuildOrderId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<UnbuildOrderId> for Uuid {
    fn from(id: UnbuildOrderId) -> Self { id.0 }
}

impl AsRef<Uuid> for UnbuildOrderId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for UnbuildOrderId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UnbuildOrder {
    pub id: Uuid,
    pub company_id: Uuid,
    pub unbuild_number: String,
    pub work_order_id: Uuid,
    pub item_id: Uuid,
    pub quantity: Decimal,
    pub status: UnbuildStatus,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl UnbuildOrder {
    /// Create a builder for UnbuildOrder
    pub fn builder() -> UnbuildOrderBuilder {
        <UnbuildOrderBuilder as Default>::default()
    }

    /// Create a new UnbuildOrder with required fields
    pub fn new(company_id: Uuid, unbuild_number: String, work_order_id: Uuid, item_id: Uuid, quantity: Decimal, status: UnbuildStatus) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            unbuild_number,
            work_order_id,
            item_id,
            quantity,
            status,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> UnbuildOrderId {
        UnbuildOrderId(self.id)
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
    pub fn status(&self) -> &UnbuildStatus {
        &self.status
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
                "unbuild_number" => {
                    if let Ok(v) = serde_json::from_value(value) { self.unbuild_number = v; }
                }
                "work_order_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.work_order_id = v; }
                }
                "item_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.item_id = v; }
                }
                "quantity" => {
                    if let Ok(v) = serde_json::from_value(value) { self.quantity = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for UnbuildOrder {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "UnbuildOrder"
    }
}

impl backbone_core::PersistentEntity for UnbuildOrder {
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

impl backbone_orm::EntityRepoMeta for UnbuildOrder {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("work_order_id".to_string(), "uuid".to_string());
        m.insert("item_id".to_string(), "uuid".to_string());
        m.insert("status".to_string(), "unbuild_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["unbuild_number"]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for UnbuildOrder entity
///
/// Provides a fluent API for constructing UnbuildOrder instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct UnbuildOrderBuilder {
    company_id: Option<Uuid>,
    unbuild_number: Option<String>,
    work_order_id: Option<Uuid>,
    item_id: Option<Uuid>,
    quantity: Option<Decimal>,
    status: Option<UnbuildStatus>,
}

impl UnbuildOrderBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the unbuild_number field (required)
    pub fn unbuild_number(mut self, value: String) -> Self {
        self.unbuild_number = Some(value);
        self
    }

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

    /// Set the quantity field (required)
    pub fn quantity(mut self, value: Decimal) -> Self {
        self.quantity = Some(value);
        self
    }

    /// Set the status field (default: `UnbuildStatus::default()`)
    pub fn status(mut self, value: UnbuildStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Build the UnbuildOrder entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<UnbuildOrder, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let unbuild_number = self.unbuild_number.ok_or_else(|| "unbuild_number is required".to_string())?;
        let work_order_id = self.work_order_id.ok_or_else(|| "work_order_id is required".to_string())?;
        let item_id = self.item_id.ok_or_else(|| "item_id is required".to_string())?;
        let quantity = self.quantity.ok_or_else(|| "quantity is required".to_string())?;

        Ok(UnbuildOrder {
            id: Uuid::new_v4(),
            company_id,
            unbuild_number,
            work_order_id,
            item_id,
            quantity,
            status: self.status.unwrap_or_default(),
            metadata: AuditMetadata::default(),
        })
    }
}
