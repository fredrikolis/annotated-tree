#!/usr/bin/env bash
# Concern: ships the built artifact to the release host | Non-concern: building the artifact (the build step owns that) | IO: (artifact) -> exit_code
set -euo pipefail
echo "deploying"
