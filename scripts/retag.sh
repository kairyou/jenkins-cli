#!/usr/bin/env bash
set -euo pipefail

# Re-push a local tag to retrigger GitHub Actions tag workflows.
# Usage:
#   ./scripts/retag.sh          # re-push the latest reachable tag
#   ./scripts/retag.sh v0.1.27  # re-push a specific tag

tag_name="${1:-$(git describe --tags --abbrev=0 2>/dev/null || true)}"

if [ -z "$tag_name" ]; then
    echo "Error: No Git tag found"
    exit 1
fi

if ! git rev-parse -q --verify "refs/tags/${tag_name}" >/dev/null; then
    echo "Error: Local tag '${tag_name}' does not exist"
    exit 1
fi

echo "Tag: ${tag_name}"
echo
echo "Local tag target:"
git --no-pager show --no-patch --format="  %H %D%n  %s" "${tag_name}^{}"
echo
echo "Remote tag target:"
git ls-remote --tags origin "${tag_name}*" || true
echo
echo "This will delete the remote tag and push the existing local tag again."
read -r -p "Continue? (y/n) " -n 1
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]
then
    echo "Operation cancelled"
    exit 1
fi

git push origin ":refs/tags/${tag_name}" || true
git push origin "refs/tags/${tag_name}:refs/tags/${tag_name}"

echo "Tag ${tag_name} has been pushed again"
