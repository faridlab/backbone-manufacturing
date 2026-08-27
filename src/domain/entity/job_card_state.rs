use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "job_card_state", rename_all = "snake_case")]
pub enum JobCardState {
    Ready,
    Progress,
    Done,
    Cancel,
    Blocked,
}

impl std::fmt::Display for JobCardState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready => write!(f, "ready"),
            Self::Progress => write!(f, "progress"),
            Self::Done => write!(f, "done"),
            Self::Cancel => write!(f, "cancel"),
            Self::Blocked => write!(f, "blocked"),
        }
    }
}

impl FromStr for JobCardState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ready" => Ok(Self::Ready),
            "progress" => Ok(Self::Progress),
            "done" => Ok(Self::Done),
            "cancel" => Ok(Self::Cancel),
            "blocked" => Ok(Self::Blocked),
            _ => Err(format!("Unknown JobCardState variant: {}", s)),
        }
    }
}

impl Default for JobCardState {
    fn default() -> Self {
        Self::Ready
    }
}
