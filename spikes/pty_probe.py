#!/usr/bin/env python3
"""Phase 0 probe: run a CLI in a pseudo-terminal, record its raw output, optionally paste one prompt.

Answers the questions in docs/PHASE-0.md (S1, S4, S5) without the app: does a bracketed paste + Enter submit,
which OSC sequences does the agent emit, does it turn bracketed paste on. The raw log is also the input for S7.

    python3 -I spikes/pty_probe.py --log /tmp/claude.bin --paste "Reply with the single word PONG7" \
        --expect PONG7 -- claude

Standard library only. The agent runs for real and spends its normal quota.
"""
import argparse
import fcntl
import os
import pty
import re
import select
import signal
import struct
import sys
import termios
import time

OSC = re.compile(rb"\x1b\](\d+);([^\x07\x1b]*)(?:\x07|\x1b\\)")
MODES = {b"\x1b[?2004h": "bracketed paste ON", b"\x1b[?2004l": "bracketed paste OFF",
         b"\x1b[?1049h": "alternate screen ON", b"\x1b[?1049l": "alternate screen OFF"}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--log", required=True, help="file for the raw output bytes")
    ap.add_argument("--paste", help="text to paste once")
    ap.add_argument("--paste-file", help="read the text to paste from this file")
    ap.add_argument("--paste-at", type=float, default=10.0, help="seconds after start (default 10)")
    ap.add_argument("--enter-delay", type=float, default=0.15, help="seconds between paste end and Enter")
    ap.add_argument("--expect", help="string that should appear in the output after the paste")
    ap.add_argument("--duration", type=float, default=60.0, help="seconds before the program is stopped")
    ap.add_argument("--rows", type=int, default=40)
    ap.add_argument("--cols", type=int, default=120)
    ap.add_argument("cmd", nargs=argparse.REMAINDER, help="-- command [args...]")
    a = ap.parse_args()
    cmd = a.cmd[1:] if a.cmd[:1] == ["--"] else a.cmd
    if not cmd:
        ap.error("missing command after --")
    text = a.paste
    if a.paste_file:
        with open(a.paste_file, encoding="utf-8") as f:
            text = f.read()
    if a.expect and text and a.expect in text:
        ap.error("--expect must not occur in the pasted text: the TUI echoes the paste, so it would always be found")

    pid, fd = pty.fork()
    if pid == 0:  # child: size the terminal before the program reads it, then replace this process
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", a.rows, a.cols, 0, 0))
        os.environ.update(TERM="xterm-256color", COLORTERM="truecolor")
        os.execvp(cmd[0], cmd)

    buf = bytearray()
    paste_offset = None
    start = time.monotonic()
    while time.monotonic() - start < a.duration:
        ready, _, _ = select.select([fd], [], [], 0.05)
        if ready:
            try:
                chunk = os.read(fd, 65536)
            except OSError:  # EIO: the program exited
                break
            if not chunk:
                break
            buf += chunk
            # A real terminal answers these queries; some TUIs wait for the answer before drawing.
            if b"\x1b[6n" in chunk:
                os.write(fd, b"\x1b[1;1R")
            if b"\x1b[c" in chunk or b"\x1b[0c" in chunk:
                os.write(fd, b"\x1b[?62;22c")
        if text is not None and paste_offset is None and time.monotonic() - start >= a.paste_at:
            os.write(fd, b"\x1b[200~" + text.encode() + b"\x1b[201~")
            time.sleep(a.enter_delay)
            os.write(fd, b"\r")
            paste_offset = len(buf)

    stop(pid)
    with open(a.log, "wb") as f:
        f.write(buf)
    report(bytes(buf), paste_offset, text, a.expect)
    return 0


def stop(pid: int) -> None:
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.kill(pid, sig)
        except ProcessLookupError:
            return
        for _ in range(30):
            if os.waitpid(pid, os.WNOHANG)[0]:
                return
            time.sleep(0.1)


def report(buf: bytes, paste_offset, text, expect) -> None:
    events = [(m.start(), f"OSC {m[1].decode()}: {m[2][:100].decode(errors='replace')!r}") for m in OSC.finditer(buf)]
    for seq, label in MODES.items():
        events += [(m.start(), label) for m in re.finditer(re.escape(seq), buf)]
    if paste_offset is not None:
        events.append((paste_offset, f">>> pasted {len(text.encode())} bytes + Enter"))
    print(f"recorded {len(buf)} bytes")
    for offset, label in sorted(events):
        print(f"  @{offset:>8}  {label}")
    if expect:
        after = buf[paste_offset or 0:]
        print(f"expect {expect!r}: {'FOUND' if expect.encode() in after else 'NOT FOUND'} after the paste")


if __name__ == "__main__":
    sys.exit(main())
