#!/bin/sh
set -eu

# The module is trusted instructor code. It talks to the Alpine clone's
# system-libvirt socket using only the libvirt.system capability; it never
# sees the host libvirt socket. The imported OVA guest receives no NIC.
exec python3 - "$@" <<'PY'
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import time
import xml.etree.ElementTree as ET
from pathlib import Path, PurePosixPath

URI = "qemu:///system"
ROOT = Path("/tmp/thot-libvirt")
OWNER = "thot.libvirt.ova-vm/v1"
INFRASTRUCTURE_EXIT = 75
MAX_OVA_BYTES = 20 * 1024**3
MAX_EXTRACTED_BYTES = 20 * 1024**3
MAX_VIRTUAL_DISK_BYTES = 128 * 1024**3
MAX_VIRTUAL_MEMORY_MIB = 65536
MAX_VCPUS = 32
NS = {
    "ovf": "http://schemas.dmtf.org/ovf/envelope/1",
    "rasd": "http://schemas.dmtf.org/wbem/wscim/1/cim-schema/2/CIM_ResourceAllocationSettingData",
    "vbox": "http://www.virtualbox.org/ovf/machine",
}


class ModuleError(RuntimeError):
    def __init__(self, message, infrastructure=False):
        super().__init__(message)
        self.infrastructure = infrastructure


def fail(message, infrastructure=False, **details):
    raise ModuleError(message + (": " + json.dumps(details, sort_keys=True) if details else ""),
                      infrastructure=infrastructure)


def command(argv, timeout=60):
    try:
        result = subprocess.run(argv, text=True, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, timeout=timeout, check=False)
    except subprocess.TimeoutExpired as error:
        fail("command timed out", command=argv[0], timeout_seconds=timeout,
             infrastructure=True, stdout=(error.stdout or "")[-4000:],
             stderr=(error.stderr or "")[-4000:])
    if result.returncode != 0:
        fail("command failed", command=argv[0], exit_code=result.returncode,
             infrastructure=argv[0] == "virsh",
             stdout=result.stdout[-4000:], stderr=result.stderr[-4000:])
    return result.stdout.strip(), result.stderr.strip()


def safe_name(value):
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,40}", value or ""):
        fail("VM name must contain only letters, digits, dot, underscore or dash")
    return value


def vm_domain_name(name):
    return "thot-ova-" + safe_name(name)


def workdir(name):
    ROOT.mkdir(mode=0o755, parents=True, exist_ok=True)
    root = ROOT.resolve(strict=True)
    if ROOT.is_symlink() or root != Path("/tmp/thot-libvirt"):
        fail("libvirt work root is not the expected private path")
    return root / safe_name(name)


def load_manifest(name):
    directory = workdir(name)
    info = os.lstat(directory)
    if not stat.S_ISDIR(info.st_mode) or directory.is_symlink():
        fail("owned VM storage is not a regular directory")
    manifest_path = directory / "ownership.json"
    info = os.lstat(manifest_path)
    if not stat.S_ISREG(info.st_mode) or manifest_path.is_symlink():
        fail("VM ownership manifest is not a regular file")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("owner") != OWNER or manifest.get("requested_name") != name:
        fail("VM ownership manifest does not match this module request")
    return directory, manifest


