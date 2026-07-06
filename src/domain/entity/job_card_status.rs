use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "job_card_status", rename_all = "snake_case")]
pub enum JobCardStatus {
    Open,
    Completed,
}

impl std::fmt::Display for JobCardStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::Completed => write!(f, "completed"),
        }
    }
}

impl FromStr for JobCardStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "open" => Ok(Self::Open),
            "completed" => Ok(Self::Completed),
            _ => Err(format!("Unknown JobCardStatus variant: {}", s)),
        }
    }
}

impl Default for JobCardStatus {
    fn default() -> Self {
        Self::Open
    }
}
