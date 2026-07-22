use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;
use super::AuditMetadata;

/// Strongly-typed ID for BomItem
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BomItemId(pub Uuid);

impl BomItemId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for BomItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for BomItemId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for BomItemId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<BomItemId> for Uuid {
    fn from(id: BomItemId) -> Self { id.0 }
}

impl AsRef<Uuid> for BomItemId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for BomItemId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BomItem {
    pub id: Uuid,
    pub company_id: Uuid,
    pub bom_id: Uuid,
    pub item_id: Uuid,
    pub quantity: Decimal,
    pub rate: Decimal,
    pub amount: Decimal,
    pub is_phantom: bool,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl BomItem {
    /// Create a builder for BomItem
    pub fn builder() -> BomItemBuilder {
        BomItemBuilder::default()
    }

    /// Create a new BomItem with required fields
    pub fn new(company_id: Uuid, bom_id: Uuid, item_id: Uuid, quantity: Decimal, rate: Decimal, amount: Decimal, is_phantom: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            bom_id,
            item_id,
            quantity,
            rate,
            amount,
            is_phantom,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> BomItemId {
        BomItemId(self.id)
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
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "bom_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.bom_id = v; }
                }
                "item_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.item_id = v; }
                }
                "quantity" => {
                    if let Ok(v) = serde_json::from_value(value) { self.quantity = v; }
                }
                "rate" => {
                    if let Ok(v) = serde_json::from_value(value) { self.rate = v; }
                }
                "amount" => {
                    if let Ok(v) = serde_json::from_value(value) { self.amount = v; }
                }
                "is_phantom" => {
                    if let Ok(v) = serde_json::from_value(value) { self.is_phantom = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for BomItem {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "BomItem"
    }
}

impl backbone_core::PersistentEntity for BomItem {
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

impl backbone_orm::EntityRepoMeta for BomItem {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("bom_id".to_string(), "uuid".to_string());
        m.insert("item_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for BomItem entity
///
/// Provides a fluent API for constructing BomItem instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct BomItemBuilder {
    company_id: Option<Uuid>,
    bom_id: Option<Uuid>,
    item_id: Option<Uuid>,
    quantity: Option<Decimal>,
    rate: Option<Decimal>,
    amount: Option<Decimal>,
    is_phantom: Option<bool>,
}

impl BomItemBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the bom_id field (required)
    pub fn bom_id(mut self, value: Uuid) -> Self {
        self.bom_id = Some(value);
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

    /// Set the rate field (default: `Decimal::from(0)`)
    pub fn rate(mut self, value: Decimal) -> Self {
        self.rate = Some(value);
        self
    }

    /// Set the amount field (default: `Decimal::from(0)`)
    pub fn amount(mut self, value: Decimal) -> Self {
        self.amount = Some(value);
        self
    }

    /// Set the is_phantom field (default: `false`)
    pub fn is_phantom(mut self, value: bool) -> Self {
        self.is_phantom = Some(value);
        self
    }

    /// Build the BomItem entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<BomItem, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let bom_id = self.bom_id.ok_or_else(|| "bom_id is required".to_string())?;
        let item_id = self.item_id.ok_or_else(|| "item_id is required".to_string())?;
        let quantity = self.quantity.ok_or_else(|| "quantity is required".to_string())?;

        Ok(BomItem {
            id: Uuid::new_v4(),
            company_id,
            bom_id,
            item_id,
            quantity,
            rate: self.rate.unwrap_or(Decimal::from(0)),
            amount: self.amount.unwrap_or(Decimal::from(0)),
            is_phantom: self.is_phantom.unwrap_or(false),
            metadata: AuditMetadata::default(),
        })
    }
}
