# Try OVA screening with deliveries in Google Drive

This guide takes you from a fresh Debian 13 computer to an initial evaluation
of one student's OVA file stored in your Google Drive. It assumes you can sign
in to the computer as an administrator and use a web browser on that computer.
You will also use the Terminal application; copy each command into it and press
Enter. When `sudo` asks for your computer password, type it and press Enter;
the password will not appear as you type.

**This is a real-world trial.** Google Drive authorization and downloading have
not yet been tested end to end with a professor's account. The OVA workflow has
passed a small synthetic test, but the large example OVA has not completed a
full run. Start with a test file and test student, keep the originals, and do
not use the resulting grades for consequential decisions without reviewing the
evidence. Record any difficulty using the checklist at the end.

The example checks whether Thot can import an OVA and start, stop and remove its
virtual machine. It does **not** check installed operating systems, partitions,
boot menus or shared data. Starting a virtual machine is not proof that its
operating system booted correctly.

## 1. Prepare the computer

Use a dedicated x86-64 Debian 13 computer with a graphical desktop, internet,
hardware virtualization enabled in its firmware, and administrator (`sudo`)
access. This workflow uses KVM/libvirt, not VirtualBox. The outer VM is
configured for 4 virtual CPUs, 6 GiB RAM and a 100 GiB virtual disk; allow
additional RAM for Debian itself. The imported VM is capped at 2 CPUs and
2 GiB RAM. Nested VMs may run slowly under software emulation.

The disk is sparse and can grow considerably. For the example 8.49 GB OVA,
whose virtual disk is 60 GiB, allow at least 100 GiB for the outer guest **plus**
space for the downloaded OVA, extraction, overlays and system overhead. Check
free space on both the VM storage and temporary-file filesystems, and monitor it
during the run. More space may be needed for your OVA. The previously tested
59 GiB host filled up before import completed.

Open Terminal and install the prerequisites (the first command also installs
`curl` before it is used to download Thot):

```sh
sudo apt update
sudo apt install curl git nano qemu-system-x86 libvirt-daemon-system libvirt-clients virtinst libguestfs-tools cloud-image-utils
sudo systemctl enable --now libvirtd
sudo virsh net-start default
sudo virsh net-autostart default
sudo usermod -aG kvm,libvirt,libvirt-qemu "$USER"
sudo install -d -o "$USER" -g libvirt-qemu -m 2770 /var/lib/libvirt/images/thot-alpine-poc
```

If `net-start` says the network is already active, continue. Log **out of your
desktop session and back in** to apply the group changes, then open a new
Terminal and check:

```sh
test -c /dev/kvm && echo 'KVM device available' || echo 'KVM device missing'
virsh -c qemu:///system list --all
sudo virsh net-info default
df -h /var/lib/libvirt/images/thot-alpine-poc /tmp "$HOME"
```

The network should say `Active: yes`. If `/dev/kvm` is missing, enable CPU
virtualization in the computer's firmware or ask your administrator; if
libvirt access fails, check group membership with `groups` and log in again.
The default network is used to prepare Alpine and install packages. Student
test VMs have no network device.

## 2. Install Thot and obtain this example

Install a published x86-64 release that includes the KVM-Alpine provider,
Google Drive delivery agent and `alpine-ova` profile. The installer also
installs service components; this example uses the local CLI. It verifies the
release archive checksum.

```sh
curl -fLO https://github.com/cbarahon/thot/releases/latest/download/thot-install.sh
sudo bash thot-install.sh
thot --help
thot-alpine-agent --version
```

The release and example must be compatible. The release archive does not
include this example, so obtain the project files separately. For a published
release, use its matching tag in the checkout; replace `vX.Y.Z` below with the
installed version printed by the installer (also in
`/usr/local/share/thot/VERSION`). If the release predates this example or
does not support its profile, obtain a compatible release and matching source
from the maintainer before continuing.

