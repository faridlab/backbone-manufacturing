use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "repair_line_type", rename_all = "snake_case")]
pub enum RepairLineType {
    Add,
    Remove,
    Recycle,
}

impl std::fmt::Display for RepairLineType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Add => write!(f, "add"),
            Self::Remove => write!(f, "remove"),
            Self::Recycle => write!(f, "recycle"),
        }
    }
}

impl FromStr for RepairLineType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "add" => Ok(Self::Add),
            "remove" => Ok(Self::Remove),
            "recycle" => Ok(Self::Recycle),
            _ => Err(format!("Unknown RepairLineType variant: {}", s)),
        }
    }
}

impl Default for RepairLineType {
    fn default() -> Self {
        Self::Add
    }
}
