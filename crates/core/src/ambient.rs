//! Named local mixer presets. The original single-mix setting remains readable.
use crate::database::Store;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Preset {
    pub name: String,
    pub layers: Vec<(String, f32)>,
}

fn validate(preset: &Preset) -> Result<()> {
    ensure!(
        !preset.name.trim().is_empty() && preset.name.len() <= 256,
        "Give the mix a name of at most 256 bytes"
    );
    ensure!(
        preset.layers.len() <= 32,
        "A mix supports at most 32 layers"
    );
    let mut ids = HashSet::new();
    for (id, gain) in &preset.layers {
        ensure!(
            !id.is_empty() && id.len() <= 128 && ids.insert(id),
            "The mix contains an invalid or duplicate layer"
        );
        ensure!(
            gain.is_finite() && (0.0..=1.0).contains(gain),
            "Layer volume must be between 0 and 100 percent"
        );
    }
    Ok(())
}

impl Store {
    pub fn ambient_presets(&self) -> Result<Vec<Preset>> {
        if let Some(text) = self.setting("ambient-presets")? {
            ensure!(text.len() <= 1024 * 1024, "Saved mixes exceed 1 MiB");
            let presets: Vec<Preset> = serde_json::from_str(&text)?;
            ensure!(presets.len() <= 64, "At most 64 mixes can be saved");
            let mut names = HashSet::new();
            for preset in &presets {
                validate(preset)?;
                ensure!(names.insert(&preset.name), "Duplicate saved mix name");
            }
            return Ok(presets);
        }
        let mut presets = Vec::new();
        if let Some(text) = self.setting("ambient-preset")? {
            ensure!(text.len() <= 1024 * 1024, "Saved mix exceeds 1 MiB");
            let preset = Preset {
                name: "Saved mix".into(),
                layers: serde_json::from_str(&text)?,
            };
            validate(&preset)?;
            presets.push(preset);
        }
        Ok(presets)
    }

    pub fn save_ambient_preset(&self, name: &str, layers: Vec<(String, f32)>) -> Result<()> {
        let preset = Preset {
            name: name.trim().to_owned(),
            layers,
        };
        validate(&preset)?;
        let mut presets = self.ambient_presets()?;
        if let Some(existing) = presets.iter_mut().find(|p| p.name == preset.name) {
            *existing = preset;
        } else {
            ensure!(presets.len() < 64, "At most 64 mixes can be saved");
            presets.push(preset);
        }
        self.set("ambient-presets", &serde_json::to_string(&presets)?)
    }

    pub fn delete_ambient_preset(&self, name: &str) -> Result<()> {
        let mut presets = self.ambient_presets()?;
        presets.retain(|p| p.name != name);
        // Persist an empty list too, so a deleted legacy mix cannot reappear.
        self.set("ambient-presets", &serde_json::to_string(&presets)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_mix_and_named_edits_survive_restart_without_resurrection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mixes.sqlite");
        let layers = vec![("white".into(), 0.25), ("brown".into(), 0.5)];
        {
            let db = Store::open(&path).unwrap();
            db.set("ambient-preset", &serde_json::to_string(&layers).unwrap())
                .unwrap();
            assert_eq!(db.ambient_presets().unwrap()[0].layers, layers);
            db.save_ambient_preset(" Focus ", layers.clone()).unwrap();
            db.save_ambient_preset("Focus", vec![("pink".into(), 0.75)])
                .unwrap();
            assert_eq!(db.ambient_presets().unwrap().len(), 2);
            assert_eq!(db.ambient_presets().unwrap()[1].layers[0].0, "pink");
            db.delete_ambient_preset("Saved mix").unwrap();
            db.delete_ambient_preset("Focus").unwrap();
        }
        assert!(
            Store::open(path)
                .unwrap()
                .ambient_presets()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn invalid_layers_do_not_replace_a_saved_mix() {
        let db = Store::memory().unwrap();
        let layers = vec![("white".into(), 0.25)];
        db.save_ambient_preset("Focus", layers.clone()).unwrap();
        for invalid in [
            vec![("white".into(), f32::NAN)],
            vec![("white".into(), f32::INFINITY)],
            vec![("white".into(), -0.1)],
            vec![("white".into(), 1.1)],
            vec![("white".into(), 0.1), ("white".into(), 0.2)],
        ] {
            assert!(db.save_ambient_preset("Focus", invalid).is_err());
            assert_eq!(db.ambient_presets().unwrap()[0].layers, layers);
        }
        assert!(db.save_ambient_preset(" ", layers).is_err());
    }
}
