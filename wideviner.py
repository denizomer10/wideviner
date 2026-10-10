#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""wideviner - Widevine CDM installer for Chromium on Linux."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from wideviner.cli import main

if __name__ == "__main__":
    sys.exit(main())
