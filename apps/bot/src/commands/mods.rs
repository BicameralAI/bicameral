use bicameral_mods::{validate_manifest, FixtureRunner};
use std::path::Path;

pub async fn validate(manifest_path: &Path) -> anyhow::Result<()> {
    let (manifest, errors) = bicameral_mods::validate::load_and_validate(manifest_path)?;

    println!("Validating mod: {}", manifest.name);

    if errors.is_empty() {
        println!("  ✓ Manifest is valid");
        println!("  Triggers: {}", manifest.triggers.len());
        println!("  Actions: {}", manifest.actions.len());
    } else {
        println!("  Validation errors:");
        for err in &errors {
            println!("    - {}", err);
        }
        anyhow::bail!(
            "Mod manifest validation failed with {} error(s)",
            errors.len()
        );
    }

    Ok(())
}

pub async fn run(manifest_path: &Path, fixture_path: &Path) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(manifest_path)?;
    let manifest: bicameral_mods::ModManifest = serde_yaml::from_str(&content)?;

    // Validate first
    let errors = validate_manifest(&manifest);
    if !errors.is_empty() {
        println!("Manifest validation failed:");
        for err in &errors {
            println!("  - {}", err);
        }
        anyhow::bail!("Cannot run mod with invalid manifest");
    }

    let result = FixtureRunner::run(&manifest, fixture_path)?;

    println!("Mod run: {}", result.mod_name);
    println!("  Fixture: {}", result.fixture_path);
    println!("  Triggered: {}", result.triggered);
    if result.triggered {
        println!("  Actions fired: {}", result.actions_fired.len());
        for action in &result.actions_fired {
            println!("    - {:?}", action.action_type);
        }
    }

    Ok(())
}
