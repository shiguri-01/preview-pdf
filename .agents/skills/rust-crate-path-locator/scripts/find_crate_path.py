#!/usr/bin/env python3
import argparse
import json
import os
import pathlib
import re
import subprocess
import sys
from typing import Any


def run_cargo_metadata(project_path: str) -> dict[str, Any]:
    manifest_path = pathlib.Path(project_path).resolve() / "Cargo.toml"
    if not manifest_path.exists():
        raise FileNotFoundError(f"Cargo.toml not found under ProjectPath: {project_path}")

    proc = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--manifest-path", str(manifest_path)],
        capture_output=True,
        text=False,
        check=False,
    )
    if proc.returncode != 0 or not proc.stdout:
        raise RuntimeError(f"Failed to run cargo metadata for {manifest_path}")
    stdout_text = proc.stdout.decode("utf-8", errors="replace")
    return json.loads(stdout_text)


def metadata_matches(crate: str, metadata: dict[str, Any]) -> list[dict[str, Any]]:
    matches: list[dict[str, Any]] = []
    for pkg in metadata.get("packages", []):
        if pkg.get("name") != crate:
            continue
        manifest_path = pkg.get("manifest_path")
        root_dir = str(pathlib.Path(manifest_path).parent) if manifest_path else None
        matches.append(
            {
                "match_type": "metadata",
                "name": pkg.get("name"),
                "version": pkg.get("version"),
                "source": pkg.get("source"),
                "manifest_path": manifest_path,
                "root_dir": root_dir,
                "docs_rs": f"https://docs.rs/{pkg.get('name')}/{pkg.get('version')}",
            }
        )
    return matches


def candidate_cargo_homes() -> list[pathlib.Path]:
    candidates: list[pathlib.Path] = []
    cargo_home = os.environ.get("CARGO_HOME")
    if cargo_home:
        candidates.append(pathlib.Path(cargo_home))

    home = os.environ.get("HOME") or os.environ.get("USERPROFILE")
    if home:
        candidates.append(pathlib.Path(home) / ".cargo")

    # Scoop fallback for Windows.
    cargo_path = shutil_which("cargo")
    if cargo_path:
        cargo_path_obj = pathlib.Path(cargo_path)
        parts = [p.lower() for p in cargo_path_obj.parts]
        if "scoop" in parts and "shims" in parts:
            try:
                scoop_idx = parts.index("scoop")
                scoop_root = pathlib.Path(*cargo_path_obj.parts[: scoop_idx + 1])
                candidates.append(scoop_root / "persist" / "rustup" / ".cargo")
            except Exception:
                pass

    unique: list[pathlib.Path] = []
    seen: set[str] = set()
    for p in candidates:
        norm = str(p.resolve()) if p.exists() else str(p)
        if norm in seen:
            continue
        seen.add(norm)
        if p.exists():
            unique.append(p)
    return unique


def shutil_which(cmd: str) -> str | None:
    for dir_path in os.environ.get("PATH", "").split(os.pathsep):
        candidate = pathlib.Path(dir_path) / cmd
        if candidate.exists():
            return str(candidate)
        if os.name == "nt":
            for ext in (".exe", ".cmd", ".bat"):
                c2 = pathlib.Path(dir_path) / f"{cmd}{ext}"
                if c2.exists():
                    return str(c2)
    return None


def cargo_home_matches(crate: str) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    crate_name_re = re.compile(r'^\s*name\s*=\s*"([^"]+)"\s*$')

    for home in candidate_cargo_homes():
        registry_src = home / "registry" / "src"
        if registry_src.exists():
            for registry_dir in registry_src.iterdir():
                if not registry_dir.is_dir():
                    continue
                for crate_dir in registry_dir.glob(f"{crate}-*"):
                    if not crate_dir.is_dir():
                        continue
                    version = crate_dir.name[len(crate) + 1 :] if crate_dir.name.startswith(f"{crate}-") else None
                    results.append(
                        {
                            "match_type": "cargo-home-registry",
                            "name": crate,
                            "version": version,
                            "source": "registry-cache",
                            "manifest_path": str(crate_dir / "Cargo.toml"),
                            "root_dir": str(crate_dir),
                            "docs_rs": f"https://docs.rs/{crate}",
                        }
                    )

        git_checkouts = home / "git" / "checkouts"
        if git_checkouts.exists():
            for root, _, files in os.walk(git_checkouts):
                if "Cargo.toml" not in files:
                    continue
                manifest = pathlib.Path(root) / "Cargo.toml"
                try:
                    text = manifest.read_text(encoding="utf-8", errors="ignore")
                except OSError:
                    continue
                manifest_name = None
                for line in text.splitlines():
                    m = crate_name_re.match(line)
                    if m:
                        manifest_name = m.group(1)
                        break
                if manifest_name == crate:
                    results.append(
                        {
                            "match_type": "cargo-home-git",
                            "name": crate,
                            "version": None,
                            "source": "git-checkout",
                            "manifest_path": str(manifest),
                            "root_dir": str(manifest.parent),
                            "docs_rs": f"https://docs.rs/{crate}",
                        }
                    )
    return results


def main() -> int:
    parser = argparse.ArgumentParser(description="Find local Rust crate paths without hardcoding ~/.cargo.")
    parser.add_argument("--crate", required=True, help="Crate name")
    parser.add_argument("--project-path", default=".", help="Path that contains Cargo.toml")
    parser.add_argument(
        "--include-cargo-home-scan",
        action="store_true",
        help="Scan candidate cargo home locations when metadata has no match",
    )
    args = parser.parse_args()

    try:
        metadata = run_cargo_metadata(args.project_path)
    except Exception as exc:
        print(str(exc), file=sys.stderr)
        return 1

    matches = metadata_matches(args.crate, metadata)
    if not matches and args.include_cargo_home_scan:
        matches = cargo_home_matches(args.crate)

    if not matches:
        print(f"No local match for crate '{args.crate}'.")
        print(
            "Tip: add it as a dependency in a Cargo project and run 'cargo fetch', "
            "then retry with --include-cargo-home-scan."
        )
        return 2

    output: Any = matches[0] if len(matches) == 1 else matches
    print(json.dumps(output, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
