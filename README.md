# Pear Reviewer

Program to simplify PCI double approval process across repositories

## Development

0. Install nix
1. Run `nix-shell`
2. Run `cargo build`
3. Run `cargo test`
4. Run `cargo deny check`

## GitHub Action usage

pear-reviewer can be used as a GitHub Actions workflow to comment the review template on PRs.

```yaml
name: pear-reviewer
on:
  pull_request:
    branches:
      - '*'

permissions:
  pull-requests: write

jobs:
  review:
    name: Review
    runs-on: ubuntu-latest
    steps:
      - uses: sapcc/pear-reviewer@main
```
