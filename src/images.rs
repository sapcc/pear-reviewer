use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[allow(clippy::module_name_repetitions)]
#[derive(Debug, Serialize, Deserialize)]
pub struct ContainerImages {
    #[serde(rename = "containerImages")]
    pub container_images: HashMap<String, Image>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Image {
    pub account: String,
    pub repository: String,
    pub tag: String,
    pub sources: Vec<Sources>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Sources {
    pub repo: String,
    pub commit: String,
}
