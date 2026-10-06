from pathlib import Path
exec((Path(__file__).parent / "support.py").read_text())

def mouse(x, y, button=0, up=False):
    return f'\x1b[<{button};{x};{y}{"m" if up else "M"}'.encode()

with tempfile.TemporaryDirectory(prefix='claude-input-mouse-') as scratch:
    root = Path(scratch)
    for folder in ('home', 'config', 'bin', 'project'):
        (root / folder).mkdir()
    config = root / 'config/claude-code-tui/config.json'
    config.parent.mkdir()
    config.write_text(json.dumps({'projects': [str(root / 'project')]}))
    launches, inputs = root / 'launches', root / 'inputs'
    fake = root / 'bin/claude'
    fake.write_text('''#!/usr/bin/python3
import os, sys, tty, json
tty.setraw(0)
id = next(a.split('=', 1)[1] for a in sys.argv if a.startswith('--session-id='))
with open(os.environ['LAUNCHES'], 'a') as f: f.write(json.dumps({'id':id, 'pid':os.getpid()}) + '\\n')
os.write(1, b'CLI READY\\x1b[?1002h\\x1b[?1006h')
while True:
    data = os.read(0, 4096)
    with open(os.environ['INPUTS'], 'a') as f: f.write(json.dumps({'id':id, 'hex':data.hex()}) + '\\n')
    if b'\\x03' in data: break
''')
    fake.chmod(0o755)
    env = dict(os.environ, HOME=str(root/'home'), XDG_CONFIG_HOME=str(root/'config'),
               TERM='xterm-256color', PATH=str(root/'bin'), LAUNCHES=str(launches), INPUTS=str(inputs))
    tui = Tui(env)
    try:
        assert b'\x1b[?1006h' in tui.data
        tui.send(b'n\r')
        tui.wait(lambda: len(records(launches)) == 1)
        first = records(launches)[0]['id']
        # Global arrows must not reach the live CLI.
        tui.send(b'\x1b[1;3D\x1b[1;3B\x1b[1;3A\x1b[1;3C')
        tui.send(b'\x1b[1;3D')  # Alt+Left -> projects
        tui.send(b'fSEARCHMARK')
        tui.wait(lambda: 'SEARCHMARK' in tui.text())
        tui.send(b'\x1b')
        tui.send(b'\x1b[1;3C')  # Alt+Right -> dialogue
        tui.send(b'x')
        tui.wait(lambda: any(r['hex'] == '78' for r in records(inputs)))
        assert b''.join(bytes.fromhex(r['hex']) for r in records(inputs)) == b'x'
        tui.send(b'\x1bn\r')
        tui.wait(lambda: len(records(launches)) == 2)
        second = records(launches)[1]['id']
        # Bottom-left open-session list starts at terminal row 35 in 40x150.
        tui.send(mouse(5,35))
        tui.send(mouse(70,3))
        tui.send(b'a')
        tui.wait(lambda: any(r['id']==first and r['hex']=='61' for r in records(inputs)))
        tui.send(mouse(5,36))
        tui.send(mouse(70,3))
        tui.send(b'b')
        tui.wait(lambda: any(r['id']==second and r['hex']=='62' for r in records(inputs)))
        # Dialogue starts at column 49, inner column 50: x=70 becomes local x=21.
        tui.wait(lambda: any(r['id']==second and bytes.fromhex(r['hex']).startswith(b'\x1b[<0;21;2M') for r in records(inputs)))
        tui.send(mouse(70,3,64))
        tui.wait(lambda: any(r['id']==second and bytes.fromhex(r['hex'])==b'\x1b[<64;21;2M' for r in records(inputs)))
        # Native Ctrl+C ends CLI; plain n now opens picker from projects focus.
        tui.send(b'\x03')
        tui.wait(lambda: 'завершён' in tui.text())
        assert alive(records(launches)[0]['pid']), 'Other chat was killed'
        assert not alive(records(launches)[1]['pid']), 'Exited chat is still alive'
        # Old second row is gone; it cannot receive input or be reactivated.
        tui.send(mouse(5,36))
        tui.send(mouse(70,3))
        tui.send(b'z')
        assert not any(r['hex'] == '7a' for r in records(inputs))
        tui.send(mouse(5,35))
        tui.send(mouse(70,3))
        tui.send(b'k')
        tui.wait(lambda: any(r['id']==first and r['hex']=='6b' for r in records(inputs)))
        tui.send(b'\x1b[1;3D')
        tui.send(b'n\r')
        tui.wait(lambda: len(records(launches)) == 3)
        tui.send('\x1bт\r'.encode())  # Russian Alt+N while CLI is active
        tui.wait(lambda: len(records(launches)) == 4)
        tui.send('\x1bч'.encode())  # Russian Alt+X closes only current chat
        assert not alive(records(launches)[3]['pid'])
        assert alive(records(launches)[0]['pid']) and alive(records(launches)[2]['pid'])
        assert os.waitpid(tui.pid, os.WNOHANG) == (0, 0)
        tui.send(mouse(5,37))  # Former fourth chat is gone from open-session rows
        tui.send(mouse(70,3))
        tui.send(b'z')
        assert not any(r['hex'] == '7a' for r in records(inputs))
        tui.send(b'\x1b[1;3D')
        tui.send(b'n')
        tui.send(b'\r')
        tui.wait(lambda: len(records(launches)) == 5)
        # No navigation/new-chat bytes were forwarded to any CLI.
        allowed = {b'x', b'a', b'b', b'k', b'\x03'}
        for record in records(inputs):
            data = bytes.fromhex(record['hex'])
            assert data in allowed or data.startswith(b'\x1b[<'), data
        tui.send('\x1bй'.encode())
        tui.finish()
        assert b'\x1b[?1006l' in tui.data
        print('PASS: Alt+arrows consumed before CLI, natural exit/Alt+X remove only current session, mouse session selection/local coordinates/wheel, Ctrl+C exit returns to projects, mouse capture restored')
    finally:
        tui.cleanup()

    class NegotiatedTui(Tui):
        def pump(self):
            super().pump()
            if b'\x1b[?u\x1b[c' in self.data and not getattr(self, 'responded', False):
                self.responded = True
                os.write(self.fd, b'\x1b[?0u\x1b[?1;2c')

    tui = NegotiatedTui(env)
    try:
        assert b'\x1b[>1u' in tui.data
        before = len(records(launches))
        tui.send(b'n\r')
        tui.wait(lambda: len(records(launches)) == before + 1)
        tui.send(b'\x1b[1;3D\x1b[1;3C')  # Alt+arrows under negotiated keyboard protocol
        tui.send(b'\x1b[1089;5u')  # Cyrillic Ctrl+с
        tui.wait(lambda: 'завершён' in tui.text())
        tui.send(b'\x1b[110;3u\r')  # Alt+n from project panel after exit
        tui.wait(lambda: len(records(launches)) == before + 2)
        tui.send(b'\x1b[1090;3u\r')  # Cyrillic Alt+т while CLI is active
        tui.wait(lambda: len(records(launches)) == before + 3)
        tui.send(b'\x1b[113;3u')
        tui.finish()
        assert b'\x1b[<1u' in tui.data
        print('PASS: negotiated CSI-u keyboard protocol, Cyrillic Ctrl+C/Alt+N, panel shortcuts and restoring keyboard flags')
    finally:
        tui.cleanup()
