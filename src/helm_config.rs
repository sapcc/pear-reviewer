// Copyright 2024 SAP SE
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

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
    // TODO: add test
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
