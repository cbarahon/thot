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

for command_name in curl sha256sum tar install systemctl useradd groupadd getent awk sed grep ldd cat ssh-keygen; do
    command -v "${command_name}" >/dev/null 2>&1 || fail "required command is missing: ${command_name}"
done
[[ -d /run/systemd/system ]] || fail "systemd is required"

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
    $0 !~ /^(thot|thotd|thotd\.service|thotd\.env\.example|thot-virtualbox-agent|thot-alpine-agent|thot-virtualbox-agent\.service|thot-virtualbox-agent\.config\.yaml\.example|VERSION)$/ { bad=1 }
    END { exit bad }
' || fail "release archive contains an unexpected path"
tar -xzf "${temporary_dir}/${ASSET}" -C "${temporary_dir}"
for file_name in thot thotd thotd.service thotd.env.example thot-virtualbox-agent thot-alpine-agent thot-virtualbox-agent.service thot-virtualbox-agent.config.yaml.example VERSION; do
    [[ -f "${temporary_dir}/${file_name}" ]] || fail "release archive is missing ${file_name}"
done
installed_version="$(cat "${temporary_dir}/VERSION")"
[[ "${installed_version}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "release archive has an invalid version file"
for binary in thot thotd thot-virtualbox-agent; do
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
    "${DATA_ROOT}" "${DATA_ROOT}/artifacts" "${DATA_ROOT}/artifacts/sources" "${DATA_ROOT}/artifacts/projects" "${DATA_ROOT}/vms" "${DATA_ROOT}/vbox-profile" "${DATA_ROOT}/vbox-agent/ssh"
install -d -o root -g thotd -m 0750 /etc/thot /etc/thot/projects /etc/thot/virtualbox-agent
install -d -o root -g thotd -m 0770 /srv/thot-projects
install -d -o thotd -g thotd -m 0750 /etc/thot/projects/vms
[[ -d /srv ]] || install -d -o root -g root -m 0755 /srv
install -d -o root -g root -m 0755 "${INSTALL_ROOT}/share/thot"

if [[ ! -e /etc/thot/thotd.env ]]; then
    install -o root -g thotd -m 0640 "${temporary_dir}/thotd.env.example" /etc/thot/thotd.env
fi
if [[ ! -e /etc/thot/thot-virtualbox-agent.config.yaml ]]; then
    install -o root -g thotd -m 0640 "${temporary_dir}/thot-virtualbox-agent.config.yaml.example" /etc/thot/thot-virtualbox-agent.config.yaml
fi

install -o root -g root -m 0755 "${temporary_dir}/thot" "${INSTALL_ROOT}/bin/thot.new"
install -o root -g root -m 0755 "${temporary_dir}/thot-alpine-agent" "${INSTALL_ROOT}/bin/thot-alpine-agent.new"
install -o root -g root -m 0755 "${temporary_dir}/thotd" "${INSTALL_ROOT}/sbin/thotd.new"
install -o root -g root -m 0755 "${temporary_dir}/thot-virtualbox-agent" "${INSTALL_ROOT}/sbin/thot-virtualbox-agent.new"
mv -f "${INSTALL_ROOT}/bin/thot.new" "${INSTALL_ROOT}/bin/thot"
mv -f "${INSTALL_ROOT}/bin/thot-alpine-agent.new" "${INSTALL_ROOT}/bin/thot-alpine-agent"
mv -f "${INSTALL_ROOT}/sbin/thotd.new" "${INSTALL_ROOT}/sbin/thotd"
mv -f "${INSTALL_ROOT}/sbin/thot-virtualbox-agent.new" "${INSTALL_ROOT}/sbin/thot-virtualbox-agent"
install -o root -g root -m 0644 "${temporary_dir}/VERSION" "${INSTALL_ROOT}/share/thot/VERSION"
install -o root -g root -m 0644 "${temporary_dir}/thotd.service" /etc/systemd/system/thotd.service
install -o root -g root -m 0644 "${temporary_dir}/thot-virtualbox-agent.service" /etc/systemd/system/thot-virtualbox-agent.service

systemctl daemon-reload
systemctl enable --now thotd thot-virtualbox-agent
systemctl restart thotd thot-virtualbox-agent
systemctl --no-pager --full status thotd || fail "Thot was installed but the daemon did not start; inspect systemctl status and journalctl -u thotd"
systemctl --no-pager --full status thot-virtualbox-agent || fail "Thot was installed but the VirtualBox agent did not start; inspect systemctl status and journalctl -u thot-virtualbox-agent"

cat <<EOF

Thot ${installed_version} is installed.
CLI: /usr/local/bin/thot
Daemon: /usr/local/sbin/thotd
VirtualBox agent: /usr/local/sbin/thot-virtualbox-agent
Configuration: /etc/thot/thotd.env and /etc/thot/thot-virtualbox-agent.config.yaml
Project directory: /etc/thot/projects
Data directory: ${DATA_ROOT}

Set the SSH host, user, known_hosts entry, and shared-path mappings in
/etc/thot/thot-virtualbox-agent.config.yaml. The agent generates an Ed25519 key on
first start; add its logged public key to the configured VirtualBox host user's
~/.ssh/authorized_keys. Project folders and delivery paths must be visible to both
thotd and the agent account.
EOF
