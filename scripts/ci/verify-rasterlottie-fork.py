#!/usr/bin/env python3
"""Reconstruct Kael's rasterlottie fork from its pinned registry archive."""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import urllib.request


PACKAGE = "rasterlottie-0.2.1"
ARCHIVE_URL = f"https://static.crates.io/crates/rasterlottie/{PACKAGE}.crate"
ARCHIVE_SHA256 = "384686130a2b365f82258f5e444c512e57989addcf2b45e9913c4ef6f0e95a24"
ORIGINAL_FILES = 74
ORIGINAL_BYTES = 665_043
MAX_ARCHIVE_BYTES = 128 * 1024
MAX_PATCH_BYTES = 128 * 1024
MAX_FORK_BYTES = ORIGINAL_BYTES * 2
SELF_PROVENANCE = frozenset({
    "PATCHES.md", "UPSTREAM-SHA256.json", "zip9-compatibility.patch",
})
# Keep the reviewed implementation scope explicit. A matching reconstruction
# alone must not authorize changes to another upstream source or license file.
MODIFIED_RUST_ORIGINALS = frozenset({
    "src/dotlottie.rs",
    "src/lib.rs",
    "src/model.rs",
    "src/render/assets.rs",
    "src/render/mod.rs",
    "src/render/renderer/mod.rs",
    "src/render/renderer/prepare.rs",
    "src/render/renderer/raster.rs",
    "src/render/renderer_types.rs",
})
MODIFIED_ORIGINALS = MODIFIED_RUST_ORIGINALS | {"Cargo.toml", "Cargo.lock"}
ADDED_FILES = frozenset({
    "UPSTREAM-Cargo.lock", "UPSTREAM-Cargo.toml",
    "tests/zip9_compatibility.rs", "tests/fixtures/zip9_glyph_text.json",
    "tests/output_allocation.rs",
})
FORK_DIRECTORY = Path(__file__).resolve().parents[2] / "vendor/rasterlottie"


class VerificationError(Exception):
    """A provenance, extraction or reconstruction invariant failed."""


def require(condition, message):
    if not condition:
        raise VerificationError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def regular_file(path, description):
    require(not path.is_symlink(), f"{description} is a symlink: {path}")
    require(stat.S_ISREG(path.stat().st_mode), f"{description} is not a regular file: {path}")


def relative_parts(name):
    """Validate before joining paths; do not normalize away dot segments."""
    parts = name.split("/")
    require(parts and all(part not in ("", ".", "..") for part in parts),
            f"unsafe relative path: {name!r}")
    require(not any("\\" in part or ":" in part
                    or any(ord(character) < 32 for character in part)
                    for part in parts), f"unsafe path characters: {name!r}")
    return parts


def bounded_read(stream, limit, description):
    data = stream.read(limit + 1)
    require(len(data) <= limit, f"{description} exceeds {limit} bytes")
    return data


def read_archive(path):
    if path is not None:
        require(stat.S_ISREG(path.stat().st_mode), f"archive is not a regular file: {path}")
        require(path.stat().st_size <= MAX_ARCHIVE_BYTES,
                f"archive exceeds {MAX_ARCHIVE_BYTES} bytes: {path}")
        with path.open("rb") as source:
            return bounded_read(source, MAX_ARCHIVE_BYTES, "archive")
    request = urllib.request.Request(ARCHIVE_URL, headers={"Accept-Encoding": "identity"})
    print(f"Downloading pinned archive: {ARCHIVE_URL}", flush=True)
    try:
        with urllib.request.urlopen(request, timeout=30) as source:
            return bounded_read(source, MAX_ARCHIVE_BYTES, "archive download")
    except OSError as error:
        raise VerificationError(
            f"download failed: {error}; supply --archive for offline verification "
            "or configure Python's trusted CA bundle with SSL_CERT_FILE"
        ) from error


