#!/usr/bin/env bash
# Download per-device test fixtures from GitHub Releases.
#
# Usage:
#   bash scripts/fetch_test_fixtures.sh                          # all devices
#   bash scripts/fetch_test_fixtures.sh sony-ilce-6700           # one device
#   bash scripts/fetch_test_fixtures.sh sony-ilce-6700 apple-*   # multiple
#
# Each device (camera Make/Model) has its own GitHub Release containing a
# tarball with test images and fixture metadata. The manifest at fixtures.json
# pins which devices and versions to download.
#
# To add a new device, add an entry to fixtures.json and re-run this script.

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="${PROJECT_ROOT}/fixtures.json"
VERSIONS_DIR="${PROJECT_ROOT}/test_data/.device-versions"
DOWNLOAD_DIR="${TMPDIR:-/tmp}"

if [[ ! -f "${MANIFEST}" ]]; then
    echo "ERROR: ${MANIFEST} not found" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Parse manifest with python3
# ---------------------------------------------------------------------------
# The manifest is piped in on stdin rather than passed as a path: on Git Bash
# (Windows CI) `${MANIFEST}` is an MSYS path that native Python cannot open.
REPO="$(python3 -c "import json, sys; print(json.load(sys.stdin)['repo'])" < "${MANIFEST}")"

# Get device info as tab-separated lines: slug\tversion\tmake\tmodel
get_devices() {
    python3 -c "
import json, sys
d = json.load(sys.stdin)
for slug, info in sorted(d['devices'].items()):
    print(f\"{slug}\t{info['version']}\t{info['make']}\t{info['model']}\")
" < "${MANIFEST}"
}

# ---------------------------------------------------------------------------
# Filter to requested devices
# ---------------------------------------------------------------------------
ALL_DEVICES="$(get_devices)"

if [[ $# -gt 0 ]]; then
    FILTERED=""
    for arg in "$@"; do
        matched="$(echo "${ALL_DEVICES}" | grep "^${arg}	" || true)"
        if [[ -z "${matched}" ]]; then
            echo "WARNING: device '${arg}' not found in fixtures.json — skipping" >&2
        else
            FILTERED="${FILTERED}${matched}"$'\n'
        fi
    done
    ALL_DEVICES="$(echo "${FILTERED}" | sed '/^$/d')"
fi

if [[ -z "${ALL_DEVICES}" ]]; then
    echo "No devices to fetch."
    exit 0
fi

# ---------------------------------------------------------------------------
# Download helper
# ---------------------------------------------------------------------------
download_asset() {
    local tag="$1" tarball="$2"
    local dest="${DOWNLOAD_DIR}/${tarball}"

    if command -v gh &>/dev/null; then
        gh release download "${tag}" \
            --repo "${REPO}" \
            --pattern "${tarball}" \
            --dir "${DOWNLOAD_DIR}" \
            --clobber
    else
        local url="https://github.com/${REPO}/releases/download/${tag}/${tarball}"
        echo "  (gh CLI not found — using curl)"
        curl -fsSL -o "${dest}" "${url}"
    fi
}

# ---------------------------------------------------------------------------
# Fetch each device
# ---------------------------------------------------------------------------
mkdir -p "${PROJECT_ROOT}/test_data" "${PROJECT_ROOT}/test_fixtures" "${VERSIONS_DIR}"

echo "Fetching test fixtures from ${REPO}..."
echo ""

FETCHED=0
SKIPPED=0

while IFS=$'\t' read -r slug version make model; do
    [[ -z "${slug}" ]] && continue

    tag="device/${slug}/v${version}"
    tarball="rawshift-fixtures-${slug}-v${version}.tar.gz"
    stamp_file="${VERSIONS_DIR}/${slug}"

    # Idempotency: skip if already at this version
    if [[ -f "${stamp_file}" ]] && [[ "$(cat "${stamp_file}")" == "${version}" ]]; then
        echo "  [${slug}] already at v${version} — skipping"
        SKIPPED=$((SKIPPED + 1))
        continue
    fi

    echo "  [${slug}] downloading v${version} (${make}/${model})..."

    # Clean old data to handle file removals between versions
    rm -rf "${PROJECT_ROOT}/test_data/${make}/${model}"
    rm -rf "${PROJECT_ROOT}/test_fixtures/${make}/${model}"

    # Download
    dest="${DOWNLOAD_DIR}/${tarball}"
    if ! download_asset "${tag}" "${tarball}" 2>/dev/null; then
        echo "  [${slug}] not available at ${tag} — skipping"
        continue
    fi

    # Extract
    echo "  [${slug}] extracting..."
    tar -xzf "${dest}" -C "${PROJECT_ROOT}"
    rm -f "${dest}"

    # Write version stamp
    echo "${version}" > "${stamp_file}"
    echo "  [${slug}] done"
    FETCHED=$((FETCHED + 1))

done <<< "${ALL_DEVICES}"

echo ""
echo "Test fixtures ready (${FETCHED} fetched, ${SKIPPED} already up-to-date)."
