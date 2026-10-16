use std::path::PathBuf;

use crate::errors::AppError;

#[derive(Debug, Clone)]
pub struct MergeRequest {
    pub source_dir: PathBuf,
    pub output_dir: PathBuf,
    pub target_name: String,
    pub include_incomplete: bool,
}

#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    pub fraction: f32,
    pub stage: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ProgressSnapshot {
    pub fraction: f32,
    pub stage: String,
    pub logs: Vec<String>,
}

impl Default for ProgressSnapshot {
    fn default() -> Self {
        Self {
            fraction: 0.0,
            stage: "En attente".into(),
            logs: Vec::new(),
        }
    }
}

impl ProgressSnapshot {
    pub fn record(&mut self, update: ProgressUpdate) {
        self.fraction = update.fraction.clamp(0.0, 1.0);
        self.stage = update.stage;
        if !update.message.is_empty() && self.logs.last().is_none_or(|last| last != &update.message)
        {
            self.logs.push(update.message);
            if self.logs.len() > 200 {
                self.logs.drain(..50);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct MergeReport {
    pub output: PathBuf,
    pub segment_count: usize,
    pub partial: bool,
    pub xdm_version: String,
    pub detail: String,
}

pub type MergeResult = Result<MergeReport, AppError>;