def read_hash_index(path):
    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, f"duplicate upstream hash entry: {key!r}")
            result[key] = value
        return result

    regular_file(path, "upstream hash index")
    with path.open("rb") as source:
        data = bounded_read(source, 32 * 1024, "upstream hash index")
    hashes = json.loads(data, object_pairs_hook=unique_object)
    require(isinstance(hashes, dict) and len(hashes) == ORIGINAL_FILES,
            f"upstream hash index must contain exactly {ORIGINAL_FILES} files")
    for name, value in hashes.items():
        relative_parts(name)
        require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value),
                f"invalid SHA-256 for upstream file: {name}")
    return hashes


def extract_original(data, destination, hashes):
    """Never use extractall: reject links/devices and write checked paths only."""
    actual = {}
    seen = set()
    total = 0
    with tarfile.open(fileobj=io.BytesIO(data), mode="r|gz") as archive:
        for member in archive:
            parts = relative_parts(member.name.removesuffix("/"))
            require(parts[0] == PACKAGE, f"archive entry is outside {PACKAGE}: {member.name!r}")
            require(member.name not in seen, f"duplicate archive entry: {member.name!r}")
            seen.add(member.name)
            require(len(seen) <= ORIGINAL_FILES * 2, "archive has too many entries")
            require(member.isdir() or member.isfile(),
                    f"archive contains a link or special file: {member.name!r}")
            if len(parts) == 1:
                require(member.isdir(), "archive package root is not a directory")
                continue
            name = "/".join(parts[1:])
            target = destination.joinpath(*parts[1:])
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            require(name in hashes, f"archive contains an unrecorded original file: {name}")
            require(name not in actual, f"duplicate original file: {name}")
            require(0 <= member.size <= ORIGINAL_BYTES - total,
                    f"archive exceeds {ORIGINAL_BYTES} original bytes at {name}")
            source = archive.extractfile(member)
            require(source is not None, f"cannot read original file: {name}")
            with source:
                content = bounded_read(source, member.size, f"archive entry {name}")
            require(len(content) == member.size, f"truncated archive entry: {name}")
            require(digest(content) == hashes[name], f"upstream hash mismatch: {name}")
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(content)
            actual[name] = hashes[name]
            total += member.size
    require(actual.keys() == hashes.keys(),
            f"archive/hash index file set differs; missing: {sorted(hashes.keys() - actual.keys())}")
    require(total == ORIGINAL_BYTES,
            f"original byte total differs: expected {ORIGINAL_BYTES}, got {total}")


def checked_patch(path):
    regular_file(path, "reconstruction patch")
    with path.open("rb") as source:
        content = bounded_read(source, MAX_PATCH_BYTES, "reconstruction patch")
    headers = 0
    for line in content.decode("utf-8").splitlines():
        # Only the documented unified patch format is accepted. Reject other
        # filename hints that patch could choose over the checked headers.
        require(not line.startswith(("Index:", "diff ", "*** ")),
                "patch must use unified ---/+++ file headers only")
        if line.startswith(("--- ", "+++ ")):
            name = line[4:]
            require("\t" not in name, f"patch header has an unexpected timestamp: {name!r}")
            if name != "/dev/null":
                prefix = "a/" if line.startswith("--- ") else "b/"
                require(name.startswith(prefix), f"unexpected patch path prefix: {name!r}")
                relative_parts(name[len(prefix):])
            headers += 1
    require(headers > 0 and headers % 2 == 0, "patch has no complete unified file headers")
    return content


def file_hashes(directory):
    result = {}
    total = 0
    for current, directories, files in os.walk(directory, followlinks=False):
        for name in directories + files:
            path = Path(current) / name
            require(not path.is_symlink(), f"fork contains a symlink: {path}")
        for name in files:
            path = Path(current) / name
            require(stat.S_ISREG(path.stat().st_mode), f"fork contains a special file: {path}")
            total += path.stat().st_size
            require(total <= MAX_FORK_BYTES,
                    f"fork exceeds {MAX_FORK_BYTES} bytes at {path}")
            hasher = hashlib.sha256()
            with path.open("rb") as source:
                for chunk in iter(lambda: source.read(64 * 1024), b""):
                    hasher.update(chunk)
            result[path.relative_to(directory).as_posix()] = hasher.hexdigest()
    return result


