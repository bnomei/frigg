#!/usr/bin/env python3
"""Profile-aware parity checks for runtime tool surface vs schema/docs contracts."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

PROFILE_ORDER = ("core", "extended")
RUNTIME_EXTRA_TOOL = "intentional_runtime_extra_tool"

TOOLS_README_CORE_START = "<!-- tool-surface-profile:core:start -->"
TOOLS_README_CORE_END = "<!-- tool-surface-profile:core:end -->"
TOOLS_README_EXT_START = "<!-- tool-surface-profile:extended_only:start -->"
TOOLS_README_EXT_END = "<!-- tool-surface-profile:extended_only:end -->"

OVERVIEW_CORE_START = "<!-- tool-surface-profile:core:start -->"
OVERVIEW_CORE_END = "<!-- tool-surface-profile:core:end -->"
OVERVIEW_EXT_START = "<!-- tool-surface-profile:extended_only:start -->"
OVERVIEW_EXT_END = "<!-- tool-surface-profile:extended_only:end -->"


def fail(message: str) -> None:
    print(f"tool-surface-parity check failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError:
        fail(f"missing required file: {path}")
    except OSError as error:
        fail(f"failed to read {path}: {error}")


def parse_rust_string_array(path: Path, const_name: str) -> list[str]:
    source = read_text(path)
    pattern = re.compile(
        rf"{re.escape(const_name)}\s*:[^=]*=\s*\[(?P<body>.*?)\];",
        flags=re.DOTALL,
    )
    match = pattern.search(source)
    if not match:
        fail(f"unable to locate Rust constant `{const_name}` in {path}")
    values = re.findall(r'"([^"]+)"', match.group("body"))
    if not values:
        fail(f"constant `{const_name}` in {path} has no string values")
    return values


def parse_active_profile(path: Path) -> str:
    source = read_text(path)

    direct_match = re.search(
        r"active_runtime_tool_surface_profile\(\)\s*->\s*ToolSurfaceProfile\s*\{\s*ToolSurfaceProfile::([A-Za-z_]+)\s*\}",
        source,
        flags=re.DOTALL,
    )
    if direct_match:
        profile = direct_match.group(1).lower()
    else:
        env_default_match = re.search(
            r"fn\s+runtime_tool_surface_profile_from_env\([^)]*\)\s*->\s*ToolSurfaceProfile\s*\{(?P<body>.*?)\n\}",
            source,
            flags=re.DOTALL,
        )
        if not env_default_match:
            fail(f"unable to parse active runtime profile from {path}")
        default_arm = re.search(
            r"_\s*=>\s*ToolSurfaceProfile::([A-Za-z_]+)",
            env_default_match.group("body"),
        )
        if not default_arm:
            fail(f"unable to parse default runtime profile arm from {path}")
        profile = default_arm.group(1).lower()

    if profile not in PROFILE_ORDER:
        fail(f"unsupported active runtime profile `{profile}` in {path}")
    return profile


def extract_marked_section(path: Path, start_marker: str, end_marker: str) -> str:
    text = read_text(path)
    if text.count(start_marker) != 1:
        fail(f"{path} must include exactly one marker `{start_marker}`")
    if text.count(end_marker) != 1:
        fail(f"{path} must include exactly one marker `{end_marker}`")
    start = text.find(start_marker)
    end = text.find(end_marker, start + len(start_marker))
    if end < 0:
        fail(f"{path} marker `{end_marker}` appears before `{start_marker}`")
    return text[start + len(start_marker) : end]


def parse_marked_tool_names(path: Path, start_marker: str, end_marker: str) -> set[str]:
    section = extract_marked_section(path, start_marker, end_marker)
    names = {
        match.group(1)
        for line in section.splitlines()
        for match in [re.match(r'^\s*-\s*`([a-z0-9._-]+)`', line)]
        if match
    }
    if not names:
        fail(f"{path} section `{start_marker}`..`{end_marker}` has no tool bullets")
    return names


def parse_schema_tool_names(schema_dir: Path) -> set[str]:
    if not schema_dir.is_dir():
        fail(f"missing required directory: {schema_dir}")
    names = {
        path.name[: -len(".v1.schema.json")]
        for path in sorted(schema_dir.glob("*.v1.schema.json"))
    }
    if not names:
        fail(f"no schema files found in {schema_dir}")
    return names


def load_sources(repo_root: Path) -> tuple[
    dict[str, list[str]],
    str,
    set[str],
    set[str],
    dict[str, dict[str, set[str]]],
]:
    types_rs = repo_root / "crates/cli/src/mcp/types.rs"
    tool_surface_rs = repo_root / "crates/cli/src/mcp/tool_surface.rs"
    schema_dir = repo_root / "docs/contracts/tools/v1"
    tools_readme = schema_dir / "README.md"
    overview = repo_root / "docs/overview.md"

    public_names = parse_rust_string_array(types_rs, "PUBLIC_READ_ONLY_TOOL_NAMES")
    extended_only_names = parse_rust_string_array(tool_surface_rs, "EXTENDED_ONLY_TOOL_NAMES")
    public_set = set(public_names)
    extended_only_set = set(extended_only_names)
    if not extended_only_set.issubset(public_set):
        fail("EXTENDED_ONLY_TOOL_NAMES must be a subset of PUBLIC_READ_ONLY_TOOL_NAMES")

    manifests = {
        "core": sorted(public_set - extended_only_set),
        "extended": sorted(public_set),
    }
    active_profile = parse_active_profile(tool_surface_rs)
    runtime_tools = set(manifests[active_profile])
    schema_tools = parse_schema_tool_names(schema_dir)

    readme_core = parse_marked_tool_names(
        tools_readme,
        TOOLS_README_CORE_START,
        TOOLS_README_CORE_END,
    )
    readme_extended_only = parse_marked_tool_names(
        tools_readme,
        TOOLS_README_EXT_START,
        TOOLS_README_EXT_END,
    )
    overview_core = parse_marked_tool_names(
        overview,
        OVERVIEW_CORE_START,
        OVERVIEW_CORE_END,
    )
    overview_extended_only = parse_marked_tool_names(
        overview,
        OVERVIEW_EXT_START,
        OVERVIEW_EXT_END,
    )

    docs_sources = {
        "core": {
            "tools_readme_core": readme_core,
            "overview_core": overview_core,
        },
        "extended": {
            "tools_readme_extended": readme_core | readme_extended_only,
            "overview_extended": overview_core | overview_extended_only,
        },
    }

    return manifests, active_profile, runtime_tools, schema_tools, docs_sources


def apply_intentional_fail(
    mode: str | None,
    active_profile: str,
    manifests: dict[str, list[str]],
    runtime_tools: set[str],
    schema_tools: set[str],
    docs_sources: dict[str, dict[str, set[str]]],
) -> None:
    if mode is None:
        return

    active_expected = manifests[active_profile]
    if not active_expected:
        fail("active profile manifest is empty; cannot apply intentional fail mode")
    target = active_expected[0]

    if mode == "runtime_extra":
        runtime_tools.add(RUNTIME_EXTRA_TOOL)
        return
    if mode == "schema_missing":
        schema_tools.discard(target)
        return
    if mode == "docs_missing":
        source_name = sorted(docs_sources[active_profile].keys())[0]
        docs_sources[active_profile][source_name].discard(target)
        return

    fail(f"unsupported intentional fail mode: {mode}")


def build_profile_report(
    profile: str,
    active_profile: str,
    manifests: dict[str, list[str]],
    runtime_tools: set[str],
    schema_tools: set[str],
    docs_sources: dict[str, dict[str, set[str]]],
) -> dict[str, object]:
    expected = set(manifests[profile])
    extended_expected = set(manifests["extended"])

    if profile == active_profile:
        missing_in_runtime = sorted(expected - runtime_tools)
        unexpected_in_runtime = sorted(runtime_tools - expected)
        runtime_check = "active_profile"
    else:
        missing_in_runtime = []
        unexpected_in_runtime = []
        runtime_check = "not_active_profile"

    missing_in_schema = sorted(expected - schema_tools)
    unexpected_in_schema = sorted(schema_tools - extended_expected)

    source_reports: dict[str, dict[str, list[str]]] = {}
    missing_docs_union: set[str] = set()
    unexpected_docs_union: set[str] = set()
    for source_name in sorted(docs_sources[profile].keys()):
        claimed = docs_sources[profile][source_name]
        source_missing = sorted(expected - claimed)
        source_unexpected = sorted(claimed - expected)
        source_reports[source_name] = {
            "missing": source_missing,
            "unexpected": source_unexpected,
        }
        missing_docs_union.update(source_missing)
        unexpected_docs_union.update(source_unexpected)

    return {
        "profile": profile,
        "runtime_check": runtime_check,
        "missing_in_runtime": missing_in_runtime,
        "unexpected_in_runtime": unexpected_in_runtime,
        "missing_in_schema": missing_in_schema,
        "unexpected_in_schema": unexpected_in_schema,
        "missing_in_docs": sorted(missing_docs_union),
        "unexpected_in_docs": sorted(unexpected_docs_union),
        "docs_source_diffs": source_reports,
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Check profile-aware parity between runtime tool registration, schema files, "
            "and docs tool-surface sections."
        )
    )
    parser.add_argument(
        "--intentional-fail",
        choices=("runtime_extra", "schema_missing", "docs_missing"),
        default=None,
        help="Inject deterministic drift for validation of failure diagnostics.",
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parent.parent
    manifests, active_profile, runtime_tools, schema_tools, docs_sources = load_sources(repo_root)
    apply_intentional_fail(
        args.intentional_fail,
        active_profile,
        manifests,
        runtime_tools,
        schema_tools,
        docs_sources,
    )

    reports = [
        build_profile_report(
            profile=profile,
            active_profile=active_profile,
            manifests=manifests,
            runtime_tools=runtime_tools,
            schema_tools=schema_tools,
            docs_sources=docs_sources,
        )
        for profile in PROFILE_ORDER
    ]

    failing = any(
        report["missing_in_runtime"]
        or report["unexpected_in_runtime"]
        or report["missing_in_schema"]
        or report["unexpected_in_schema"]
        or report["missing_in_docs"]
        or report["unexpected_in_docs"]
        for report in reports
    )
    status = "fail" if failing else "pass"

    print(
        f"tool-surface-parity summary status={status} active_profile={active_profile} "
        f"profiles={','.join(PROFILE_ORDER)}"
    )
    for report in reports:
        print(
            "tool-surface-parity "
            f"profile={report['profile']} "
            f"missing_in_runtime={len(report['missing_in_runtime'])} "
            f"unexpected_in_runtime={len(report['unexpected_in_runtime'])} "
            f"missing_in_schema={len(report['missing_in_schema'])} "
            f"unexpected_in_schema={len(report['unexpected_in_schema'])} "
            f"missing_in_docs={len(report['missing_in_docs'])} "
            f"unexpected_in_docs={len(report['unexpected_in_docs'])}"
        )

    payload = {
        "active_profile": active_profile,
        "intentional_fail_mode": args.intentional_fail,
        "profiles": reports,
        "status": status,
    }
    print(json.dumps(payload, sort_keys=True))

    return 1 if failing else 0


if __name__ == "__main__":
    raise SystemExit(main())
