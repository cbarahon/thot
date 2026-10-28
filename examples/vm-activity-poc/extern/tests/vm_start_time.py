#!/usr/bin/env python3
# Purpose: Example external test that measures HTTP readiness and saves VM evidence.
"""Example teacher-owned external test: measure HTTP readiness and capture evidence."""

import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path


def command(args, timeout=30):
    args = list(args)
    if args and args[0] == "VBoxManage":
        args[0] = os.environ.get("VBOX_MANAGE_PATH", "VBoxManage")
    return subprocess.run(args, check=False, capture_output=True, text=True, timeout=timeout)


def vm_is_running(vm_name):
    result = command(["VBoxManage", "showvminfo", vm_name, "--machinereadable"], timeout=10)
    return result.returncode == 0 and 'VMState="running"' in result.stdout


def stop_vm(vm_name):
    if not vm_is_running(vm_name):
        return
    command(["VBoxManage", "controlvm", vm_name, "acpipowerbutton"], timeout=10)
    deadline = time.monotonic() + 25
    while time.monotonic() < deadline:
        if not vm_is_running(vm_name):
            return
        time.sleep(0.5)
    result = command(["VBoxManage", "controlvm", vm_name, "poweroff"], timeout=10)
    if result.returncode != 0:
        raise RuntimeError("VBoxManage poweroff failed: " + result.stderr.strip())
    time.sleep(5)


def main():
    parameters = dict(arg.split("=", 1) for arg in sys.argv[1:] if "=" in arg)
    vm_name = parameters.get("vm_name")
    if not vm_name:
        raise ValueError("vm_name=... is required")

    artifacts = Path(os.environ["THOT_ARTIFACT_DIR"])
    artifacts.mkdir(parents=True, exist_ok=True)
    html_path = artifacts / "response.html"
    screenshot_path = artifacts / "screenshot.png"
    started_at = time.time()
    start = time.monotonic()
    status = None
    body = b""
    error = None
    elapsed_ms = None
    try:
        start_result = command(["VBoxManage", "startvm", vm_name, "--type", "headless"], timeout=30)
        if start_result.returncode != 0:
            raise RuntimeError("VBoxManage startvm failed: " + start_result.stderr.strip())

        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            try:
                with urllib.request.urlopen("http://127.0.0.1:8080/", timeout=2) as response:
                    status = response.status
                    body = response.read(10 * 1024 * 1024 + 1)
                    elapsed_ms = round((time.monotonic() - start) * 1000)
                break
            except urllib.error.HTTPError as response:
                status = response.code
                body = response.read(10 * 1024 * 1024 + 1)
                elapsed_ms = round((time.monotonic() - start) * 1000)
                break
            except (urllib.error.URLError, TimeoutError, OSError) as exc:
                error = str(exc)
                time.sleep(1)

        if elapsed_ms is None:
            elapsed_ms = round((time.monotonic() - start) * 1000)
        if status is not None:
            html_path.write_bytes(body[: 10 * 1024 * 1024])
        shot = command(["VBoxManage", "controlvm", vm_name, "screenshotpng", str(screenshot_path)], timeout=30)
        if shot.returncode != 0:
            raise RuntimeError("VirtualBox screenshot failed: " + shot.stderr.strip())
    finally:
        stop_vm(vm_name)

    result = {
        "vm_name": vm_name,
        "started_at_unix": started_at,
        "elapsed_ms": elapsed_ms,
        "http_status": status,
        "html_path": str(html_path) if status is not None else None,
        "html_truncated": len(body) > 10 * 1024 * 1024,
        "screenshot_path": str(screenshot_path),
        "error": error if status is None else None,
    }
    print(json.dumps(result, sort_keys=True))
    return 0 if status is not None else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:
        print(str(exc), file=sys.stderr)
        sys.exit(1)
