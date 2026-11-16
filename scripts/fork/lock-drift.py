#!/usr/bin/env python3
"""Fail when plus/Cargo.lock pins a registry package to a version upstream's lock does not.

Usage: lock-drift.py <upstream Cargo.lock> <plus Cargo.lock>

A package that exists only in the plus lock (a fork-only dependency) is fine.
"""
import sys
import tomllib


def registry_versions(path):
    with open(path, "rb") as handle:
        packages = tomllib.load(handle).get("package", [])
    versions = {}
    for package in packages:
        if "source" in package:
            versions.setdefault(package["name"], set()).add(package["version"])
    return versions


def main():
    upstream, plus = (registry_versions(path) for path in sys.argv[1:3])
    drift = sorted(
        (name, sorted(found - upstream[name]), sorted(upstream[name]))
        for name, found in plus.items()
        if name in upstream and not found <= upstream[name]
    )
    fork_only = sorted(set(plus) - set(upstream))
    for name, extra, pinned in drift:
        print(f"drift: {name} {', '.join(extra)} in plus/Cargo.lock, upstream pins {', '.join(pinned)}")
    print(f"{len(plus)} registry packages in plus/Cargo.lock, {len(fork_only)} fork-only")
    return 1 if drift else 0


if __name__ == "__main__":
    sys.exit(main())
