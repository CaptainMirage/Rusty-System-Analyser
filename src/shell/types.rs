use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize)]
pub struct CommandInfo {
    pub title: &'static str,
    pub description: &'static str,
}