def verify(data, fork_directory):
    actual_digest = digest(data)
    require(actual_digest == ARCHIVE_SHA256,
            f"archive SHA-256 mismatch: expected {ARCHIVE_SHA256}, got {actual_digest}")
    hashes = read_hash_index(fork_directory / "UPSTREAM-SHA256.json")
    patch = checked_patch(fork_directory / "zip9-compatibility.patch")
    require(shutil.which("patch") is not None, "patch executable is required to reconstruct the fork")
    with tempfile.TemporaryDirectory(prefix="kael-rasterlottie-verify-") as temporary:
        staging = Path(temporary)
        reconstructed = staging / "source"
        reconstructed.mkdir()
        extract_original(data, reconstructed, hashes)
        patch_path = staging / "compatibility.patch"
        patch_path.write_bytes(patch)
        command = ["patch", "--batch", "--forward", "--fuzz=0", "-p1", "-E", "-i", str(patch_path)]
        completed = subprocess.run(command, cwd=reconstructed, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT, text=True, timeout=30,
                                   env={**os.environ, "LC_ALL": "C"})
        require(completed.returncode == 0,
                f"reconstruction patch failed:\n{completed.stdout.strip()}")
        shutil.copyfile(reconstructed / "Cargo.toml.orig", reconstructed / "UPSTREAM-Cargo.toml")
        expected = file_hashes(reconstructed)
        modified_originals = {
            name for name, original_hash in hashes.items()
            if expected.get(name) != original_hash
        }
        require(modified_originals == MODIFIED_ORIGINALS,
                "reconstructed original change set differs from reviewed scope; "
                f"missing: {sorted(MODIFIED_ORIGINALS - modified_originals)}; "
                f"unexpected: {sorted(modified_originals - MODIFIED_ORIGINALS)}")
        added_files = expected.keys() - hashes.keys()
        require(added_files == ADDED_FILES,
                "reconstructed additions differ from reviewed scope; "
                f"missing: {sorted(ADDED_FILES - added_files)}; "
                f"unexpected: {sorted(added_files - ADDED_FILES)}")
        actual = file_hashes(fork_directory)
        require(SELF_PROVENANCE <= actual.keys(), "fork is missing its self-provenance files")
        actual = {name: value for name, value in actual.items() if name not in SELF_PROVENANCE}
        require(actual.keys() == expected.keys(),
                f"reconstructed file set differs; missing: {sorted(expected.keys() - actual.keys())}; "
                f"unexpected: {sorted(actual.keys() - expected.keys())}")
        for name, expected_hash in expected.items():
            require(actual[name] == expected_hash, f"reconstructed file differs: {name}")
        # Only the declared admission/API implementation files, normalized
        # manifest and historical lock location may differ from the registry.
        # Original tests, fixtures, licenses and all other sources stay intact.
        for name, original_hash in hashes.items():
            if name not in MODIFIED_ORIGINALS:
                require(actual.get(name) == original_hash, f"original file was modified or removed: {name}")
        require(actual.get("UPSTREAM-Cargo.lock") == hashes["Cargo.lock"],
                "UPSTREAM-Cargo.lock does not preserve the original lockfile")
        require(actual.get("UPSTREAM-Cargo.toml") == hashes["Cargo.toml.orig"],
                "UPSTREAM-Cargo.toml does not preserve Cargo.toml.orig")
        return len(actual) + len(SELF_PROVENANCE)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path,
                        help="use the pinned .crate file locally instead of downloading it")
    arguments = parser.parse_args()
    try:
        count = verify(read_archive(arguments.archive), FORK_DIRECTORY)
    except (VerificationError, OSError, ValueError, tarfile.TarError,
            subprocess.TimeoutExpired) as error:
        print(f"RASTERLOTTIE_FORK_FAIL: {error}", file=sys.stderr)
        return 1
    print(f"RASTERLOTTIE_FORK_OK: archive_sha256={ARCHIVE_SHA256} "
          f"original_files={ORIGINAL_FILES} original_bytes={ORIGINAL_BYTES} "
          f"reconstructed_files={count} reviewed_rust_changes={len(MODIFIED_RUST_ORIGINALS)}; "
          "other original sources/tests/fixtures/licenses unchanged")
    return 0


if __name__ == "__main__":
    sys.exit(main())
