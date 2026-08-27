use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "work_order_state", rename_all = "snake_case")]
pub enum WorkOrderState {
    Draft,
    Confirmed,
    Progress,
    ToClose,
    Done,
    Cancel,
}

impl std::fmt::Display for WorkOrderState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "draft"),
            Self::Confirmed => write!(f, "confirmed"),
            Self::Progress => write!(f, "progress"),
            Self::ToClose => write!(f, "to_close"),
            Self::Done => write!(f, "done"),
            Self::Cancel => write!(f, "cancel"),
        }
    }
}

impl FromStr for WorkOrderState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "draft" => Ok(Self::Draft),
            "confirmed" => Ok(Self::Confirmed),
            "progress" => Ok(Self::Progress),
            "to_close" => Ok(Self::ToClose),
            "done" => Ok(Self::Done),
            "cancel" => Ok(Self::Cancel),
            _ => Err(format!("Unknown WorkOrderState variant: {}", s)),
        }
    }
}

impl Default for WorkOrderState {
    fn default() -> Self {
        Self::Draft
    }
}
