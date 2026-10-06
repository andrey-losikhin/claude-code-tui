from pathlib import Path
exec((Path(__file__).parent / "support.py").read_text())
with tempfile.TemporaryDirectory(prefix="claude-tui-notes-") as scratch:
    root = Path(scratch)
    for name in ("bin", "home", "config", "data", "project"):
        (root / name).mkdir()
    launches, editors, sizes, inputs = (root / name for name in ("launches", "editors", "sizes", "inputs"))
    config = root / "config/claude-code-tui/config.json"
    config.parent.mkdir()
    config.write_text(json.dumps({"projects": [str(root / "project")]}))
    fake = root / "bin/claude"
    fake.write_text("""#!/usr/bin/python3
import fcntl, json, os, pathlib, signal, struct, sys, termios, tty
tty.setraw(sys.stdin.fileno())
arg = next(arg for arg in sys.argv if arg.startswith(('--session-id=', '--resume=')))
id = arg.split('=', 1)[1]
with open(os.environ['NOTES_SMOKE_LAUNCHES'], 'a') as log:
    log.write(json.dumps({'id': id, 'argument': arg, 'cwd': os.getcwd()}) + '\\n')
history = pathlib.Path(os.environ['HOME']) / '.claude/projects/test'
history.mkdir(parents=True, exist_ok=True)
(history / (id + '.jsonl')).write_text(json.dumps({'sessionId': id, 'cwd': os.getcwd(), 'aiTitle': 'Chat ' + id}) + '\\n')
def size(*_):
    rows, cols, _, _ = struct.unpack('HHHH', fcntl.ioctl(0, termios.TIOCGWINSZ, b'\\0' * 8))
    with open(os.environ['NOTES_SMOKE_SIZES'], 'a') as log:
        log.write(json.dumps({'id': id, 'rows': rows, 'cols': cols}) + '\\n')
signal.signal(signal.SIGWINCH, size)
size()
print('fake claude ready', flush=True)
while True:
    data = os.read(0, 1024)
    if not data: break
    with open(os.environ['NOTES_SMOKE_INPUTS'], 'ab') as log: log.write(data)
""")
    fake.chmod(0o700)
    wrapper = root / "bin/nvim"
    wrapper.write_text("""#!/usr/bin/python3
import json, os, sys
with open(os.environ['NOTES_SMOKE_EDITORS'], 'a') as log:
    log.write(json.dumps({'file': sys.argv[-1], 'pid': os.getpid()}) + '\\n')
os.execv('/usr/bin/nvim', ['nvim', '--clean', '-i', 'NONE', '-n', *sys.argv[1:]])
""")
    wrapper.chmod(0o700)
    env = dict(os.environ, HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"),
               XDG_DATA_HOME=str(root / "data"), TERM="xterm-256color", PATH=str(root / "bin"),
               NOTES_SMOKE_LAUNCHES=str(launches), NOTES_SMOKE_EDITORS=str(editors),
               NOTES_SMOKE_SIZES=str(sizes), NOTES_SMOKE_INPUTS=str(inputs))
    notes_root = root / "home/knowledge-base/claude-code-tui/notes"
    tui = Tui(env)
    try:
        tui.send(b"n\r")
        tui.wait(lambda: len(records(launches)) == 1)
        first = records(launches)[0]['id']
        assert uuid.UUID(first).version == 4
        assert records(launches)[0]['argument'] == '--session-id=' + first
        first_file = notes_root / (first + '.md')
        initial_rows = records(sizes)[0]['rows']
        tui.send(b"\x1bm")
        tui.wait(lambda: len(records(editors)) == 1 and 'Notes' in tui.text())
        tui.wait(lambda: any(item['id'] == first and item['rows'] < initial_rows for item in records(sizes)))
        tui.send(b"Go")
        tui.send(b"\x1b[200~My note\x1b[201~")
        tui.send(b"\x1b:w\r")
        tui.wait(lambda: 'My note' in first_file.read_text())
        tui.send(b"\x1bm")
        tui.send("\x1bь".encode())
        assert len(records(editors)) == 1
        tui.send(b"GoUNSAVED\x1b")
        assert 'UNSAVED' not in first_file.read_text()
        tui.send(b"\x1bm\x1bm")
        tui.send(b":w\r")
        tui.wait(lambda: 'UNSAVED' in first_file.read_text())
        assert len(records(editors)) == 1
        tui.send(b"\x1b[1;3A")  # Focus Claude, then click the note pane.
        tui.send(b"\x1b[<0;70;26M\x1b[<0;70;26m")
        tui.send(b"GoMOUSE NOTE\x1b:w\r")
        tui.wait(lambda: 'MOUSE NOTE' in first_file.read_text())
        tui.send(b"\x1bn\r")
        tui.wait(lambda: len(records(launches)) == 2)
        second = records(launches)[1]['id']
        assert second != first
        second_file = notes_root / (second + '.md')
        tui.send(b"\x1bm")
        tui.wait(lambda: len(records(editors)) == 2)
        tui.send(b"GoSECOND\x1b:w\r")
        tui.wait(lambda: 'SECOND' in second_file.read_text())
        tui.send(b"\x1b[1;3D\x1b[A\x1b[1;3C")
        tui.send(b"GoFIRST AGAIN\x1b:w\r")
        tui.wait(lambda: 'FIRST AGAIN' in first_file.read_text())
        assert 'FIRST AGAIN' not in second_file.read_text()
        assert len(records(editors)) == 2
        assert not inputs.exists(), 'Editor keys leaked to Claude'
        tui.send(b"GoEXIT SAVE\x1b")
        tui.send(b"\x1bm\x1bx\x1bq")
        tui.wait(lambda: 'Savechanges' in tui.text())
        assert 'EXIT SAVE' not in first_file.read_text()
        assert os.waitpid(tui.pid, os.WNOHANG) == (0, 0)
        tui.send(b"y")
        tui.finish()
        assert 'EXIT SAVE' in first_file.read_text()
    finally:
        tui.cleanup()
    print('PASS: real nvim, stable UUID, split/resize, per-chat files, hide/reuse, Russian keys, safe save-on-exit')

    tui = Tui(env)
    try:
        tui.wait(lambda: '✎' in tui.text())
        tui.send(b"\x1b[B\r")
        tui.wait(lambda: len(records(launches)) == 3)
        resumed = records(launches)[2]['id']
        saved_file = notes_root / (resumed + '.md')
        saved = saved_file.read_bytes()
        assert records(launches)[2]['argument'] == '--resume=' + resumed
        tui.send(b"\x1bm")
        tui.wait(lambda: len(records(editors)) == 3)
        assert records(editors)[2]['file'] == str(saved_file)
        assert saved_file.read_bytes() == saved
        tui.send(b":wq\r")
        tui.send(b"\x1bq")
        tui.finish()
    finally:
        tui.cleanup()
    print('PASS: restart history markers, resumed session association, existing Markdown preservation, :wq')

    # Two dirty editors: refuse saving the first and save the second; wait for both.
    before_first, before_second = first_file.read_bytes(), second_file.read_bytes()
    tui = Tui(env)
    try:
        tui.send(b'\x1b[B\r')
        tui.wait(lambda: len(records(launches)) == 4)
        selected = records(launches)[3]['id']
        tui.send(b'\x1bm')
        tui.wait(lambda: len(records(editors)) == 4)
        tui.send(b'GoDISCARD ME\x1b')
        # Resume the other chat from history, keeping the first dirty note alive.
        tui.send(b'\x1b[1;3A\x1b[1;3D\x1b[B\r')
        tui.wait(lambda: len(records(launches)) == 5)
        other = records(launches)[4]['id']
        assert other != selected
        tui.send(b'\x1bm')
        tui.wait(lambda: len(records(editors)) == 5)
        tui.send(b'GoKEEP ME\x1b')
        tui.send(b'\x1bq')
        tui.wait(lambda: 'Savechanges' in tui.text())
        tui.send(b'n')
        tui.wait(lambda: not alive(records(editors)[3]['pid']))
        tui.send(b'')
        assert os.waitpid(tui.pid, os.WNOHANG) == (0, 0)
        tui.send(b'y')
        tui.finish()
        discarded = notes_root / (selected + '.md')
        kept = notes_root / (other + '.md')
        assert discarded.read_bytes() == (before_first if selected == first else before_second)
        assert 'KEEP ME' in kept.read_text()
        assert 'DISCARD ME' not in discarded.read_text()
    finally:
        tui.cleanup()
    print('PASS: multiple dirty editors, discard one/save the other, exit waits for all')

    wrapper.unlink()
    tui = Tui(env)
    try:
        tui.send(b"\x1b[B\r")
        tui.wait(lambda: len(records(launches)) == 6)
        tui.send(b"\x1bm")
        tui.wait(lambda: 'Unabletospawnnvimbecause' in tui.text())
        tui.send(b"hello")
        tui.wait(lambda: inputs.exists() and inputs.read_bytes() == b'hello')
        tui.send(b"\x1bq")
        tui.finish()
    finally:
        tui.cleanup()
    print('PASS: missing nvim shows an error and leaves the Claude pane usable')
