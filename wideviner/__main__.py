# SPDX-License-Identifier: MIT
"""Allow running wideviner with ``python3 -m wideviner``."""

import sys

from .cli import main

if __name__ == "__main__":
    sys.exit(main())
