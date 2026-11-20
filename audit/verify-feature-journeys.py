"""Match feature journey selectors against actual Vitest and Rust test reports."""
import argparse
from collections import Counter
import csv
import io
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
TABLE = ROOT / "audit/feature-journeys.tsv"
RESULT_LINE = re.compile(r"^test ([\w:]+) \.\.\. (ok|FAILED|ignored)\b", re.MULTILINE)


def read_results(vitest_paths, rust_paths):
    web = {}
    for path in vitest_paths:
        for suite in json.loads(path.read_text())["testResults"]:
            for case in suite["assertionResults"]:
                web[suite["name"], case["title"]] = case["status"]
    rust = {}
    for path in rust_paths:
        for name, status in RESULT_LINE.findall(path.read_text(errors="replace")):
            rust[name] = status
    return web, rust


def measured(row, web, rust):
    selector, source = row["selector"], row["source"]
    if row["evidence_kind"] == "vitest":
        states = {status for (name, title), status in web.items()
                  if (name == source or name.endswith("/" + source)) and title == selector}
        if states == {"passed"}:
            return "통과"
        return "실패" if "failed" in states else "실행 근거 없음"
    function = selector.split("::")[-1]
    declaration = r"\bfn\s+" + re.escape(function) + r"\s*\("
    if not re.search(declaration, (ROOT / source).read_text()):
        return "소스와 테스트 불일치"
    matches = [status for name, status in rust.items()
               if name == selector or name.endswith("::" + selector)]
    if len(matches) > 1:
        return "테스트 이름 중복"
    if matches == ["ok"]:
        return "통과"
    return "실패" if matches == ["FAILED"] else "실행 근거 없음"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vitest-json", action="append", default=[], type=Path)
    parser.add_argument("--rust-log", action="append", default=[], type=Path,
                        help="Run output in chronological order, latest result for each exact test wins")
    parser.add_argument("--write", action="store_true", help="Update only automated run statuses in the table")
    args = parser.parse_args()
    if not args.vitest_json or not args.rust_log:
        parser.error("Provide both Vitest and Rust execution reports")
    web, rust = read_results(args.vitest_json, args.rust_log)
    with TABLE.open(newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        fields = reader.fieldnames
        rows = list(reader)
    counts = Counter(row["feature_id"] for row in rows)
    catalogue = (ROOT / "docs/features.md").read_text()
    known_features = set(re.findall(r"^\|\s*(F\d{2,})\s*\|", catalogue, re.MULTILINE))
    if not known_features or set(counts) != known_features or any(not 3 <= n <= 10 for n in counts.values()):
        raise ValueError("Every catalog feature must have 3–10 scenarios")
    ids = set()
    problems = []
    for row in rows:
        scenario = row["scenario_id"]
        if scenario in ids or not scenario.startswith(row["feature_id"] + "-"):
            raise ValueError("Invalid or duplicate scenario ID " + scenario)
        ids.add(scenario)
        source = ROOT / row["source"]
        if not source.is_file():
            raise ValueError("Missing evidence source for " + scenario)
        if row["evidence_kind"] == "recorded":
            headings = re.findall(r"^#{1,6}\s+(.+)$", source.read_text(), re.MULTILINE)
            if row["selector"] not in headings:
                raise ValueError("Missing recorded evidence section for " + scenario)
        if row["evidence_kind"] in ("vitest", "rust"):
            status = measured(row, web, rust)
            if status != "통과" or (not args.write and status != row["run_status"]):
                problems.append((scenario, status))
            row["run_status"] = status
    if args.write:
        new = io.StringIO()
        writer = csv.DictWriter(new, fieldnames=fields, delimiter="\t", lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)
        TABLE.write_text(new.getvalue())
    print(f"{len(counts)} features, {len(rows)} scenarios", dict(Counter(r["run_status"] for r in rows)))
    for scenario, condition in problems:
        print(f"{scenario}  {condition}", file=sys.stderr)
    return int(bool(problems))


if __name__ == "__main__":
    raise SystemExit(main())
