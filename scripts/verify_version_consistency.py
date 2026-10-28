#!/usr/bin/env python3
"""
Vanta Version Consistency & Release Gate Verifier

Validates that:
1. All workspace member crates share the exact same version string.
2. The version adheres strictly to Vanta release policy (1.0.0-rc.N or 1.0.0).
3. If running on a Git tag (or provided via CLI), the tag strictly corresponds
   to the Cargo workspace version.
4. Outputs consistent archive and checksum identifiers for CI pipelines.
"""

import argparse
import json
import os
import re
import subprocess
import sys

RC_PATTERN = re.compile(r"^1\.0\.0-rc\.(\d+)$")
STABLE_PATTERN = re.compile(r"^1\.0\.0$")

def get_workspace_metadata(repo_root):
    cmd = ["cargo", "metadata", "--no-deps", "--format-version", "1"]
    res = subprocess.run(cmd, cwd=repo_root, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if res.returncode != 0:
        print(f"Error: 'cargo metadata' failed:\n{res.stderr}", file=sys.stderr)
        sys.exit(1)
    return json.loads(res.stdout)

def semver_to_release_tag(version):
    rc_match = RC_PATTERN.match(version)
    if rc_match:
        rc_num = rc_match.group(1)
        return f"1.0-RC{rc_num}"
    if STABLE_PATTERN.match(version):
        return "1.0.0"
    return None

def validate_version_consistency(repo_root, expected_tag=None, override_packages=None):
    if override_packages is not None:
        packages = override_packages
    else:
        metadata = get_workspace_metadata(repo_root)
        packages = metadata.get("packages", [])

    if not packages:
        print("Error: No packages found in workspace metadata.", file=sys.stderr)
        return False, None

    # Verify that vanta_driver exists
    driver_pkg = next((p for p in packages if p["name"] == "vanta_driver"), None)
    if not driver_pkg:
        print("Error: Package 'vanta_driver' not found in workspace packages.", file=sys.stderr)
        return False, None

    driver_version = driver_pkg["version"]

    # Verify all member crates have identical versions
    mismatches = []
    for pkg in packages:
        if pkg["version"] != driver_version:
            mismatches.append(f"  - {pkg['name']}: {pkg['version']} (expected {driver_version})")

    if mismatches:
        print(f"Error: Workspace version mismatch detected! Target version is {driver_version}:", file=sys.stderr)
        for m in mismatches:
            print(m, file=sys.stderr)
        return False, None

    # Validate version against release policy
    release_tag = semver_to_release_tag(driver_version)
    if not release_tag:
        print(f"Error: Version '{driver_version}' does not conform to allowed release policy.", file=sys.stderr)
        print("Allowed formats are Release Candidates ('1.0.0-rc.N') or Stable GA ('1.0.0').", file=sys.stderr)
        return False, None

    # Verify tag consistency if a tag is specified
    if expected_tag:
        # Strip refs/tags/ if full ref was passed
        tag_name = expected_tag.replace("refs/tags/", "").strip()
        expected_git_tag = f"v{release_tag}"
        # For stable, accept either v1.0.0 or v1.0
        valid_tags = [expected_git_tag]
        if release_tag == "1.0.0":
            valid_tags.append("v1.0")

        if tag_name not in valid_tags:
            print(f"Error: Git tag '{tag_name}' does not match workspace version '{driver_version}'.", file=sys.stderr)
            print(f"Expected Git tag: {expected_git_tag}", file=sys.stderr)
            return False, None

    target_triple = "x86_64-unknown-linux-gnu"
    archive_name = f"vanta-v{release_tag}-{target_triple}"
    tarball = f"{archive_name}.tar.gz"
    checksum_file = f"{tarball}.sha256"

    info = {
        "version": driver_version,
        "release_tag": release_tag,
        "git_tag": f"v{release_tag}",
        "target": target_triple,
        "archive_name": archive_name,
        "tarball": tarball,
        "checksum_file": checksum_file,
    }
    return True, info

def main():
    parser = argparse.ArgumentParser(description="Vanta Version Consistency Verifier")
    parser.add_argument("--repo-root", default=os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))
    parser.add_argument("--expected-tag", default=None)
    parser.add_argument("--github-env", action="store_true", help="Write environment variables to GITHUB_ENV")
    parser.add_argument("--github-output", action="store_true", help="Write step outputs to GITHUB_OUTPUT")

    args = parser.parse_args()

    # Detect git tag from GitHub Actions environment if not provided
    expected_tag = args.expected_tag
    if not expected_tag:
        ref_type = os.environ.get("GITHUB_REF_TYPE", "")
        ref = os.environ.get("GITHUB_REF", "")
        if ref_type == "tag" or ref.startswith("refs/tags/"):
            expected_tag = os.environ.get("GITHUB_REF_NAME", ref)

    print("=== Vanta Release Gate: Verifying Version Consistency ===")
    ok, info = validate_version_consistency(args.repo_root, expected_tag)
    if not ok:
        sys.exit(1)

    print(f"Cargo workspace version: {info['version']}")
    print(f"Mapped release tag:      {info['release_tag']}")
    print(f"Target distribution:     {info['target']}")
    print(f"Release archive:         {info['tarball']}")
    print(f"Checksum manifest:       {info['checksum_file']}")
    print("Version validation:      PASSED")

    # Write to GitHub Actions environments if requested
    if args.github_env and "GITHUB_ENV" in os.environ:
        with open(os.environ["GITHUB_ENV"], "a") as f:
            f.write(f"VANTA_VERSION={info['version']}\n")
            f.write(f"VANTA_RELEASE_TAG={info['release_tag']}\n")
            f.write(f"VANTA_ARCHIVE_NAME={info['archive_name']}\n")
            f.write(f"VANTA_TARBALL={info['tarball']}\n")
            f.write(f"VANTA_CHECKSUM_FILE={info['checksum_file']}\n")

    if args.github_output and "GITHUB_OUTPUT" in os.environ:
        with open(os.environ["GITHUB_OUTPUT"], "a") as f:
            f.write(f"version={info['version']}\n")
            f.write(f"release_tag={info['release_tag']}\n")
            f.write(f"archive_name={info['archive_name']}\n")
            f.write(f"tarball={info['tarball']}\n")
            f.write(f"checksum_file={info['checksum_file']}\n")

if __name__ == "__main__":
    main()