```sh
git clone https://github.com/cbarahon/thot.git "$HOME/thot-source"
git -C "$HOME/thot-source" checkout vX.Y.Z
cp -a "$HOME/thot-source/examples/ova-screening-kvm-alpine" "$HOME/ova-screening"
cd "$HOME/ova-screening"
mkdir -p results
thot validate
```

`thot validate` should report a valid project. If checkout or validation fails,
stop and record the exact message and installed version; do not assume that a
newer checkout works with an older release. The rest of the guide assumes all
commands are run from `~/ova-screening`, as your normal user, without `sudo`.
Return there in a new terminal with `cd "$HOME/ova-screening"`.

## 3. Prepare the Drive folder

In [Google Drive](https://drive.google.com/), create a folder in **My Drive**
and upload an ordinary `.ova` file directly into it. For the first test, use
the filename `ArchLinux_Dualboot.ova`, matching the supplied assignment. The
example does not include this OVA. You can use another name by changing the
assignment in step 7. Wait for the upload to finish, open the folder and copy
the browser address. It should look like:

```text
https://drive.google.com/drive/folders/FOLDER_ID
```

The account you authorize must be able to read the folder and file. Use a
regular file directly in the folder: Drive folders, shortcuts and Google
Docs/Sheets/Slides are not supported as deliveries; shared-drive cases have
not been qualified. The project accepts files up to 20 GiB by default, subject
to host and guest storage. Keep each filename unique unless selecting by ID.

## 4. Obtain the pilot application's details

The developer creates the Google Cloud application and bundles its client credentials in the release. For a pilot, give the developer the Google account you will use so they can add it as a test user. Never send your Google password or authorization tokens. A Workspace account may be restricted by its administrator. See the [developer guide](../../docs/guides/google-drive-developer.md).

The permission requested is read-only but applies to **all Drive files visible
to the authorized account**, not just the chosen folder. Continue only if you
accept that access. Do not put client values or tokens in the project,
screenshots, shared logs or issue reports.

## 5. Configure the example to use Drive

In Terminal, open the project configuration:

```sh
nano configuration.thot.yaml
```

Immediately below the existing `delivery_folder: deliveries` line, add the
following three lines, replacing the example URL with your copied folder address
and preserving the indentation:

```yaml
delivery_agent:
  kind: gdrive
  folder_url: https://drive.google.com/drive/folders/FOLDER_ID
```

`kind` and `folder_url` must each have two leading
spaces. Leave the existing `delivery_folder` and other settings in place.
Save with Ctrl+O, Enter, then exit with Ctrl+X. Check the configuration:

```sh
thot validate
```

Validation checks project configuration; it does not prove that Google access
or the OVA evaluation works.

## 6. Sign in and test downloading one file

Run in the project directory:

```sh
thot deliveries list .
```

Thot prints an authorization link and attempts to open your browser. If it
does not open, copy the printed link into a browser **on this computer**, while
the command is waiting. Sign in with the intended account, review the access
request and consent. Google may show an unverified-app warning for the pilot;
proceed only after verifying it is the application supplied by the developer.
The callback must reach this computer's localhost within five minutes. Return
to Terminal: the command should display a table including the
OVA's exact name, ID and size. The browser message alone does not establish
that the token exchange or listing succeeded.

Thot saves the client credentials and access/refresh tokens in a private user
cache, normally `~/.config/thot/google-drive-token.json`. Subsequent runs as
the same Linux user can reuse it; do not use `sudo` for the commands in this
guide. If Google refuses access, check the selected account and the error in
Terminal; ask the developer to check the test-user entry, API and organization
policy. Testing consent can expire after seven days and require a new
authorization. If the cache belongs to a different OAuth client or account,
move it to a safe private backup before
authorizing again. A remote/headless server requires a separate desktop
authorization and secure transfer of the private token cache; see the
[delivery guide](../../docs/guides/delivery-agents.md).

Now test a standalone download. This creates a local copy and can require the
full file size in additional free disk space:

```sh
mkdir -p review
thot deliveries get ArchLinux_Dualboot.ova . --output review/ArchLinux_Dualboot.ova
sha256sum review/ArchLinux_Dualboot.ova
```

The command prints the destination and SHA-256. Compare that digest with a
trusted digest of the original if you have one. The destination must not
already exist; choose a new output name for a retry. Keep or remove this review
copy yourself after checking it. A successful download is a useful first check,
but does not establish that evaluation works.

## 7. Connect students to their files

The supplied [students.yaml](students.yaml) contains one test student,
`student1`; [assignments.yaml](assignments.yaml) maps that student to
`ArchLinux_Dualboot.ova`. If you used that exact filename, no edit is needed.
Otherwise edit `assignments.yaml` with `nano assignments.yaml` and replace the
name with the exact name shown by `thot deliveries list .`, including case and
extension. Keep it quoted if it contains YAML special characters.

For more students, edit both files. Each student ID must match across the two
files. For example:

```yaml
# students.yaml
schema_version: thot.dev/v2
identity_field: student_id
students:
  - student_id: student1
    name: Test student one
  - student_id: student2
    name: Test student two
```

```yaml
# assignments.yaml
schema_version: thot.dev/v2
assignments:
  - student_id: student1
    deliverable: "student-one.ova"
  - student_id: student2
    deliverable: "student-two.ova"
```

If Drive contains duplicate names, replace `deliverable` with
`deliverable_id: "FILE_ID"` using the ID shown in the inventory. Specify
exactly one of these selectors per student. The standalone `deliveries get`
command selects by name and cannot disambiguate by ID. For an initial test,
use distinct names. Do not edit the module or preparation scripts: the project
pins their contents and changes require updating their hashes.

## 8. Run and review

Confirm there is sufficient free space, the computer will remain powered on,
and internet is available for Drive and initial Alpine preparation. Then run:

```sh
thot validate
thot run
thot status --watch
```

`thot run` starts a background job and prints a run ID; save it. The first run
downloads and prepares an Alpine image and may take a long time. Press Ctrl+C
to stop watching status; this does not stop the background run. In another
terminal, from the project folder, inspect the log with `thot log --follow`
(Ctrl+C stops following). Monitor disk space with the `df -h` command from
step 1. Do not start another run while one is active.

The runner processes students sequentially, downloading the current Drive
file into temporary staging, transferring it to a disposable guest, and
removing the staged copy after its tests. It does not delete the Drive file.
After the job finishes, inspect the results:

```sh
thot status
thot log
thot report
```

`thot report RUN_ID` opens an earlier run; replace `RUN_ID` with the printed
ID. Results and logs are stored under the project's `results/` directory.
Check each test and its diagnostics, not just the score. A pending
infrastructure result is not a student zero; a test failure can also expose
an unsupported OVA or implementation defect. Keep results private because they
can contain student names and assessment evidence.

## 9. Record the trial

Record the installed Thot version, Debian version, host CPU/RAM and available
disk space, OVA size and virtual disk capacity, run ID, and whether each step
below succeeded. Capture the exact error and the relevant `thot log` output
for failures, including the point at which they occurred:

1. Install, KVM and libvirt checks, and project validation.
2. OAuth consent, folder listing and standalone download; compare the
   checksum if a trusted original digest is available.
3. Assignment lookup, student-by-student acquisition, import, start, stop,
   cleanup and final report. For multiple students, check the log order.
4. Verify the original remains in Drive and note disk usage or any interrupted
   downloads, timeouts, confusing messages or manual steps.

Remove credentials, tokens, authorization links and sensitive student data
before sharing diagnostics. Retry/backoff and resumable Drive downloads are
not implemented; an interrupted download may need a new attempt. See the
[OVA screening guide](../../docs/guides/ova-screening.md) for technical
details and known limitations.
