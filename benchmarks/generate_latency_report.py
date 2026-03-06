#!/usr/bin/env python3
"""Generate deterministic benchmark budget reports from Criterion sample.json files."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def percentile(sorted_values: list[float], p: float) -> float:
    if not sorted_values:
        raise ValueError("cannot compute percentile for empty value list")
    if len(sorted_values) == 1:
        return sorted_values[0]
    rank = (len(sorted_values) - 1) * p
    lower = int(rank)
    upper = min(lower + 1, len(sorted_values) - 1)
    fraction = rank - lower
    return sorted_values[lower] * (1.0 - fraction) + sorted_values[upper] * fraction


def parse_sample_file(sample_path: Path) -> list[float]:
    raw = json.loads(sample_path.read_text())
    iters = raw.get("iters")
    times = raw.get("times")
    if not isinstance(iters, list) or not isinstance(times, list) or len(iters) != len(times):
        raise ValueError(f"invalid Criterion sample format: {sample_path}")

    per_iter_ms: list[float] = []
    for iteration_count, total_ns in zip(iters, times, strict=True):
        if not isinstance(iteration_count, (int, float)) or float(iteration_count) <= 0.0:
            continue
        if not isinstance(total_ns, (int, float)):
            continue
        per_iter_ms.append((float(total_ns) / float(iteration_count)) / 1_000_000.0)

    if not per_iter_ms:
        raise ValueError(f"no valid samples in {sample_path}")

    per_iter_ms.sort()
    return per_iter_ms


def compute_quantiles_ms(sample_path: Path) -> dict[str, float]:
    samples = parse_sample_file(sample_path)
    return {
        "p50_ms": percentile(samples, 0.50),
        "p95_ms": percentile(samples, 0.95),
        "p99_ms": percentile(samples, 0.99),
    }


def evaluate_status(metrics: dict[str, float], budget: dict[str, float]) -> str:
    if (
        metrics["p50_ms"] <= budget["p50_ms"]
        and metrics["p95_ms"] <= budget["p95_ms"]
        and metrics["p99_ms"] <= budget["p99_ms"]
    ):
        return "pass"
    return "fail"


def load_budgets(path: Path) -> dict[str, Any]:
    payload = json.loads(path.read_text())
    workloads = payload.get("workloads")
    if not isinstance(workloads, list):
        raise ValueError(f"invalid workloads list in {path}")
    return payload


def discover_criterion_roots(primary_root: Path) -> list[Path]:
    # Keep a deterministic root ordering independent of local filesystem state so
    # release-gate byte-diff checks do not flap on missing/extra build artifacts.
    roots = [
        primary_root,
        Path("crates/cli/target/criterion"),
    ]

    # Preserve order while removing duplicates.
    unique_roots: list[Path] = []
    seen: set[str] = set()
    for root in roots:
        marker = root.as_posix()
        if marker in seen:
            continue
        seen.add(marker)
        unique_roots.append(root)
    return unique_roots


def locate_sample_file(criterion_roots: list[Path], workload_id: str) -> Path | None:
    for root in criterion_roots:
        candidate = root / workload_id / "new" / "sample.json"
        if candidate.is_file():
            return candidate
    return None


def markdown_report(
    report_rows: list[dict[str, Any]],
    criterion_roots: list[Path],
    budget_file: Path,
    pass_count: int,
    fail_count: int,
    missing_count: int,
) -> str:
    lines = [
        "# Benchmark Latency Report",
        "",
        "- report_version: `v1`",
        f"- criterion_roots: `{', '.join(str(root) for root in criterion_roots)}`",
        f"- budget_file: `{budget_file}`",
        f"- summary: pass={pass_count} fail={fail_count} missing={missing_count}",
        "",
        "| workload | status | p50 (ms) | p95 (ms) | p99 (ms) | budget p50/p95/p99 (ms) |",
        "| --- | --- | ---: | ---: | ---: | --- |",
    ]

    for row in report_rows:
        if row["status"] == "missing":
            lines.append(
                f"| `{row['id']}` | missing | - | - | - | "
                f"{row['budget']['p50_ms']:.2f}/{row['budget']['p95_ms']:.2f}/{row['budget']['p99_ms']:.2f} |"
            )
            continue

        lines.append(
            f"| `{row['id']}` | {row['status']} | "
            f"{row['metrics']['p50_ms']:.3f} | {row['metrics']['p95_ms']:.3f} | {row['metrics']['p99_ms']:.3f} | "
            f"{row['budget']['p50_ms']:.2f}/{row['budget']['p95_ms']:.2f}/{row['budget']['p99_ms']:.2f} |"
        )

    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--criterion-root",
        default="target/criterion",
        help="Criterion output root directory (default: target/criterion)",
    )
    parser.add_argument(
        "--budgets",
        default="benchmarks/budgets.v1.json",
        help="Path to benchmark budget JSON (default: benchmarks/budgets.v1.json)",
    )
    parser.add_argument(
        "--output",
        default="benchmarks/latest-report.md",
        help="Markdown report output path (default: benchmarks/latest-report.md)",
    )
    parser.add_argument(
        "--fail-on-budget",
        action="store_true",
        help="Exit non-zero if any workload fails budget or is missing",
    )
    args = parser.parse_args()

    criterion_root = Path(args.criterion_root)
    budget_file = Path(args.budgets)
    output_path = Path(args.output)
    criterion_roots = discover_criterion_roots(criterion_root)

    budget_payload = load_budgets(budget_file)
    workload_entries = sorted(
        budget_payload["workloads"],
        key=lambda workload: str(workload["id"]),
    )

    report_rows: list[dict[str, Any]] = []
    pass_count = 0
    fail_count = 0
    missing_count = 0

    print("benchmark_report_version=v1")
    print(f"criterion_root={criterion_root}")
    print(f"criterion_roots={','.join(str(root) for root in criterion_roots)}")
    print(f"budget_file={budget_file}")

    for workload in workload_entries:
        workload_id = str(workload["id"])
        budget = workload["budget"]
        sample_path = locate_sample_file(criterion_roots, workload_id)
        if sample_path is None:
            print(f"workload={workload_id} status=missing sample={sample_path}")
            missing_count += 1
            report_rows.append(
                {
                    "id": workload_id,
                    "status": "missing",
                    "budget": budget,
                }
            )
            continue

        metrics = compute_quantiles_ms(sample_path)
        status = evaluate_status(metrics, budget)
        if status == "pass":
            pass_count += 1
        else:
            fail_count += 1

        print(
            " ".join(
                [
                    f"workload={workload_id}",
                    f"status={status}",
                    f"p50_ms={metrics['p50_ms']:.3f}",
                    f"p95_ms={metrics['p95_ms']:.3f}",
                    f"p99_ms={metrics['p99_ms']:.3f}",
                    f"budget_p50_ms={budget['p50_ms']:.2f}",
                    f"budget_p95_ms={budget['p95_ms']:.2f}",
                    f"budget_p99_ms={budget['p99_ms']:.2f}",
                ]
            )
        )
        report_rows.append(
            {
                "id": workload_id,
                "status": status,
                "metrics": metrics,
                "budget": budget,
            }
        )

    print(f"summary pass={pass_count} fail={fail_count} missing={missing_count}")

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(
        markdown_report(
            report_rows,
            criterion_roots,
            budget_file,
            pass_count,
            fail_count,
            missing_count,
        )
    )
    print(f"report_path={output_path}")

    if args.fail_on_budget and (fail_count > 0 or missing_count > 0):
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
