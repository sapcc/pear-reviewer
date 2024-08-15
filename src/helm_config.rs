use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Serialize, Deserialize)]
pub struct ImageRefs {
    #[serde(rename = "containerImages")]
    pub container_images: HashMap<String, ImageRef>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageRef {
    pub account:    String,
    pub repository: String,
    pub tag:        String,
    pub sources:    Vec<SourceRepoRef>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SourceRepoRef {
    pub repo:   String,
    pub commit: String,
}
