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

#[derive(Clone, Debug)]
pub struct Commit {
    pub html_url: String,
    pub message: String,
    pub sha: String,
}

#[derive(Clone, Debug)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct Review {
    pub approved: bool,
    pub commit_id: String,
    pub submitted_at: i64,
    pub user: String,
}
