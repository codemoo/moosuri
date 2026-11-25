"""Build an owner-installable Chrome extension bound to one configured vault origin."""
import argparse
import json
import re
from pathlib import Path
from urllib.parse import urlsplit
from zipfile import ZIP_DEFLATED, ZipFile


def package(origin, destination):
    site = urlsplit(origin)
    if (site.scheme != "https" or not site.hostname or origin != "https://" + site.hostname
            or "." not in site.hostname or not all(re.fullmatch(r"[a-z0-9](?:[a-z0-9-]*[a-z0-9])?", label)
                                                     for label in site.hostname.split("."))):
        raise ValueError("Use the vault's exact HTTPS origin with its default port.")
    root = Path(__file__).resolve().parent
    manifest = json.loads((root / "manifest.json.in").read_text())
    manifest["externally_connectable"]["matches"] = [origin + "/*"]
    with ZipFile(destination, "x", ZIP_DEFLATED) as archive:
        archive.writestr("manifest.json", json.dumps(manifest, ensure_ascii=False, indent=2))
        archive.write(root / "worker.js", "worker.js")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("vault_origin")
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    package(args.vault_origin, args.output)
