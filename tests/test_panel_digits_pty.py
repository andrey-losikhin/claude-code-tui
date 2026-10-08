"""Panel digits bypass active children and launchers override terminal tab bindings."""
from pathlib import Path
exec((Path(__file__).parent/'support.py').read_text())
exec((Path(__file__).parent/'screen.py').read_text())
import subprocess
import shutil

with tempfile.TemporaryDirectory(prefix='claude-panel-digits-') as directory:
    root=Path(directory)
    for name in ('home','config','bin','project'):
        (root/name).mkdir()
    config=root/'config/claude-code-tui/config.json'
    config.parent.mkdir()
    config.write_text(json.dumps({'projects':[str(root/'project')]}))
    capture,editors=root/'capture',root/'editors'
    fake=root/'bin/claude'
    fake.write_text('''#!/usr/bin/python3
import os, tty
tty.setraw(0)
print('DIGIT CLI READY',flush=True)
while True:
    data=os.read(0,4096)
    if not data: break
    with open(os.environ['CAPTURE'],'ab') as file: file.write(data)
''')
    fake.chmod(0o755)
    nvim=root/'bin/nvim'
    nvim.write_text('''#!/usr/bin/python3
import json,os,sys
with open(os.environ['EDITORS'],'a') as file: file.write(json.dumps({'file':sys.argv[-1]})+'\\n')
os.execv('/usr/bin/nvim',['nvim','--clean','-i','NONE','-n',*sys.argv[1:]])
''')
    nvim.chmod(0o755)
    env=dict(os.environ,HOME=str(root/'home'),XDG_CONFIG_HOME=str(root/'config'),XDG_DATA_HOME=str(root/'data'),PATH=str(root/'bin'),TERM='xterm-256color',CAPTURE=str(capture),EDITORS=str(editors))
    tui=Tui(env)
    expected=b''
    try:
        tui.send(b'\x1b4')
        tui.wait(lambda: any('Alt+4:' in line and 'заметки' in line for line in screen(tui)))
        assert not editors.exists()
        tui.send(b'n\r')
        tui.wait(lambda: 'DIGITCLIREADY' in tui.text())
        encodings=[lambda digit:b'\x1b'+digit.encode(),lambda digit:f'\x1b[{ord(digit)};3u'.encode(),lambda digit:f'\x1b[{ord(digit)};3:2u'.encode()]
        for index,encode in enumerate(encodings):
            tui.send(encode('1')+b'f')
            tui.wait(lambda: 'Поиск:' in screen(tui)[0])
            tui.send(encode('2')+b'A')
            expected+=b'A'
            tui.wait(lambda: capture.exists() and capture.read_bytes()==expected)
            tui.send(encode('3')+b'r')
            tui.wait(lambda: 'Новое название:' in screen(tui)[0])
            tui.send(encode('4'))
            tui.wait(lambda: editors.exists() and any('Заметка' in line for line in screen(tui)))
            tui.send(f'GoDIGIT NOTE {index}\x1b:w\r'.encode())
            note=Path(records(editors)[0]['file'])
            tui.wait(lambda: f'DIGIT NOTE {index}' in note.read_text())
            tui.send(encode('4'))
            assert len(records(editors))==1, 'Alt+4 toggled/restarted the editor'
            # Switch from INSERT mode; next marker must go only to Claude.
            tui.send(b'GoUNSAVED')
            tui.send(encode('2')+b'B')
            expected+=b'B'
            tui.wait(lambda: capture.read_bytes()==expected)
            tui.send(encode('4')+b'\x1b:w\r')
            tui.wait(lambda: 'UNSAVED' in note.read_text())
            tui.send(encode('2'))
        tui.send(b'1234')
        expected+=b'1234'
        tui.wait(lambda: capture.read_bytes()==expected)
        # Popups and full-screen output cannot capture panel shortcuts.
        tui.send(b'\x1bk\x1b1f')
        tui.wait(lambda: 'Поиск:' in screen(tui)[0])
        tui.send(b'\x1b2\x1bb')
        tui.wait(lambda: len(records(editors))==2)
        tui.send(b'\x1b3r')
        tui.wait(lambda: 'Новое название:' in screen(tui)[0])
        tui.send(b'\x1b2\x1bu\x1b1')
        tui.wait(lambda: not json.loads(config.read_text())['layout']['sidebar_hidden'])
        tui.send(b'\x1b2\x1bd\x1b4')
        tui.wait(lambda: any('Заметка' in line for line in screen(tui)))
        assert capture.read_bytes()==expected, 'A panel digit leaked to Claude'
        tui.send(b'\x1bq')
        tui.finish()
    finally:
        tui.cleanup()
    print('PASS: Alt+1..4 classic/CSI-u/repeat, active Claude/insert nvim, modal/viewer escape, hidden/maximized panels, unchanged ordinary digits, no child leakage')

    # Verify launch arguments without opening real GUI terminals or changing host config.
    # The installer checks a release path, but the check suite only builds debug.
    # Keep that fixture isolated rather than relying on a developer's target tree.
    launcher_repo=root/'launcher-repo'
    (launcher_repo/'scripts').mkdir(parents=True)
    for name in ('run.sh','install.sh','scripts/cargo-build.sh'):
        shutil.copy2(REPO/name,launcher_repo/name)
    for profile in ('debug','release'):
        destination=launcher_repo/'target'/profile/'claude-code-tui'
        destination.parent.mkdir(parents=True)
        shutil.copy2(BINARY,destination)
    argv=root/'argv'
    for terminal in ('ghostty','kitty'):
        stub=root/'bin'/terminal
        stub.write_text('#!/usr/bin/python3\nimport json,os,sys\nopen(os.environ["ARGV"],"w").write(json.dumps(sys.argv[1:]))\n')
        stub.chmod(0o755)
    for compiler in ('cargo','rustc'):
        stub=root/'bin'/compiler
        stub.write_text('#!/bin/sh\nexit 0\n')
        stub.chmod(0o755)
    env.update(PATH=str(root/'bin')+':/usr/bin:/bin',ARGV=str(argv))
    def assert_args(terminal):
        args=json.loads(argv.read_text())
        for digit in range(1,5):
            if terminal=='ghostty':
                assert f'--keybind=alt+digit_{digit}=esc:{digit}' in args
                assert f'--keybind=alt+{digit}=esc:{digit}' in args
            else:
                index=args.index(f'map alt+{digit} send_text all \\x1b{digit}')
                assert args[index-1]=='-o'
    for terminal in ('ghostty','kitty'):
        subprocess.run(['bash',str(launcher_repo/'run.sh'),'--terminal',terminal],env=env,check=True,timeout=10)
        assert_args(terminal)
        subprocess.run(['bash',str(launcher_repo/'install.sh'),'--no-build','--terminal',terminal],env=env,check=True,timeout=10,stdout=subprocess.PIPE)
        subprocess.run([str(root/'home/.local/bin/claude-code-tui-launch')],env=env,check=True,timeout=10)
        assert_args(terminal)
    print('PASS: run.sh and installed launcher scope Ghostty/Kitty Alt+digits overrides to the application window')
