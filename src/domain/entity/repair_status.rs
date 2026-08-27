use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "repair_status", rename_all = "snake_case")]
pub enum RepairStatus {
    Draft,
    Confirmed,
    UnderRepair,
    Done,
    Cancel,
}

impl std::fmt::Display for RepairStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "draft"),
            Self::Confirmed => write!(f, "confirmed"),
            Self::UnderRepair => write!(f, "under_repair"),
            Self::Done => write!(f, "done"),
            Self::Cancel => write!(f, "cancel"),
        }
    }
}

impl FromStr for RepairStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "draft" => Ok(Self::Draft),
            "confirmed" => Ok(Self::Confirmed),
            "under_repair" => Ok(Self::UnderRepair),
            "done" => Ok(Self::Done),
            "cancel" => Ok(Self::Cancel),
            _ => Err(format!("Unknown RepairStatus variant: {}", s)),
        }
    }
}

impl Default for RepairStatus {
    fn default() -> Self {
        Self::Draft
    }
}