def save_manifest(directory, manifest):
    path = directory / "ownership.json"
    temporary = directory / ".ownership.pending"
    temporary.write_text(json.dumps(manifest, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    os.chmod(temporary, 0o644)
    os.replace(temporary, path)


def verify_ova_manifest(directory, members):
    manifest_members = [path for path in members if path.suffix.lower() == ".mf"]
    if not manifest_members:
        return {"present": False, "verified_entries": 0}
    if len(manifest_members) != 1:
        fail("OVA contains more than one manifest")
    pattern = re.compile(r"^(SHA1|SHA256|SHA512)\(([^)]+)\)=\s*([0-9a-fA-F]+)\s*$")
    verified = 0
    for line in manifest_members[0].read_text(encoding="utf-8", errors="strict").splitlines():
        if not line.strip():
            continue
        match = pattern.match(line.strip())
        if not match:
            fail("OVA manifest contains an unsupported digest entry", entry=line[:160])
        algorithm, relative, expected = match.groups()
        relative_path = PurePosixPath(relative)
        if relative_path.is_absolute() or any(part in ("", ".", "..") for part in relative_path.parts):
            fail("OVA manifest references an unsafe path", path=relative)
        target = directory.joinpath(*relative_path.parts)
        if target not in members:
            fail("OVA manifest references a missing member", path=relative)
        digest = hashlib.new(algorithm.lower())
        with target.open("rb") as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(block)
        if digest.hexdigest().lower() != expected.lower():
            fail("OVA member digest does not match its manifest", path=relative, algorithm=algorithm)
        verified += 1
    return {"present": True, "verified_entries": verified}


def extract_ova(ova, destination):
    ova = Path(ova)
    if not str(ova).startswith("/work/"):
        fail("deliverable path must be staged below /work")
    source_info = os.lstat(ova)
    if not stat.S_ISREG(source_info.st_mode) or ova.is_symlink():
        fail("OVA input must be a regular non-symlink file")
    if source_info.st_size == 0 or source_info.st_size > MAX_OVA_BYTES:
        fail("OVA input size is outside the module limit", size_bytes=source_info.st_size,
             max_bytes=MAX_OVA_BYTES)
    with tarfile.open(ova, mode="r:*") as archive:
        entries = archive.getmembers()
        if not entries or len(entries) > 64:
            fail("OVA member count is outside the supported limit", count=len(entries))
        total = 0
        safe_entries = []
        seen = set()
        for member in entries:
            relative = PurePosixPath(member.name)
            if relative.is_absolute() or any(part in ("", ".", "..") for part in relative.parts):
                fail("OVA contains an unsafe member path", path=member.name)
            if member.name in seen:
                fail("OVA contains a duplicate member path", path=member.name)
            seen.add(member.name)
            if not member.isfile():
                fail("OVA contains a non-regular member", path=member.name)
            total += member.size
            if total > MAX_EXTRACTED_BYTES:
                fail("OVA expanded members exceed the extraction limit", expanded_bytes=total,
                     max_bytes=MAX_EXTRACTED_BYTES)
            safe_entries.append((member, relative))
        required = total + max(source_info.st_size, 1)
        free = shutil.disk_usage(destination).free
        if free < required:
            fail("insufficient guest disk space to unpack OVA", required_free_bytes=required,
             available_bytes=free, infrastructure=True)
        extracted = []
        for member, relative in safe_entries:
            target = destination.joinpath(*relative.parts)
            target.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
            resolved_parent = target.parent.resolve(strict=True)
            if not resolved_parent.is_relative_to(destination.resolve(strict=True)):
                fail("OVA extraction escaped its private directory", path=member.name)
            with archive.extractfile(member) as source, target.open("xb") as output:
                shutil.copyfileobj(source, output, 1024 * 1024)
            os.chmod(target, 0o644)
            extracted.append(target)
    hashes = verify_ova_manifest(destination, extracted)
    descriptors = [path for path in extracted if path.suffix.lower() == ".ovf"]
    if len(descriptors) != 1:
        fail("OVA must contain exactly one OVF descriptor", count=len(descriptors))
    return descriptors[0], extracted, hashes, source_info.st_size


def child_text(node, path, default=None):
    value = node.find(path, NS)
    return value.text.strip() if value is not None and value.text else default


def parse_ovf(descriptor):
    raw = descriptor.read_bytes()
    if b"<!DOCTYPE" in raw.upper() or b"<!ENTITY" in raw.upper():
        fail("OVF DTD/entity declarations are not accepted")
    try:
        root = ET.fromstring(raw)
    except ET.ParseError as error:
        fail("OVF XML is invalid", detail=str(error))
    refs = {}
    for entry in root.findall("./ovf:References/ovf:File", NS):
        file_id = entry.get("{" + NS["ovf"] + "}id")
        href = entry.get("{" + NS["ovf"] + "}href")
        if not file_id or not href:
            fail("OVF file reference is incomplete")
        rel = PurePosixPath(href)
        if rel.is_absolute() or any(part in ("", ".", "..") for part in rel.parts):
            fail("OVF refers to an unsafe file path", href=href)
        refs[file_id] = rel
    disks = {}
    for entry in root.findall("./ovf:DiskSection/ovf:Disk", NS):
        disk_id = entry.get("{" + NS["ovf"] + "}diskId")
        file_ref = entry.get("{" + NS["ovf"] + "}fileRef")
        capacity = entry.get("{" + NS["ovf"] + "}capacity")
        if not disk_id or file_ref not in refs or not capacity:
            fail("OVF disk definition is incomplete")
        try:
            capacity_bytes = int(capacity)
        except ValueError:
            fail("OVF disk capacity is not an integer", disk_id=disk_id)
        if capacity_bytes <= 0 or capacity_bytes > MAX_VIRTUAL_DISK_BYTES:
            fail("OVF disk capacity exceeds the module limit", disk_id=disk_id,
                 capacity_bytes=capacity_bytes)
        disks[disk_id] = {"source": refs[file_ref], "capacity_bytes": capacity_bytes}
    if not disks:
        fail("OVF has no virtual disks")
    cpu = 1
    memory_mib = 1024
    ordered_disk_ids = []
    network_count = 0
    for item in root.findall(".//ovf:VirtualHardwareSection/ovf:Item", NS):
        resource_type = child_text(item, "rasd:ResourceType")
        if resource_type == "3":
            try:
                cpu = int(child_text(item, "rasd:VirtualQuantity", "1"))
            except ValueError:
                fail("OVF CPU quantity is invalid")
        elif resource_type == "4":
            try:
                memory = int(child_text(item, "rasd:VirtualQuantity", "1024"))
            except ValueError:
                fail("OVF memory quantity is invalid")
            units = (child_text(item, "rasd:AllocationUnits", "MegaBytes") or "").lower()
            if "byte" in units and "2^20" in units:
                memory_mib = memory
            elif "kilobyte" in units:
                memory_mib = max(1, memory // 1024)
            else:
                memory_mib = memory
        elif resource_type == "17":
            host_resource = child_text(item, "rasd:HostResource", "")
            match = re.fullmatch(r"ovf:/disk/([A-Za-z0-9_.-]+)", host_resource)
            if not match or match.group(1) not in disks:
                fail("OVF hard disk item points to an unknown disk", host_resource=host_resource)
            if match.group(1) not in ordered_disk_ids:
                ordered_disk_ids.append(match.group(1))
        elif resource_type == "10":
            network_count += 1
    for disk_id in disks:
        if disk_id not in ordered_disk_ids:
            ordered_disk_ids.append(disk_id)
    if cpu < 1 or cpu > MAX_VCPUS or memory_mib < 128 or memory_mib > MAX_VIRTUAL_MEMORY_MIB:
        fail("OVF CPU/memory values are outside the supported range", vcpus=cpu,
             memory_mib=memory_mib)
    machine = root.find(".//vbox:Machine", NS)
    firmware = "bios"
    if machine is not None:
        firmware_node = machine.find("./ovf:Hardware/ovf:Firmware", NS)
        if firmware_node is not None and firmware_node.get("type", "").lower() == "efi":
            firmware = "efi"
    return {
        "descriptor": descriptor.name,
        "vcpus": cpu,
        "memory_mib": memory_mib,
        "firmware": firmware,
        "disks": [{"disk_id": disk_id, **disks[disk_id],
                   "source": disks[disk_id]["source"].as_posix()}
                  for disk_id in ordered_disk_ids],
        "network_adapters_omitted": network_count,
        "source_ostype": (machine.get("OSType") if machine is not None else None),
    }


def find_ovmf():
    roots = [Path("/usr/share/edk2-ovmf"), Path("/usr/share/OVMF"), Path("/usr/share/qemu")]
    code = variables = None
    for root in roots:
        if root.exists():
            for path in root.rglob("*"):
                if path.is_file() and path.name in ("OVMF_CODE.fd", "OVMF_CODE_4M.fd") and code is None:
                    code = path
                if path.is_file() and path.name in ("OVMF_VARS.fd", "OVMF_VARS_4M.fd") and variables is None:
                    variables = path
    if code is None or variables is None:
        fail("EFI OVF requires OVMF_CODE.fd and OVMF_VARS.fd from edk2-ovmf")
    return code, variables


def define_domain(name, directory, summary):
    domain_name = vm_domain_name(name)
    emulator = shutil.which("qemu-system-x86_64")
    if not emulator:
        fail("qemu-system-x86_64 is missing from the prepared Alpine baseline")
    domain = ET.Element("domain", {"type": "qemu"})
    ET.SubElement(domain, "name").text = domain_name
    ET.SubElement(domain, "memory", {"unit": "MiB"}).text = str(summary["memory_mib"])
    ET.SubElement(domain, "currentMemory", {"unit": "MiB"}).text = str(summary["memory_mib"])
    ET.SubElement(domain, "vcpu", {"placement": "static"}).text = str(summary["vcpus"])
    os_node = ET.SubElement(domain, "os")
    ET.SubElement(os_node, "type", {"arch": "x86_64", "machine": "q35"}).text = "hvm"
    if summary["firmware"] == "efi":
        code, variables = find_ovmf()
        vars_copy = directory / "nvram.fd"
        shutil.copyfile(variables, vars_copy)
        os.chmod(vars_copy, 0o666)
        ET.SubElement(os_node, "loader", {"readonly": "yes", "type": "pflash"}).text = str(code)
        ET.SubElement(os_node, "nvram", {"template": str(variables)}).text = str(vars_copy)
    ET.SubElement(os_node, "boot", {"dev": "hd"})
    features = ET.SubElement(domain, "features")
    ET.SubElement(features, "acpi")
    ET.SubElement(features, "apic")
    ET.SubElement(domain, "clock", {"offset": "utc"})
    ET.SubElement(domain, "devices")
    devices = domain.find("devices")
    ET.SubElement(devices, "emulator").text = emulator
    ET.SubElement(devices, "controller", {"type": "sata", "index": "0"})
    for index, disk in enumerate(summary["disks"]):
        disk_path = directory / (disk["disk_id"] + ".qcow2")
        disk_node = ET.SubElement(devices, "disk", {"type": "file", "device": "disk"})
        ET.SubElement(disk_node, "driver", {"name": "qemu", "type": "qcow2"})
        ET.SubElement(disk_node, "source", {"file": str(disk_path)})
        ET.SubElement(disk_node, "target", {"dev": "sd" + chr(ord("a") + index), "bus": "sata"})
        address = ET.SubElement(disk_node, "address", {"type": "drive", "controller": "0",
                                                        "bus": "0", "target": "0", "unit": str(index)})
    # A libvirt domain with no graphics device is headless. The string value
    # "none" is not accepted as a <graphics type="..."> by libvirt.
    ET.SubElement(devices, "serial", {"type": "pty"})
    ET.SubElement(devices, "console", {"type": "pty"})
    xml_path = directory / "domain.xml"
    xml_path.write_bytes(ET.tostring(domain, encoding="utf-8", xml_declaration=True))
    os.chmod(xml_path, 0o644)
    command(["virsh", "-c", URI, "define", str(xml_path)])
    uuid, _ = command(["virsh", "-c", URI, "domuuid", domain_name])
    return domain_name, uuid.strip()


def import_ova(ova, name):
    directory = workdir(name)
    if directory.exists() or directory.is_symlink():
        fail("a libvirt OVA workspace with this name already exists")
    directory.mkdir(mode=0o755, parents=False)
    os.chmod(directory, 0o755)
    manifest = {"owner": OWNER, "requested_name": name, "domain_name": vm_domain_name(name),
                "state": "importing", "created_at": int(time.time()), "domain_uuid": None}
    save_manifest(directory, manifest)
    descriptor, extracted, verified_manifest, ova_bytes = extract_ova(ova, directory)
    summary = parse_ovf(descriptor)
    total_virtual = 0
    converted = []
    for disk in summary["disks"]:
        total_virtual += disk["capacity_bytes"]
    if total_virtual > MAX_VIRTUAL_DISK_BYTES:
        fail("combined virtual disk capacity exceeds the module limit", capacity_bytes=total_virtual)
    # Reserve conservatively for a fully allocated qcow2 conversion. Do this
    # before conversion: a sparse source must not trick the module into filling
    # the disposable guest disk while it expands toward its virtual capacity.
    free_before_conversion = shutil.disk_usage(directory).free
    conversion_reserve = sum(disk["capacity_bytes"] for disk in summary["disks"])
    if free_before_conversion < conversion_reserve:
        fail("insufficient guest disk space for safe virtual-disk conversion",
             required_free_bytes=conversion_reserve, available_bytes=free_before_conversion,
             virtual_disk_bytes=total_virtual, infrastructure=True)
    for disk in summary["disks"]:
        source = directory.joinpath(*PurePosixPath(disk["source"]).parts)
        info_raw, _ = command(["qemu-img", "info", "--output=json", str(source)])
        info = json.loads(info_raw)
        if info.get("format") not in ("vmdk", "vdi"):
            fail("OVA disk format is not VMDK or VDI", disk=disk["disk_id"], format=info.get("format"))
        disk["source_format"] = info["format"]
        disk["virtual_size_bytes"] = info.get("virtual-size")
        if disk["virtual_size_bytes"] != disk["capacity_bytes"]:
            fail("OVF and disk virtual sizes disagree", disk=disk["disk_id"],
                 ovf_bytes=disk["capacity_bytes"], image_bytes=disk["virtual_size_bytes"])
        output = directory / (disk["disk_id"] + ".qcow2")
        command(["qemu-img", "convert", "-f", info["format"], "-O", "qcow2", str(source), str(output)], timeout=1800)
        os.chmod(output, 0o644)
        disk["qcow2_size_bytes"] = output.stat().st_size
        converted.append(output)
    # Retain the original OVA as input evidence in Thot's run folder; remove only
    # extracted source members after the converted disks are verified.
    for path in extracted:
        if path.is_file() and path.suffix.lower() in (".vmdk", ".vdi", ".ovf", ".mf", ".nvram"):
            path.unlink()
    summary["ova_size_bytes"] = ova_bytes
    summary["verified_manifest"] = verified_manifest
    summary["converted_disks"] = [str(path) for path in converted]
    summary["network_isolated"] = True
    summary["firmware_variables_source"] = "fresh OVMF template; VirtualBox NVRAM not imported"
    domain_name, domain_uuid = define_domain(name, directory, summary)
    manifest.update({"state": "imported", "domain_name": domain_name,
                     "domain_uuid": domain_uuid, "summary": summary})
    save_manifest(directory, manifest)
    print(json.dumps({"operation": "libvirt/import_ova", "status": "imported",
                      "domain_name": domain_name, "domain_uuid": domain_uuid,
                      "summary": summary}, sort_keys=True))


def verify_domain(name):
    directory, manifest = load_manifest(name)
    domain_name = manifest["domain_name"]
    uuid, _ = command(["virsh", "-c", URI, "domuuid", domain_name])
    if uuid.strip() != manifest.get("domain_uuid"):
        fail("libvirt VM UUID does not match its ownership manifest")
    xml, _ = command(["virsh", "-c", URI, "dumpxml", domain_name])
    root = ET.fromstring(xml)
    for source in root.findall("./devices/disk/source"):
        filename = source.get("file")
        if not filename or not Path(filename).resolve().is_relative_to(directory.resolve()):
            fail("libvirt VM disk is outside its owned workspace")
    return directory, manifest, domain_name


def vm_state(domain_name):
    state, _ = command(["virsh", "-c", URI, "domstate", domain_name])
    return state.strip().lower()


def start_vm(name):
    _, manifest, domain_name = verify_domain(name)
    state = vm_state(domain_name)
    if state == "running":
        method = "already_running"
    else:
        stdout, stderr = command(["virsh", "-c", URI, "start", domain_name], timeout=120)
        method = "started"
    print(json.dumps({"operation": "libvirt/start_vm", "status": "running",
                      "domain_name": domain_name, "domain_uuid": manifest["domain_uuid"],
                      "start_method": method, "acceleration": "qemu-tcg"}, sort_keys=True))


def stop_vm(name):
    _, manifest, domain_name = verify_domain(name)
    state = vm_state(domain_name)
    if state in ("shut off", "shutoff", "crashed"):
        method = "already_off"
    else:
        command(["virsh", "-c", URI, "shutdown", "--mode", "acpi", domain_name])
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            if vm_state(domain_name) in ("shut off", "shutoff", "crashed"):
                method = "acpi"
                break
            time.sleep(1)
        else:
            command(["virsh", "-c", URI, "destroy", domain_name])
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline and vm_state(domain_name) not in ("shut off", "shutoff", "crashed"):
                time.sleep(1)
            if vm_state(domain_name) not in ("shut off", "shutoff", "crashed"):
                fail("libvirt VM remained active after forced stop", domain_name=domain_name)
            method = "forced_destroy"
    print(json.dumps({"operation": "libvirt/stop_vm", "status": "stopped",
                      "domain_name": domain_name, "domain_uuid": manifest["domain_uuid"],
                      "shutdown_method": method}, sort_keys=True))


def destroy_vm(name):
    directory, manifest = load_manifest(name)
    domain_name = manifest["domain_name"]
    exists = subprocess.run(["virsh", "-c", URI, "dominfo", domain_name],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, timeout=30, check=False).returncode == 0
    if exists:
        verify_domain(name)
        if vm_state(domain_name) not in ("shut off", "shutoff", "crashed"):
            command(["virsh", "-c", URI, "destroy", domain_name])
        command(["virsh", "-c", URI, "undefine", domain_name, "--nvram"])
    root = ROOT.resolve(strict=True)
    resolved = directory.resolve(strict=True)
    if resolved.parent != root or resolved.name != name or directory.is_symlink():
        fail("refusing to remove a workspace outside the owned libvirt root")
    shutil.rmtree(resolved)
    print(json.dumps({"operation": "libvirt/destroy_vm", "status": "destroyed",
                      "domain_name": domain_name, "vm_files_removed": True,
                      "domain_existed": exists}, sort_keys=True))


def main():
    if len(sys.argv) < 3:
        fail("usage: entrypoint.sh <import_ova|start_vm|stop_vm|destroy_vm> <name> [ova_path]")
    action, name = sys.argv[1], safe_name(sys.argv[2])
    if action == "import_ova" and len(sys.argv) == 4:
        import_ova(sys.argv[3], name)
    elif action == "start_vm" and len(sys.argv) == 3:
        start_vm(name)
    elif action == "stop_vm" and len(sys.argv) == 3:
        stop_vm(name)
    elif action == "destroy_vm" and len(sys.argv) == 3:
        destroy_vm(name)
    else:
        fail("unknown libvirt operation or incorrect arguments", operation=action)


try:
    main()
except Exception as error:
    print(json.dumps({"status": "error", "error": str(error)}, sort_keys=True), file=sys.stderr)
    sys.exit(INFRASTRUCTURE_EXIT if isinstance(error, ModuleError) and error.infrastructure else 1)
PY
