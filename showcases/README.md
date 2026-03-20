# Frigg Showcases

This directory is the public example corpus for Frigg.

Each JSON file captures a small set of realistic questions for one repository and the kinds of paths a good answer should surface.

## Format

- one JSON file per repository
- filename format: `owner__repo.json`
- `repo_slug` keeps the canonical GitHub slug
- `cases` is a list of question and expectation pairs

Each case stays broad enough for public use:
- `question` reads like something a developer would really ask
- `expected_answer_shape` describes the acceptable answer style
- `expected_paths_any` lists paths where at least one hit is expected
- `expected_paths_should` lists stronger but non-blocking paths
- `expected_kinds` summarizes the surface types the answer should touch

## Example

[`astral-sh__ruff.json`](/Users/bnomei/Sites/frigg/showcases/astral-sh__ruff.json) is a representative example.
