"""Real Neovim, fake Claude, isolated files/config: no user history or nvim config."""
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
import uuid

REPO = Path(__file__).resolve().parents[1]
BINARY = REPO / "target/debug/claude-code-tui"


class Tui:
    def __init__(self, env):
        self.data = bytearray()
        self.exited = False
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.chdir(REPO)
            os.execve(BINARY, [str(BINARY)], env)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 150, 0, 0))
        self.wait(lambda: "Проекты" in self.text())

    def text(self):
        return "".join(re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", self.data.decode(errors="replace")).split())

    def pump(self):
        if select.select([self.fd], [], [], 0.05)[0]:
            try:
                self.data.extend(os.read(self.fd, 65536))
            except OSError:
                pass

    def wait(self, condition):
        end = time.monotonic() + 8
        while time.monotonic() < end:
            self.pump()
            if condition():
                return
        raise AssertionError("TUI timeout: " + self.text()[-500:])

    def send(self, keys):
        os.write(self.fd, keys)
        for _ in range(4):
            self.pump()

    def finish(self):
        end = time.monotonic() + 5
        while time.monotonic() < end:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.exited = True
                os.close(self.fd)
                assert os.waitstatus_to_exitcode(status) == 0
                return
            self.pump()
        raise AssertionError("TUI did not finish: " + self.text()[-600:])

    def cleanup(self):
        if not self.exited:
            os.kill(self.pid, signal.SIGKILL)
            os.waitpid(self.pid, 0)
            os.close(self.fd)


def records(path):
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
