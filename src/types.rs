use std::fmt::Display;

use chrono::{DateTime, Utc};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Intermediate type for questions from all sources
#[derive(Debug, Deserialize, Serialize)]
pub struct Question {
    pub source: QuestionSource,
    pub source_url: String,
    pub source_id: String,
    pub question: String,
    pub criteria: Option<String>,
    pub end_date: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum BinaryResolution {
    Yes,
    No,
    Percent(f64),
    Cancel,
}

#[derive(Debug, Deserialize, Serialize, Clone, ValueEnum, PartialEq)]
pub enum QuestionSource {
    Kalshi,
    Metaculus,
    Polymarket,
}

impl Display for QuestionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            QuestionSource::Kalshi => "Kalshi",
            QuestionSource::Metaculus => "Metaculus",
            QuestionSource::Polymarket => "Polymarket",
        })?;
        Ok(())
    }
}
