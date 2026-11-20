"""Compare published recipe content to the excluded legacy file fingerprints."""
import csv
import hashlib
from collections import defaultdict
from pathlib import Path

root = Path(__file__).resolve().parent.parent
excluded = defaultdict(set)
with (root / "audit/source-inventory.tsv").open() as inventory:
    for row in csv.DictReader(inventory, delimiter="\t"):
        if "/skill_library/" in row["source"] and row["disposition"] == "exclude":
            excluded[int(row["bytes"])].add(row["sha256"])
if not excluded:
    raise ValueError("Missing excluded recipe inventory")

files = sorted((root / "community/skills").glob("*/*"))
bodies = [p for p in files if p.name == "SKILL.md"]
if not bodies:
    raise ValueError("Missing public recipe bodies")
for path in files:
    data = memoryview(path.read_bytes())
    for length, fingerprints in excluded.items():
        for offset in range(len(data) - length + 1):
            if hashlib.sha256(data[offset:offset + length]).hexdigest() in fingerprints:
                raise ValueError(f"Excluded file content in {path.relative_to(root)}")

print(f"{len(bodies)} recipe bodies, {len(files)} public package files, "
      f"{sum(map(len, excluded.values()))} excluded fingerprints, 0 verbatim matches")
