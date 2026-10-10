# SPDX-License-Identifier: MIT
"""Command line interface and interactive flows."""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

from . import chromium, firefox, sysinfo, widevine
from .constants import APP_NAME, APP_VERSION, MOZILLA_PLUGIN_DIRS
from .sysinfo import SystemInfo
from .ui import UI, banner


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog=APP_NAME,
        description=(
            "Install the Widevine CDM into Chromium and Firefox on Linux and "
            "Windows."
        ),
    )
    parser.add_argument(
        "--version", action="version", version=f"{APP_NAME} {APP_VERSION}"
    )
    parser.add_argument(
        "--install", action="store_true", help="install Widevine without the menu"
    )
    parser.add_argument(
        "--update", action="store_true", help="update Widevine without the menu"
    )
    parser.add_argument(
        "--info", action="store_true", help="only print system information"
    )
    parser.add_argument(
        "--target", metavar="PATH", help="browser binary or directory to use"
    )
    parser.add_argument(
        "--cache-dir", metavar="PATH", help="where to keep the downloaded package"
    )
    parser.add_argument(
        "--deep", action="store_true", help="scan the usual roots for browsers"
    )
    parser.add_argument(
        "--refresh", action="store_true", help="ignore the cached package"
    )
    parser.add_argument(
        "--source",
        choices=("auto", "deb", "mozilla"),
        default="auto",
        help="which CDM source to use (default: auto)",
    )
    parser.add_argument(
        "--user", action="store_true", help="install for the current user only"
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="show what would be done"
    )
    parser.add_argument(
        "--yes", action="store_true", help="answer yes to all questions"
    )
    parser.add_argument(
        "--no-color", action="store_true", help="disable coloured output"
    )
    parser.add_argument("--quiet", action="store_true", help="only print errors")
    return parser


# ---------------------------------------------------------------------------
# Output helpers
# ---------------------------------------------------------------------------
def print_system_info(ui: UI, si: SystemInfo) -> None:
    ui.step("System information")
    for label, value in sysinfo.report(si):
        ui.field(label, value)


def _can_prompt() -> bool:
    try:
        return sys.stdin.isatty()
    except (AttributeError, OSError, ValueError):
        return False


def _reexec_with_sudo() -> None:
    """Re-run wideviner through sudo, keeping the original arguments."""
    script = Path(sys.argv[0])
    try:
        resolved = script.resolve()
    except OSError:
        resolved = script
    if resolved.is_file():
        command = ["sudo", sys.executable, str(resolved), *sys.argv[1:]]
    else:
        command = ["sudo", sys.executable, "-m", "wideviner", *sys.argv[1:]]
    try:
        os.execvp("sudo", command)
    except OSError as exc:
        print(f"could not run sudo: {exc}", file=sys.stderr)


def _state_text(target) -> str:
    return target.widevine_version or "no widevine"


def _show_target(ui: UI, target) -> None:
    ui.ok(f"{target.kind} found: {target.binary}")
    ui.field("browser", target.describe())
    ui.field("version", target.version or "unknown")
    ui.field("source", target.source)
    ui.field("widevine", target.widevine_version or "not installed")
    if getattr(target, "user_data_dir", None) is not None:
        ui.field("profile", str(target.user_data_dir))
    for note in target.notes:
        ui.field("note", note)


# ---------------------------------------------------------------------------
# Flows
# ---------------------------------------------------------------------------
def _scan(ui: UI, si: SystemInfo, args: argparse.Namespace) -> list:
    ui.step("Scanning the system for browsers ...")
    targets: list = []
    targets.extend(chromium.discover(si, deep=args.deep, target=args.target))
    targets.extend(firefox.discover(si, deep=args.deep, target=args.target))
    if not targets:
        ui.warn("no supported browser found.")
        ui.press_enter("Press Enter to continue")
        return []
    return targets


def _select_browser(ui: UI, targets: list, args: argparse.Namespace):
    if len(targets) == 1:
        return targets[0]

    # Non-interactive runs cannot answer a menu, so use the first match.
    if args.yes or not _can_prompt():
        ui.info(f"Using {targets[0].binary}")
        return targets[0]

    ui.print()
    ui.info(f"Found {len(targets)} browsers:")
    labels = []
    for target in targets:
        labels.append(f"{target.describe()}  ({_state_text(target)})")
    labels.append("Cancel")
    choice = ui.menu("Which browser should be used?", labels)
    if choice < 0 or choice == len(labels) - 1:
        return None
    return targets[choice]


def _cache_dir(args: argparse.Namespace) -> Path | None:
    return Path(args.cache_dir).expanduser() if args.cache_dir else None


