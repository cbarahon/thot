#!/usr/bin/env bash
# Purpose: Downloads a verified Thot release and installs its binaries and systemd service.
set -Eeuo pipefail

readonly REPOSITORY="cbarahon/thot"
readonly ASSET="thot-linux-x86_64-unknown-linux-gnu.tar.gz"
readonly INSTALL_ROOT="/usr/local"
readonly DATA_ROOT="/var/lib/thot"
readonly VERSION="${THOT_VERSION:-latest}"

fail() {
    echo "thot installer: $*" >&2
    exit 1
}

[[ "$(id -u)" -eq 0 ]] || fail "run as root, for example: sudo bash thot-install.sh"
[[ "$(uname -s)" == "Linux" ]] || fail "this release supports Linux hosts only"
[[ "$(uname -m)" == "x86_64" ]] || fail "this release supports x86_64 hosts only"

for command_name in curl sha256sum tar install systemctl useradd groupadd getent awk sed grep ldd cat; do
    command -v "${command_name}" >/dev/null 2>&1 || fail "required command is missing: ${command_name}"
done
[[ -d /run/systemd/system ]] || fail "systemd is required"

vbox_manage="$(command -v VBoxManage || true)"
[[ -n "${vbox_manage}" ]] || fail "install VirtualBox before installing Thot"
[[ -c /dev/vboxdrv ]] || fail "VirtualBox driver /dev/vboxdrv is unavailable; load the VirtualBox kernel module"
vbox_full_version="$("${vbox_manage}" --version)" || fail "VBoxManage --version failed"
vbox_version="${vbox_full_version%%r*}"
[[ -n "${vbox_version}" ]] || fail "could not determine the VirtualBox version"

if [[ "${VERSION}" == "latest" ]]; then
    release_url="https://github.com/${REPOSITORY}/releases/latest/download"
else
    [[ "${VERSION}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "THOT_VERSION must be latest or a version tag such as v0.1.0"
    release_url="https://github.com/${REPOSITORY}/releases/download/${VERSION}"
fi

temporary_dir="$(mktemp -d)"
trap 'rm -rf "${temporary_dir}"' EXIT
curl --fail --location --silent --show-error --retry 3 "${release_url}/${ASSET}" -o "${temporary_dir}/${ASSET}" || fail "could not download ${ASSET} from ${release_url}"
curl --fail --location --silent --show-error --retry 3 "${release_url}/SHA256SUMS" -o "${temporary_dir}/SHA256SUMS" || fail "could not download release checksums"
awk -v asset="${ASSET}" '$2 == asset { print; count++ } END { if (count != 1) exit 1 }' \
    "${temporary_dir}/SHA256SUMS" > "${temporary_dir}/asset-checksum" || fail "release checksum is missing or duplicated"
(cd "${temporary_dir}" && sha256sum --check --status asset-checksum) || fail "release archive checksum verification failed"

tar -tzf "${temporary_dir}/${ASSET}" | awk '
    $0 !~ /^(thot|thotd|thotd\.service|thotd\.env\.example|VERSION)$/ { bad=1 }
    END { exit bad }
' || fail "release archive contains an unexpected path"
tar -xzf "${temporary_dir}/${ASSET}" -C "${temporary_dir}"
for file_name in thot thotd thotd.service thotd.env.example VERSION; do
    [[ -f "${temporary_dir}/${file_name}" ]] || fail "release archive is missing ${file_name}"
done
installed_version="$(cat "${temporary_dir}/VERSION")"
[[ "${installed_version}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "release archive has an invalid version file"
for binary in thot thotd; do
    dependency_report="$(ldd "${temporary_dir}/${binary}" 2>&1)" || fail "cannot inspect runtime libraries for ${binary}"
    if grep -Eq "not found|version .* not found" <<< "${dependency_report}"; then
        fail "${binary} needs runtime libraries unavailable on this host: ${dependency_report}"
    fi
done

if ! getent group thotd >/dev/null; then
    groupadd --system thotd
fi
if ! getent passwd thotd >/dev/null; then
    useradd --system --gid thotd --home-dir "${DATA_ROOT}" --create-home --shell /usr/sbin/nologin thotd
fi

install -d -o thotd -g thotd -m 0750 \
    "${DATA_ROOT}" "${DATA_ROOT}/artifacts" "${DATA_ROOT}/vms" "${DATA_ROOT}/vbox-profile"
install -d -o root -g thotd -m 0750 /etc/thot /etc/thot/projects
install -d -o thotd -g thotd -m 0750 /etc/thot/projects/vms
[[ -d /srv ]] || install -d -o root -g root -m 0755 /srv
install -d -o root -g root -m 0755 "${INSTALL_ROOT}/share/thot"

if [[ ! -e /etc/thot/thotd.env ]]; then
    sed -e "s/@VBOX_VERSION@/${vbox_version}/" \
        -e "s|@VBOX_MANAGE_PATH@|${vbox_manage}|" \
        "${temporary_dir}/thotd.env.example" > "${temporary_dir}/thotd.env"
    install -o root -g thotd -m 0640 "${temporary_dir}/thotd.env" /etc/thot/thotd.env
fi

install -o root -g root -m 0755 "${temporary_dir}/thot" "${INSTALL_ROOT}/bin/thot.new"
install -o root -g root -m 0755 "${temporary_dir}/thotd" "${INSTALL_ROOT}/sbin/thotd.new"
mv -f "${INSTALL_ROOT}/bin/thot.new" "${INSTALL_ROOT}/bin/thot"
mv -f "${INSTALL_ROOT}/sbin/thotd.new" "${INSTALL_ROOT}/sbin/thotd"
install -o root -g root -m 0644 "${temporary_dir}/VERSION" "${INSTALL_ROOT}/share/thot/VERSION"
install -o root -g root -m 0644 "${temporary_dir}/thotd.service" /etc/systemd/system/thotd.service

systemctl daemon-reload
systemctl enable --now thotd
systemctl restart thotd
systemctl --no-pager --full status thotd || fail "Thot was installed but the daemon did not start; inspect systemctl status and journalctl -u thotd"

cat <<EOF

Thot ${installed_version} is installed.
CLI: /usr/local/bin/thot
Daemon: /usr/local/sbin/thotd
Configuration: /etc/thot/thotd.env
Project directory: /etc/thot/projects
Data directory: ${DATA_ROOT}

Before running a project, review /etc/thot/thotd.env and confirm the host firewall
and guest egress policy. Set THOTD_FIREWALL_CONFIRMATION=true only after that review.
EOF
