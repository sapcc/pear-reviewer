use std::collections::HashMap;

use anyhow::Context;
use git2::{DiffFile, Repository};
use serde::{Deserialize, Serialize};

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Serialize, Deserialize)]
pub struct ImageRefs {
    #[serde(rename = "containerImages")]
    pub container_images: HashMap<String, ImageRef>,
}

impl ImageRefs {
    pub fn parse(repo: &Repository, diff_file: &DiffFile) -> Result<Self, anyhow::Error> {
        let blob_id = diff_file.id();
        let blob = repo
            .find_blob(blob_id)
            .with_context(|| format!("cannot find Git blob {blob_id}"))?;
        serde_yml::from_slice(blob.content()).with_context(|| format!("cannot parse yaml file {:?}", diff_file.path()))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageRef {
    pub account: String,
    pub repository: String,
    pub tag: String,
    pub sources: Vec<SourceRepoRef>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SourceRepoRef {
    pub repo: String,
    pub commit: String,
}
