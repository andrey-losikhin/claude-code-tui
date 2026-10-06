from pathlib import Path
exec((Path(__file__).parent / "support.py").read_text())

with tempfile.TemporaryDirectory(prefix='claude-keyboard-') as directory:
    root = Path(directory)
    for folder in ['home', 'config', 'bin', 'project']:
        (root/folder).mkdir()
    config = root/'config/claude-code-tui/config.json'
    config.parent.mkdir()
    config.write_text(json.dumps({'projects': [str(root/'project')]}))
    capture = root/'input'
    fake = root/'bin/claude'
    fake.write_text('''#!/usr/bin/python3
import os, tty
tty.setraw(0)
os.write(1, b'CLI KEYS READY\\x1b[?2004h\\x1b[?1h')
while True:
    data=os.read(0,4096)
    with open(os.environ['KEY_CAPTURE'], 'ab') as f: f.write(data)
''')
    fake.chmod(0o755)
    env = dict(os.environ, HOME=str(root/'home'), XDG_CONFIG_HOME=str(root/'config'),
               PATH=str(root/'bin'), TERM='xterm-256color', KEY_CAPTURE=str(capture))
    tui=Tui(env)
    expected=b''
    try:
        assert b'\x1b[?2004h' in tui.data
        tui.send(b'n\r')
        tui.wait(lambda: 'CLIKEYSREADY' in tui.text())
        for incoming, outgoing in [
            (b'\x1b[13;2u', b'\x1b[13;2u'),
            (b'\x1b[13;6u', b'\x1b[13;6u'),
            (b'\r', b'\r'),
            (b'\n', b'\n'),
            (b'\x1b[1;5D', b'\x1b[1;5D'),
            (b'\x1b[1;6C', b'\x1b[1;6C'),
            (b'\x1b[1;7D', b'\x1b[1;7D'),
            (b'\x1b[120;7u', b'\x1b\x18'),
            (b'\x1b[106;6u', b'\x1b[106;6u'),
            (b'\x1b[3;5~', b'\x1b[3;5~'),
            (b'\x1b[9;6u', b'\x1b[9;6u'),
            (b'\x1b[127;5u', b'\x1b[127;5u'),
            (b'\x1b[A', b'\x1bOA'),
        ]:
            tui.send(incoming)
            expected+=outgoing
            tui.wait(lambda: capture.exists() and len(capture.read_bytes())>=len(expected))
            assert capture.read_bytes()==expected, (incoming, capture.read_bytes(), expected)
        pasted='line one\nстрока два\nq\x1bx'.encode()
        tui.send(b'\x1b[200~'+pasted+b'\x1b[201~')
        expected+=b'\x1b[200~'+pasted+b'\x1b[201~'
        tui.wait(lambda: len(capture.read_bytes())>=len(expected))
        assert capture.read_bytes()==expected
        tui.send(b'\x1b[1;3D')
        tui.send(b'f')
        tui.send(b'\x1b[200~SEARCHPASTE\x1b[201~')
        tui.wait(lambda: 'SEARCHPASTE' in tui.text())
        tui.send(b'\x1b')
        tui.send(b'\x1b[1;3C')
        tui.send(b'z')
        expected+=b'z'
        tui.wait(lambda: len(capture.read_bytes())>=len(expected))
        assert capture.read_bytes()==expected
        tui.send(b'\x1bq')
        tui.finish()
        assert b'\x1b[?2004l' in tui.data
        print('PASS: Shift/Ctrl+Enter, modified navigation/editing, Ctrl+Shift/Alt letters, application cursor, bracketed multiline paste, UI paste routing and cleanup')
    finally:
        tui.cleanup()