def _perform_install(
    ui: UI,
    si: SystemInfo,
    args: argparse.Namespace,
    target,
    payload: widevine.CdmPayload,
) -> bool:
    owner = None
    invoking = sysinfo.invoking_user()
    if invoking is not None:
        owner = (invoking[1], invoking[2])

    if target.kind == "firefox":
        result = firefox.install(
            target,
            payload,
            si,
            ui,
            dry_run=args.dry_run,
            user_only=args.user,
            owner=owner,
        )
    else:
        result = widevine.install(
            target,
            payload,
            si,
            ui,
            dry_run=args.dry_run,
            user_only=args.user,
            owner=owner,
        )

    if not result.ok:
        ui.error(result.message)
        return False

    if args.dry_run:
        ui.info(f"Dry run: would install Widevine {result.version} to {result.path}")
        return True

    ui.print()
    ui.ok(f"Successfully installed Widevine {result.version}")
    ui.field("location", str(result.path))
    if result.hint_file is not None:
        label = "prefs" if target.kind == "firefox" else "hint file"
        ui.field(label, str(result.hint_file))
    ui.print()
    if target.kind == "firefox":
        ui.info(
            "Restart Firefox, then check about:support under 'Media' for the "
            "Widevine Content Decryption Module."
        )
    else:
        ui.info(
            "Restart the browser and check chrome://components for "
            "'Widevine Content Decryption Module'."
        )
    return True


def _needs_admin(target, si: SystemInfo) -> bool:
    """Check whether a system wide installation would need elevation."""
    if target.kind == "firefox":
        if not si.is_linux:
            return False  # the profile based install never needs elevation
        for base in MOZILLA_PLUGIN_DIRS:
            candidate = Path(base)
            if candidate.is_dir() and os.access(candidate, os.W_OK):
                return False
        return True
    return not target.writable


def _maybe_elevate(
    ui: UI, si: SystemInfo, args: argparse.Namespace, target, action: str
) -> bool:
    """Offer to re-run through sudo when a system wide install is needed."""
    if args.user or si.is_admin or not _needs_admin(target, si):
        return False
    if (
        si.has_sudo
        and _can_prompt()
        and not args.yes
        and ui.confirm(
            f"Administrator rights are required to {action} the browser "
            "directory. Re-run with sudo?",
            default=True,
        )
    ):
        _reexec_with_sudo()
        return True
    ui.warn(f"{action.capitalize()}ing for the current user only.")
    return False


def flow_install(ui: UI, si: SystemInfo, args: argparse.Namespace) -> bool:
    targets = _scan(ui, si, args)
    if not targets:
        return False

    target = _select_browser(ui, targets, args)
    if target is None:
        return False

    _show_target(ui, target)

    if not args.yes and not ui.confirm("Continue?", default=True):
        return False

    if target.kind == "chromium":
        supported = widevine.build_supports_widevine(target.executable)
        if supported is False:
            ui.warn("This browser build does not appear to include Widevine support.")
            ui.warn("The CDM will be installed, but the browser may not load it.")

    _maybe_elevate(ui, si, args, target, "install")

    try:
        payload = widevine.fetch_cdm(
            si,
            ui,
            _cache_dir(args),
            refresh=args.refresh,
            sources=None if args.source == "auto" else [args.source],
        )
    except widevine.WidevineError as exc:
        ui.error(str(exc))
        return False

    ui.ok(
        f"Widevine {payload.version or 'unknown'} obtained from Google "
        f"({payload.source} download)"
    )
    return _perform_install(ui, si, args, target, payload)


def flow_update(ui: UI, si: SystemInfo, args: argparse.Namespace) -> bool:
    targets = _scan(ui, si, args)
    if not targets:
        return False

    target = _select_browser(ui, targets, args)
    if target is None:
        return False

    _show_target(ui, target)
    installed = target.widevine_version
    _maybe_elevate(ui, si, args, target, "update")

    try:
        payload = widevine.fetch_cdm(
            si,
            ui,
            _cache_dir(args),
            refresh=args.refresh,
            sources=None if args.source == "auto" else [args.source],
        )
    except widevine.WidevineError as exc:
        ui.error(str(exc))
        return False

    ui.field("available widevine", payload.version or "unknown")

    if (
        installed
        and payload.version
        and widevine.version_key(payload.version) <= widevine.version_key(installed)
    ):
        ui.ok(f"Widevine is already up to date ({installed}).")
        return True

    if not args.yes and not ui.confirm(
        f"Update Widevine to {payload.version or 'the latest version'}?", default=True
    ):
        return False

    return _perform_install(ui, si, args, target, payload)


def interactive(ui: UI, si: SystemInfo, args: argparse.Namespace) -> int:
    while True:
        choice = ui.menu(
            "Main menu",
            [
                "Install Widevine",
                "Update Widevine",
                "System information",
                "Exit",
            ],
        )
        if choice < 0:
            ui.print()
            return 0
        if choice == 0:
            flow_install(ui, si, args)
        elif choice == 1:
            flow_update(ui, si, args)
        elif choice == 2:
            print_system_info(ui, si)
        else:
            ui.print()
            ui.print("Bye.")
            return 0


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------
def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    ui = UI(color=False if args.no_color else None, quiet=args.quiet)
    si = sysinfo.detect()

    banner(ui)
    print_system_info(ui, si)

    if not si.arch_supported:
        ui.error(
            f"Unsupported platform: {si.os_name or 'unknown'} / "
            f"{si.machine or 'unknown'}."
        )
        ui.error(
            "wideviner supports x86_64 and ARM64 on Linux and x86_64, ARM64 "
            "and x86 on Windows."
        )
        return 2

    if args.info:
        return 0

    try:
        if args.install:
            return 0 if flow_install(ui, si, args) else 1
        if args.update:
            return 0 if flow_update(ui, si, args) else 1
        return interactive(ui, si, args)
    except KeyboardInterrupt:
        ui.print()
        ui.warn("Interrupted.")
        return 130
