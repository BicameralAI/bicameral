//! Fixture runner for testing mods against sample inputs.

use crate::manifest::{ModAction, ModManifest, TriggerEvent};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Result of running a mod against a fixture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureResult {
    pub mod_name: String,
    pub fixture_path: String,
    pub triggered: bool,
    pub actions_fired: Vec<ModAction>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Fixture runner for mods.
pub struct FixtureRunner;

impl FixtureRunner {
    /// Run a mod manifest against a fixture file.
    ///
    /// The fixture file is a JSON object representing an event. The runner
    /// checks whether any trigger matches, and if so, returns the actions
    /// that would fire.
    pub fn run(manifest: &ModManifest, fixture_path: &Path) -> anyhow::Result<FixtureResult> {
        let content = std::fs::read_to_string(fixture_path)?;
        let fixture: serde_json::Value = serde_json::from_str(&content)?;

        let event_type = fixture
            .get("event_type")
            .and_then(|v| v.as_str())
            .and_then(|s| {
                serde_json::from_value::<TriggerEvent>(serde_json::Value::String(s.to_string()))
                    .ok()
            });

        let mut triggered = false;
        let mut actions_fired = Vec::new();

        for trigger in &manifest.triggers {
            let type_matches = match &event_type {
                Some(et) => *et == trigger.event_type,
                None => false,
            };

            if type_matches && filters_match(&trigger.filters, &fixture) {
                triggered = true;
                actions_fired.extend(manifest.actions.clone());
                break;
            }
        }

        Ok(FixtureResult {
            mod_name: manifest.name.clone(),
            fixture_path: fixture_path.display().to_string(),
            triggered,
            actions_fired,
            warnings: Vec::new(),
        })
    }
}

fn filters_match(filters: &[crate::manifest::TriggerFilter], fixture: &serde_json::Value) -> bool {
    for filter in filters {
        let value = json_path_get(fixture, &filter.field);
        match value {
            None => return false,
            Some(v) => {
                use crate::manifest::FilterOperator;
                let matches = match filter.operator {
                    FilterOperator::Equals => v == &filter.value,
                    FilterOperator::Contains => {
                        if let (Some(haystack), Some(needle)) = (v.as_str(), filter.value.as_str())
                        {
                            haystack.contains(needle)
                        } else {
                            false
                        }
                    }
                    FilterOperator::Matches => {
                        // Simple substring match for v0.1
                        if let (Some(haystack), Some(needle)) = (v.as_str(), filter.value.as_str())
                        {
                            haystack.contains(needle)
                        } else {
                            false
                        }
                    }
                    FilterOperator::GreaterThan => match (v.as_f64(), filter.value.as_f64()) {
                        (Some(a), Some(b)) => a > b,
                        _ => false,
                    },
                    FilterOperator::LessThan => match (v.as_f64(), filter.value.as_f64()) {
                        (Some(a), Some(b)) => a < b,
                        _ => false,
                    },
                };
                if !matches {
                    return false;
                }
            }
        }
    }
    true
}

fn json_path_get<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;
    for part in parts {
        current = current.get(part)?;
    }
    Some(current)
}
