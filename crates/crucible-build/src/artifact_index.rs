use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Artifact;

#[derive(Serialize, Deserialize)]
struct ArtifactIndex {
    version: u32,
    artifacts: Vec<Artifact>,
}

pub(crate) fn write(path: &Path, artifacts: &BTreeMap<String, Artifact>) -> Result<(), String> {
    let index = ArtifactIndex {
        version: 1,
        artifacts: artifacts.values().cloned().collect(),
    };
    let bytes = serde_json::to_vec_pretty(&index).map_err(|e| e.to_string())?;
    if std::fs::read(path).ok().as_deref() != Some(&bytes) {
        std::fs::create_dir_all(path.parent().expect("index parent")).map_err(|e| e.to_string())?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn load(path: &Path, name: &str) -> Result<Artifact, String> {
    crate::artifact::validate_name(name)?;
    let text = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "cannot read SDK registry {}: {e}; the producer must generate named SDK outputs",
            path.display()
        )
    })?;
    let index: ArtifactIndex =
        serde_json::from_str(&text).map_err(|e| format!("invalid SDK registry: {e}"))?;
    if index.version != 1 {
        return Err(format!(
            "unsupported SDK registry version {}",
            index.version
        ));
    }
    let artifact = index
        .artifacts
        .iter()
        .find(|artifact| artifact.name == name)
        .ok_or_else(|| {
            format!(
                "SDK artifact {name:?} was not generated; available outputs: {}",
                index
                    .artifacts
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    Ok(artifact.clone())
}

/// Export one named SDK from a Cargo producer's generated artifact index.
pub fn export_artifact(
    index: impl AsRef<Path>,
    name: &str,
    output: impl AsRef<Path>,
) -> Result<(), String> {
    load(index.as_ref(), name)?.export(output)
}
