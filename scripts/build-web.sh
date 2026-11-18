#!/bin/sh
# Build the web bundle the same way CI does: trunk release build + preload
# cleanup (Chrome crbug.com/981419) so the console is warning-free.
set -e
cd "$(dirname "$0")/.."
trunk build --release
python3 - <<'PY'
import re
from pathlib import Path
p = Path("dist/index.html")
t = p.read_text()
t, n = re.subn(r'(<link[^>]*rel="(?:preload|modulepreload)"[^>]*?)\s+integrity="[^"]*"', r'\1', t)
p.write_text(t)
print(f"stripped integrity from {n} preload link(s)")
PY
