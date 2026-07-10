#!/usr/bin/env bash
# Deploy: ships the built artifact to the release host. Responsible for upload. NOT concerned with building. | I/O: (artifact) -> exit_code
set -euo pipefail
echo "deploying"
