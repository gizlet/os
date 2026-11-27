#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[2]
VERSION = r"(?:0|[1-9][0-9]{0,8})(?:\.(?:0|[1-9][0-9]{0,8})){2}"
BOARD = Path("boards/gameconsole/r36s")


def artifact(output, path):
    digest = hashlib.sha256()
    with (output / path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return {"path": path.as_posix(), "sha256": digest.hexdigest(), "bytes": (output / path).stat().st_size}


def assemble(tag, commit, build, output, signing_key):
    version = tag[1:]
    image = BOARD / "gizlet-os-r36s.img.gz"
    checksum = BOARD / "SHA256SUMS"
    runtime = Path("runtime/gizlet-runtime-1.sqfs")
    launcher = Path(f"launcher/gizlet-launcher-{version}.gizlet")
    board_scripts = {"macos_linux": BOARD / "flash.sh", "windows": BOARD / "flash.ps1"}
    for relative_path in (image, runtime, launcher):
        (output / relative_path).parent.mkdir(parents=True, exist_ok=True)
    for relative_path in (image, runtime):
        shutil.copyfile(build / relative_path.name, output / relative_path)
    for relative_path in board_scripts.values():
        shutil.copyfile(ROOT / relative_path, output / relative_path)
    for filename in ("flash.sh", "flash.ps1"):
        contents = (ROOT / filename).read_text().replace("@RELEASE_TAG@", tag, 1)
        encoding = "utf-8-sig" if filename.endswith(".ps1") else "utf-8"
        (output / filename).write_text(contents, encoding=encoding)
    windows_script = output / board_scripts["windows"]
    windows_script.write_text(windows_script.read_text(encoding="utf-8-sig"), encoding="utf-8-sig")
    (output / checksum).write_text(f"{artifact(output, image)['sha256']}  {image.name}\n")

    manifest = {
        "schema_version": 1,
        "tag": tag,
        "commit": commit,
        "boards": {
            "gameconsole": {
                "r36s": {
                    "image": {"path": image.as_posix(), "checksum": checksum.as_posix()},
                    "flash_scripts": {platform: path.as_posix() for platform, path in board_scripts.items()},
                },
            },
        },
        "runtime": {"arm64": runtime.as_posix()},
        "launcher": {"arm64": launcher.as_posix()},
    }
    with tempfile.TemporaryDirectory() as key_dir:
        key = Path(key_dir) / "launcher"
        key.touch(mode=0o600)
        key.write_text(signing_key.rstrip("\n") + "\n")
        public = subprocess.check_output(["ssh-keygen", "-y", "-f", key], text=True).split()[:2]
        trusted = (ROOT / "rootfs/usr/lib/gizlet/launcher/allowed_signers").read_text().split()[-2:]
        if public != trusted:
            raise ValueError("Launcher signing key does not match the public key built into the image")
        subprocess.run(
            [ROOT / "launcher/pack_release.sh", version, key, build / "gizlet-launcher",
             build / "gizlet-cli", output / launcher],
            check=True,
        )
        manifest["ota"] = {
            "protocol": 2,
            "arch": "aarch64",
            "runtime_abi": 1,
            "runtime": artifact(output, runtime),
            "launcher": artifact(output, launcher),
        }
        manifest_path = output / "manifest.json"
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
        subprocess.run(["ssh-keygen", "-Y", "sign", "-f", key, "-n", "gizlet-launcher", manifest_path], check=True)


def publish(tag, output, credentials):
    endpoint = credentials["R2_ENDPOINT"]
    bucket = credentials["R2_BUCKET"]
    if not re.fullmatch(r"https://[a-f0-9]{32}(?:\.(?:eu|fedramp))?\.r2\.cloudflarestorage\.com", endpoint):
        raise ValueError("Expected a Cloudflare R2 S3 endpoint")
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{1,61}[a-z0-9]", bucket):
        raise ValueError("Invalid R2 bucket name")
    environment = dict(
        os.environ,
        AWS_ACCESS_KEY_ID=credentials["R2_ACCESS_KEY_ID"],
        AWS_SECRET_ACCESS_KEY=credentials["R2_SECRET_ACCESS_KEY"],
        AWS_DEFAULT_REGION="auto",
        AWS_EC2_METADATA_DISABLED="true",
        AWS_REQUEST_CHECKSUM_CALCULATION="when_required",
    )
    for name in ("AWS_SESSION_TOKEN", "AWS_PROFILE", "SOPS_AGE_KEY"):
        environment.pop(name, None)
    prefix = f"releases/{tag}/"
    command = ["aws", "--endpoint-url", endpoint, "--no-cli-pager", "s3api"]
    manifest_key = prefix + "manifest.json"
    manifest = subprocess.run(
        [*command, "head-object", "--bucket", bucket, "--key", manifest_key],
        env=environment, capture_output=True, text=True,
    )
    if manifest.returncode == 0:
        raise ValueError(f"{manifest_key} already exists")
    if "(404)" not in manifest.stderr and "(NotFound)" not in manifest.stderr:
        raise RuntimeError("Could not check for an existing release in R2")

    assets = sorted(path for path in output.rglob("*") if path.is_file() and path.name != "manifest.json")
    for path in [*assets, output / "manifest.json"]:
        key = prefix + path.relative_to(output).as_posix()
        if path.name == "manifest.json":
            content_type = "application/json"
        elif path.name == "SHA256SUMS" or path.suffix in (".sh", ".ps1"):
            content_type = "text/plain; charset=utf-8"
        else:
            content_type = "application/octet-stream"
        args = [*command, "put-object", "--bucket", bucket, "--key", key, "--body", str(path),
                "--content-type", content_type, "--cache-control", "public, max-age=31536000, immutable"]
        if key == manifest_key:
            args.extend(["--if-none-match", "*"])
        subprocess.run(args, env=environment, stdout=subprocess.DEVNULL, check=True)
        print(f"Uploaded {key}", flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("tag")
    parser.add_argument("commit")
    parser.add_argument("build", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--assemble-only", action="store_true")
    args = parser.parse_args()
    if not re.fullmatch(f"v{VERSION}", args.tag) or not re.fullmatch(r"[0-9a-f]{40}", args.commit):
        parser.error("expected vMAJOR.MINOR.PATCH and a full commit SHA")
    credentials = json.load(sys.stdin)
    args.output.mkdir(parents=True, exist_ok=False)
    assemble(args.tag, args.commit, args.build, args.output, credentials["LAUNCHER_SIGNING_KEY"])
    if not args.assemble_only:
        publish(args.tag, args.output, credentials)


if __name__ == "__main__":
    main()
