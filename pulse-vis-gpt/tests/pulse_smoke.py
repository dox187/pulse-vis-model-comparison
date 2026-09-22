#!/usr/bin/env python3
"""Optional integration test against a live PulseAudio-compatible server."""
import json
import math
import os
from pathlib import Path
import re
import struct
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/release/pulse-vis"


def pactl(*args):
    return subprocess.check_output(["pactl", *args], text=True, timeout=5).strip()


def main():
    token = f"pulse_vis_test_{os.getpid()}"
    players = []
    module = None
    try:
        module = pactl("load-module", "module-null-sink", f"sink_name={token}")
        with tempfile.TemporaryDirectory(prefix="pulse-vis-test-") as tmp:
            config = Path(tmp) / "config.toml"
            config.write_text("auto_gain = false\n")
            for name, amplitude, frequency in [("quiet", 0.1, 440), ("loud", 0.5, 1000)]:
                path = Path(tmp) / f"{name}.raw"
                # Twenty seconds provides enough data for all capture checks.
                with path.open("wb") as stream:
                    second = b"".join(struct.pack("<ff", *(2 * [amplitude * math.sin(2 * math.pi * frequency * i / 48000)])) for i in range(48000))
                    for _ in range(20):
                        stream.write(second)
                players.append(subprocess.Popen([
                    "pacat", "--playback", "--raw", "--format=float32le", "--rate=48000", "--channels=2",
                    f"--device={token}", f"--client-name={token}_{name}",
                    f"--property=application.name={token}_{name}", str(path),
                ], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE))
            time.sleep(0.5)
            inputs = json.loads(pactl("--format=json", "list", "sink-inputs"))
            assert sum(token in i.get("properties", {}).get("application.name", "") for i in inputs) == 2

            def check(*args):
                output = subprocess.check_output([str(BINARY), "--config", str(config), "--check", "2", *args], text=True, timeout=10)
                print(output)
                assert "audio detected" in output
                assert int(re.search(r"Samples received: (\d+)", output)[1]) > 0
                return float(re.search(r"Peak: ([\d.-]+)", output)[1])

            quiet = check("--app", f"{token}_quiet")
            loud = check("--app", f"{token}_loud")
            mixed = check("--sink", token)
            combined = check("--app", f"{token}_quiet", "--app", f"{token}_loud")
            assert abs(quiet - (-20.0)) < 1.0, quiet
            assert abs(loud - (-6.02)) < 1.0, loud
            assert mixed > loud + 0.5, (mixed, loud)
            assert combined > loud + 0.5, (combined, loud)
            print("PASS: isolated application capture, output mix, and multi-app capture")
    finally:
        for player in players:
            player.terminate()
            try:
                player.communicate(timeout=3)
            except subprocess.TimeoutExpired:
                player.kill()
                player.communicate()
        if module is not None:
            pactl("unload-module", module)


if __name__ == "__main__":
    main()
