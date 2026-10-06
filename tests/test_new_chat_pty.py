"""Exercise the actual TUI with an isolated config and a fake Claude executable."""
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import tempfile
import termios
import time

REPO = Path(__file__).resolve().parents[1]
BINARY = REPO / "target/debug/claude-code-tui"


class Tui:
    def __init__(self, environment):
        self.data = bytearray()
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.chdir(REPO)
            os.execve(BINARY, [str(BINARY)], environment)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", 35, 150, 0, 0))
        self.wait(lambda: "Проекты".encode() in self.data)

    def pump(self):
        if select.select([self.fd], [], [], 0.05)[0]:
            try:
                self.data.extend(os.read(self.fd, 65536))
            except OSError:
                pass

    def wait(self, predicate):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            self.pump()
            if predicate():
                return
        raise AssertionError("Timed out waiting for TUI state")

    def send(self, keys):
        os.write(self.fd, keys)
        for _ in range(3):
            self.pump()

    def text(self):
        text = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", self.data.decode(errors="replace"))
        return "".join(text.split())

    def close(self):
        self.send(b"\x1bq")
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                os.close(self.fd)
                assert os.waitstatus_to_exitcode(status) == 0
                return
            self.pump()
        os.kill(self.pid, signal.SIGKILL)
        os.waitpid(self.pid, 0)
        os.close(self.fd)
        raise AssertionError("TUI did not shut down")


with tempfile.TemporaryDirectory(prefix="claude-tui-picker-smoke-") as scratch:
    root = Path(scratch)
    for name in ("bin", "config", "home", "a-one", "b-two", "z-new folder"):
        (root / name).mkdir()
    one, two, extra = (root / name for name in ("a-one", "b-two", "z-new folder"))
    config = root / "config/claude-code-tui/config.json"
    config.parent.mkdir()
    config.write_text(json.dumps({"projects": [str(one), str(two)],
                                  "hidden_projects": [str(extra)],
                                  "collapsed_projects": [str(extra)]}))
    launches, inputs = root / "launches.jsonl", root / "inputs.bin"
    fake = root / "bin/claude"
    fake.write_text("""#!/usr/bin/python3
import json, os, sys, tty
tty.setraw(sys.stdin.fileno())
with open(os.environ['PICKER_SMOKE_LAUNCHES'], 'a') as log:
    log.write(json.dumps({'cwd': os.getcwd()}) + '\\n')
print('fake claude ready', flush=True)
while True:
    data = os.read(sys.stdin.fileno(), 1024)
    if not data:
        break
    with open(os.environ['PICKER_SMOKE_INPUTS'], 'ab') as log:
        log.write(data)
""")
    fake.chmod(0o700)
    environment = dict(os.environ, HOME=str(root / "home"),
                       XDG_CONFIG_HOME=str(root / "config"), TERM="xterm-256color",
                       PATH=str(root / "bin"), PICKER_SMOKE_LAUNCHES=str(launches),
                       PICKER_SMOKE_INPUTS=str(inputs))

    def records():
        return [json.loads(line) for line in launches.read_text().splitlines()] if launches.exists() else []

    tui = Tui(environment)
    try:
        tui.send(b"n\r")
        tui.wait(lambda: len(records()) == 1)
        assert records()[0]["cwd"] == str(one)
        # Sidebar selection changes; the active session's cwd still wins.
        tui.send(b"\x1b[1;3D\x1b[B\x1bn\r")
        tui.wait(lambda: len(records()) == 2)
        assert records()[1]["cwd"] == str(one)
        # Russian Alt+Т opens from dialogue; repeated Alt+N must not reset selection.
        tui.send("\x1bт".encode())
        tui.send(b"\x1b[B\x1bn\r")
        tui.wait(lambda: len(records()) == 3)
        assert records()[2]["cwd"] == str(two)
        # Open from session pane, browse up, choose another directory with spaces.
        tui.send(b"\x1b[1;3D\x1b[1;3B")
        tui.send("\x1bт".encode())
        tui.send(b"\x1b[F\r\x7f\x1b[F\r\x1b[H\r")
        tui.wait(lambda: len(records()) == 4)
        assert records()[3]["cwd"] == str(extra)
        saved = json.loads(config.read_text())
        assert str(extra) in saved["projects"]
        assert str(extra) not in saved["hidden_projects"]
        assert str(extra) not in saved["collapsed_projects"]
        assert not inputs.exists(), "Modal input leaked into a Claude PTY"
        tui.send(b"\x1bn\x1b")
        tui.send(b"ok")
        tui.wait(lambda: inputs.exists() and inputs.read_bytes() == b"ok")
    finally:
        tui.close()
    print("PASS: n, Alt+N/Т, active default, modal interception, browsing, cwd, persistence, cancellation")

    # Missing executable: leave the dialog available and do not persist its folder.
    environment["PATH"] = str(root / "missing-bin")
    unsaved = root / "z-unsaved"
    unsaved.mkdir()
    before = config.read_bytes()
    tui = Tui(environment)
    try:
        tui.send(b"n\x1b[F\r\x7f\x1b[F\r\x1b[H\r")
        try:
            tui.wait(lambda: "НеудалосьоткрытьClaudeCode" in tui.text())
        except AssertionError:
            print(re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", tui.data.decode(errors="replace"))[-3000:])
            raise
        assert len(records()) == 4
        assert config.read_bytes() == before
        marker = len(tui.data)
        tui.send(b"\x1b")
        tui.wait(lambda: len(tui.data) > marker)
    finally:
        tui.close()
    print("PASS: CLI launch failure preserves config and leaves the chooser cancellable")

    # A corrupt config must not be overwritten, and the successful spawn must warn.
    environment["PATH"] = str(root / "bin")
    config.write_text("{invalid")
    tui = Tui(environment)
    try:
        tui.send(b"n\r\x7f\x1b[F\r\x1b[H\r")
        tui.wait(lambda: len(records()) == 5)
        assert records()[4]["cwd"] == str(unsaved)
        tui.wait(lambda: "Настройкинесохранены" in tui.text())
        assert config.read_text() == "{invalid"
    finally:
        tui.close()
    print("PASS: successful CLI spawn reports persistence failure and preserves corrupt config")
