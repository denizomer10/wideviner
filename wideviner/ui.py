# SPDX-License-Identifier: MIT
"""Terminal user interface helpers."""

from __future__ import annotations

import os
import sys
from collections.abc import Sequence

_RESET = "\033[0m"
_STYLES = {
    "bold": "\033[1m",
    "dim": "\033[2m",
    "red": "\033[31m",
    "green": "\033[32m",
    "yellow": "\033[33m",
    "blue": "\033[34m",
    "magenta": "\033[35m",
    "cyan": "\033[36m",
}


def human_bytes(num: float) -> str:
    """Format a byte count in a human readable way."""
    for unit in ("B", "KiB", "MiB", "GiB"):
        if abs(num) < 1024.0 or unit == "GiB":
            if unit == "B":
                return f"{int(num)} {unit}"
            return f"{num:.1f} {unit}"
        num /= 1024.0
    return f"{num:.1f} GiB"


class UI:
    """Small helper around the terminal.

    All output goes through this class so that colour handling and quiet mode
    stay in one place.
    """

    def __init__(self, color: bool | None = None, quiet: bool = False) -> None:
        self.color = self._auto_color() if color is None else color
        self.quiet = quiet

    @staticmethod
    def _auto_color() -> bool:
        if os.environ.get("NO_COLOR"):
            return False
        if os.environ.get("TERM") == "dumb":
            return False
        try:
            return sys.stdout.isatty()
        except (AttributeError, OSError, ValueError):
            return False

    # -- formatting --------------------------------------------------------
    def style(self, text: str, *styles: str) -> str:
        if not self.color or not styles:
            return text
        prefix = "".join(_STYLES[s] for s in styles if s in _STYLES)
        return f"{prefix}{text}{_RESET}"

    # -- output ------------------------------------------------------------
    def print(self, text: str = "") -> None:
        if not self.quiet:
            print(text)

    def info(self, text: str) -> None:
        self.print(f"{self.style('[*]', 'cyan')} {text}")

    def ok(self, text: str) -> None:
        self.print(f"{self.style('[+]', 'green')} {text}")

    def warn(self, text: str) -> None:
        self.print(f"{self.style('[!]', 'yellow')} {text}")

    def error(self, text: str) -> None:
        print(f"{self.style('[-]', 'red')} {text}", file=sys.stderr)

    def step(self, text: str) -> None:
        self.print(f"{self.style('==>', 'magenta', 'bold')} {text}")

    def field(self, label: str, value: str) -> None:
        plain = label + ":"
        padding = " " * max(1, 28 - len(plain))
        self.print(f"    {self.style(plain, 'dim')}{padding} {value}")

    # -- input -------------------------------------------------------------
    def prompt(self, text: str, default: str | None = None) -> str:
        suffix = f" [{default}]" if default is not None else ""
        try:
            answer = input(f"{self.style('?', 'cyan')} {text}{suffix}: ").strip()
        except EOFError:
            return default or ""
        return answer or (default or "")

    def confirm(self, text: str, default: bool = False) -> bool:
        hint = "Y/n" if default else "y/N"
        answer = self.prompt(f"{text} [{hint}]").lower()
        if not answer:
            return default
        return answer in ("y", "yes", "e", "evet")

    def press_enter(self, text: str = "Press Enter to continue") -> None:
        try:
            input(f"{self.style('...', 'dim')} {text} ")
        except EOFError:
            pass

    def menu(self, title: str, options: Sequence[str]) -> int:
        """Show a numbered menu and return the 0-based index of the choice.

        Returns -1 when the input is closed (for example when running
        non-interactively), so callers can cancel cleanly.
        """
        while True:
            self.print()
            self.print(self.style(title, "bold"))
            for index, option in enumerate(options, 1):
                self.print(f"  {self.style(f'[{index}]', 'cyan')} {option}")
            self.print()
            try:
                answer = input(f"{self.style('?', 'cyan')} Select an option: ").strip()
            except EOFError:
                return -1
            try:
                choice = int(answer) - 1
            except ValueError:
                self.warn("Please enter a number.")
                continue
            if 0 <= choice < len(options):
                return choice
            self.warn("Invalid option.")

    # -- progress ----------------------------------------------------------
    def progress(self, done: int, total: int, prefix: str = "    ") -> None:
        if self.quiet:
            return
        try:
            if not sys.stdout.isatty():
                return
        except (AttributeError, OSError, ValueError):
            return
        width = 32
        if total > 0:
            fraction = min(done / total, 1.0)
            filled = int(width * fraction)
            bar = "#" * filled + "-" * (width - filled)
            text = (
                f"\r{prefix}[{bar}] {fraction * 100:5.1f}%  "
                f"{human_bytes(done)}/{human_bytes(total)}"
            )
        else:
            text = f"\r{prefix}{human_bytes(done)}"
        sys.stdout.write(text)
        sys.stdout.flush()

    def progress_done(self) -> None:
        if self.quiet:
            return
        try:
            if not sys.stdout.isatty():
                return
        except (AttributeError, OSError, ValueError):
            return
        sys.stdout.write("\n")
        sys.stdout.flush()


def banner(ui: UI) -> None:
    """Print the application banner."""
    from .constants import APP_NAME, APP_TAGLINE, APP_VERSION

    title = f"{APP_NAME} {APP_VERSION}"
    ui.print()
    ui.print(ui.style(f"  {title}", "bold", "cyan"))
    ui.print(ui.style(f"  {APP_TAGLINE}", "dim"))
    ui.print(ui.style("  " + "-" * (len(title) + 2), "dim"))
