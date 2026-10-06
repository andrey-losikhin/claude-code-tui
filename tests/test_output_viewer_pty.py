from pathlib import Path
exec((Path(__file__).parent / "support.py").read_text())
exec((Path(__file__).parent / "screen.py").read_text())

def mouse(x, y, button=0, release=False):
    return f'\x1b[<{button};{x};{y}{"m" if release else "M"}'.encode()

with tempfile.TemporaryDirectory(prefix='claude-output-viewer-') as directory:
    root = Path(directory)
    for name in ['home', 'config', 'bin', 'project']:
        (root / name).mkdir()
    config = root / 'config/claude-code-tui/config.json'
    config.parent.mkdir()
    config.write_text(json.dumps({'projects': [str(root / 'project')]}))
    launches, editors, inputs = root / 'launches', root / 'editors', root / 'inputs'
    clipboard = root / 'clipboard'
    copier = root / 'bin/wl-copy'
    copier.write_text('''#!/usr/bin/python3
import os,sys,json
with open(os.environ['CLIPBOARD'],'a') as f: f.write(json.dumps({'text':sys.stdin.read()})+'\\n')
''')
    copier.chmod(0o755)
    for name in ['xclip','xsel']:
        program=root/'bin'/name
        program.write_text('#!/usr/bin/python3\nraise SystemExit(1)\n')
        program.chmod(0o755)
    fake = root / 'bin/claude'
    fake.write_text('''#!/usr/bin/python3
import os, json, sys, tty
tty.setraw(0)
with open(os.environ['LAUNCHES'],'a') as f: f.write(json.dumps({'pid':os.getpid()})+'\\n')
for i in range(60): os.write(1, f'LOG_LINE_{i}\\r\\n'.encode())
os.write(1,b'\\x1b[32mCURRENT_OUTPUT\\x1b[0m')
while True:
    data=os.read(0,4096)
    with open(os.environ['INPUTS'],'ab') as f: f.write(data)
''')
    fake.chmod(0o755)
    editor = root / 'bin/nvim'
    editor.write_text('''#!/usr/bin/python3
import os, json, sys
with open(os.environ['EDITORS'],'a') as f: f.write(json.dumps({'args':sys.argv[1:],'file':sys.argv[-1]})+'\\n')
os.execv('/usr/bin/nvim',['nvim','--clean']+sys.argv[1:])
''')
    editor.chmod(0o755)
    env = dict(os.environ, HOME=str(root / 'home'), XDG_CONFIG_HOME=str(root / 'config'),
               PATH=str(root / 'bin')+':/usr/bin:/bin', TERM='xterm-256color',
               LAUNCHES=str(launches), EDITORS=str(editors), INPUTS=str(inputs), CLIPBOARD=str(clipboard))
    tui = Tui(env)
    try:
        tui.send(b'n\r')
        tui.wait(lambda: 'CURRENT_OUTPUT' in tui.text())
        tui.wait(lambda: any('CURRENT_OUTPUT' in row[49:] for row in screen(tui)))
        rows=screen(tui)
        expected=rows[1][51:148].rstrip()+'\n'+rows[2][49:54].rstrip()
        assert 'G_LINE_' in expected, (expected,rows[:5])
        # Multi-line selection uses only the Claude viewport, in both directions.
        tui.send(mouse(52,2))
        start=len(tui.data)
        tui.send(mouse(54,3,32))
        assert b'\x1b[7m' in tui.data[start:], repr(tui.data[start:])
        tui.send(mouse(54,3,release=True))
        tui.wait(lambda: len(records(clipboard))==1)
        assert records(clipboard)[0]['text']==expected, (records(clipboard),expected)
        tui.send(mouse(54,3)); tui.send(mouse(52,2,32)); tui.send(mouse(52,2,release=True))
        tui.wait(lambda: len(records(clipboard))==2)
        assert records(clipboard)[1]['text']==expected
        # Dragging out into the sidebar is clamped at the Claude left edge.
        tui.send(mouse(52,2)); tui.send(mouse(5,3,32)); tui.send(mouse(5,3,release=True))
        tui.wait(lambda: len(records(clipboard))==3)
        assert records(clipboard)[2]['text']==rows[1][51:148].rstrip()+'\n'+rows[2][49:50]
        tui.send(b'\x1bb')
        tui.wait(lambda: len(records(editors)) == 1)
        snapshot = Path(records(editors)[0]['file'])
        text = snapshot.read_text()
        assert 'LOG_LINE_0' in text and 'CURRENT_OUTPUT' in text and '\x1b' not in text
        assert str(root/'project') not in text and 'Проекты' not in text and 'Открытые чаты' not in text
        assert snapshot.stat().st_mode & 0o777 == 0o600
        assert '-R' in records(editors)[0]['args']
        assert alive(records(launches)[0]['pid'])
        # The editor receives normal commands and exits without stopping Claude.
        tui.wait(lambda: 'Выводактивногочата' in tui.text())
        tui.send(b':q\r')
        tui.wait(lambda: not snapshot.exists())
        assert alive(records(launches)[0]['pid'])
        # Russian toggle sends drags to the child, but keeps outer mouse capture.
        start = len(tui.data)
        tui.send(b'\x1b' + 'с'.encode())
        assert b'\x1b[?1000l' not in tui.data[start:]
        tui.send(mouse(52,2)); tui.send(mouse(54,3,32)); tui.send(mouse(54,3,release=True))
        assert len(records(clipboard))==3
        tui.send(b'\x1b[1;3D\x1b[1;3C')
        start = len(tui.data)
        tui.send(b'\x1bc')
        assert b'\x1b[?1000l' not in tui.data[start:]
        # Terminal clipboard fallback contains precisely the same selected text.
        copier.write_text('#!/usr/bin/python3\nraise SystemExit(1)\n')
        start=len(tui.data)
        tui.send(mouse(52,2)); tui.send(mouse(54,3,32)); tui.send(mouse(54,3,release=True))
        import base64
        request=b'\x1b]52;c;'+base64.b64encode(expected.encode())+b'\x07'
        tui.wait(lambda: request in tui.data[start:])
        # Russian Alt+B opens the viewer too; Alt+N closes it and starts new chat dialog.
        tui.send(b'\x1b' + 'и'.encode())
        tui.wait(lambda: len(records(editors)) == 2)
        snapshot = Path(records(editors)[1]['file'])
        tui.send(b'\x1bn')
        tui.wait(lambda: not snapshot.exists())
        tui.wait(lambda: 'Новыйчат' in tui.text())
        tui.send(b'\x1b')
        tui.send(b'\x1bv')
        tui.wait(lambda: len(records(editors)) == 3)
        snapshot = Path(records(editors)[2]['file'])
        tui.send(b'\x1bv')
        tui.wait(lambda: not snapshot.exists())
        if inputs.exists(): assert b'\x1bv' not in inputs.read_bytes() and b'\x1bc' not in inputs.read_bytes()
        tui.send(b'\x1bq')
        tui.finish()
        print('PASS: linewise chat-only mouse selection, reverse/outside bounds, clipboard/OSC52, Alt+V excludes sidebar, real nvim cleanup, RU shortcuts and global Alt+N')
    finally:
        tui.cleanup()
