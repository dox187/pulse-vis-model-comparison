#!/usr/bin/env python3
"""Exercise the real terminal event loop using only the Python standard library."""
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import tempfile
import termios
import time
import tomllib

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/release/pulse-vis"


def run(config, live=False):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 120, 0, 0))
    before = termios.tcgetattr(slave)
    process = subprocess.Popen(
        [str(BINARY), "--config", str(config), *([] if live else ["--demo"])],
        stdin=slave, stdout=slave, stderr=slave,
        env={**os.environ, "TERM": "xterm-256color"},
    )
    output = bytearray()

    def pump(seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if select.select([master], [], [], 0.02)[0]:
                output.extend(os.read(master, 65536))

    try:
        pump(1)
        assert process.poll() is None
        if live:
            # Direct capture subprocesses must be reaped when SIGTERM arrives.
            children = subprocess.run(["pgrep", "-P", str(process.pid)], capture_output=True, text=True).stdout.split()
            process.send_signal(signal.SIGTERM)
        else:
            children = []
            for key in [b"1", b"2", b"3", b"4", b"5", b"6", b"a", b"\x1b", b"s", b"\x1b[C", b"\x1b", b"Q", b"p", b"w", b"?", b"\x1b", b"q"]:
                os.write(master, key)
                pump(0.08)
        pump(1)
        assert process.wait(timeout=8) == 0, output.decode(errors="replace")
        assert b"\x1b[?1049h" in output and b"\x1b[?1049l" in output
        after = termios.tcgetattr(slave)
        assert before == after, "Terminal attributes were not restored"
        for child in children:
            assert not Path(f"/proc/{child}").exists(), f"Child {child} survived exit"
        if not live:
            saved = tomllib.loads(config.read_text())
            assert saved["mode"] == "radial"
            assert saved["quality"] == "ultra"
            assert saved["theme"] == "fire"
            assert saved["sensitivity"] > 1.0
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        os.close(master)
        os.close(slave)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="pulse-vis-tui-") as directory:
        run(Path(directory) / "config.toml")
        run(Path(directory) / "live.toml", live=True)
    print("PASS: six modes, overlays, settings save, terminal restoration, SIGTERM child cleanup")
