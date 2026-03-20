use std::fs;
use std::path::Path;

use crate::domain::{FriggError, FriggResult};

use super::{LoadedHybridPlaybookRegression, PlaybookDocument, parse_playbook_document};

pub fn load_playbook_document(path: &Path) -> FriggResult<PlaybookDocument> {
    let raw = fs::read_to_string(path).map_err(FriggError::Io)?;
    parse_playbook_document(&raw).map_err(|err| {
        FriggError::InvalidInput(format!(
            "failed to load playbook metadata from '{}': {err}",
            path.display()
        ))
    })
}

pub fn load_hybrid_playbook_regressions(
    playbooks_root: &Path,
) -> FriggResult<Vec<LoadedHybridPlaybookRegression>> {
    let mut paths = fs::read_dir(playbooks_root)
        .map_err(FriggError::Io)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("md")
                && path.file_name().and_then(|name| name.to_str()) != Some("README.md")
        })
        .collect::<Vec<_>>();
    paths.sort();

    let mut regressions = Vec::new();
    for path in paths {
        let document = load_playbook_document(&path)?;
        let spec = document.metadata.hybrid_regression.clone().ok_or_else(|| {
            FriggError::InvalidInput(format!(
                "playbook '{}' is missing hybrid_regression metadata",
                path.display()
            ))
        })?;
        regressions.push(LoadedHybridPlaybookRegression {
            path,
            metadata: document.metadata,
            spec,
        });
    }

    if regressions.is_empty() {
        return Err(FriggError::InvalidInput(format!(
            "no executable hybrid playbooks found under '{}'",
            playbooks_root.display()
        )));
    }

    Ok(regressions)
}
