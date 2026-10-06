from pathlib import Path
exec((Path(__file__).parent / "support.py").read_text())
exec((Path(__file__).parent / "screen.py").read_text())
with tempfile.TemporaryDirectory(prefix='claude-live-state-') as directory:
    root=Path(directory)
    for folder in ['home','config','bin','project']: (root/folder).mkdir()
    launches=root/'launches'
    config=root/'config/claude-code-tui/config.json'
    config.parent.mkdir()
    config.write_text(json.dumps({'projects':[str(root/'project')]}))
    fake=root/'bin/claude'
    fake.write_text('''#!/usr/bin/python3
import json, os, pathlib, sys, tty
tty.setraw(0)
id=next(a.split('=',1)[1] for a in sys.argv if a.startswith(('--session-id=','--resume=')))
with open(os.environ['LAUNCHES'],'a') as f: f.write(json.dumps({'id':id,'pid':os.getpid()})+'\\n')
while True:
    data=os.read(0,4096)
    if b'u' in data:
        path=pathlib.Path(os.environ['HOME'])/'.claude/projects/test'/f'{id}.jsonl'
        path.parent.mkdir(parents=True,exist_ok=True)
        path.write_text(json.dumps({'sessionId':id,'cwd':os.getcwd(),'aiTitle':'CLI_ALPHA'})+'\\n')
    if b't' in data or b'v' in data:
        path=pathlib.Path(os.environ['HOME'])/'.claude/projects/test'/f'{id}.jsonl'
        with path.open('a') as f:
            for _ in range(40): f.write(json.dumps({'type':'assistant','message':'x'*2048})+'\\n')
            f.write(json.dumps({'type':'custom-title','sessionId':id,'customTitle':'CLI_NATIVE1111' if b'v' in data else 'CLI_NATIVE111'})+'\\n')
    if b'\\x03' in data: break
''')
    fake.chmod(0o755)
    env=dict(os.environ, HOME=str(root/'home'), XDG_CONFIG_HOME=str(root/'config'),
             TERM='xterm-256color', PATH=str(root/'bin'), LAUNCHES=str(launches))
    tui=Tui(env)
    try:
        tui.send(b'n\r')
        tui.wait(lambda: len(records(launches))==1)
        first=records(launches)[0]['id']
        # New chat exists in sidebar immediately, before CLI writes any history.
        tui.send(b'\x1b[1;3D')
        tui.send(b'r')
        tui.send(b'\x1b[200~\x1b[201~')
        for _ in range(50): tui.send(b'\x7f')
        tui.send(b'RENAMED_ALPHA\r')
        tui.wait(lambda: labels(tui,'RENAMED_ALPHA')==(True,True,True))
        assert json.loads(config.read_text())['renamed_sessions'][first]=='RENAMED_ALPHA'
        # The CLI's persisted title appears live but cannot replace user's override.
        tui.send(b'\x1b[1;3C')
        tui.send(b'u')
        for _ in range(55): tui.pump()
        assert labels(tui,'RENAMED_ALPHA')==(True,True,True)
        assert 'CLI_ALPHA' not in '\n'.join(screen(tui))
        # Native /rename metadata appended after the former 32-line/64KiB limit.
        tui.send(b't')
        tui.wait(lambda: labels(tui,'CLI_NATIVE111')==(True,True,True))
        assert first not in json.loads(config.read_text())['renamed_sessions']
        tui.send(b'v')
        tui.wait(lambda: labels(tui,'CLI_NATIVE1111')==(True,True,True))
        tui.send(b'\x1bn\r')
        tui.wait(lambda: len(records(launches))==2)
        second=records(launches)[1]['id']
        # Rename active second chat directly from the open-session panel.
        tui.send(b'\x1b[1;3D\x1b[1;3B')
        tui.send(b'r')
        for _ in range(50): tui.send(b'\x7f')
        tui.send(b'RENAMED_BETA\r')
        tui.wait(lambda: labels(tui,'RENAMED_BETA')==(True,True,True))
        assert json.loads(config.read_text())['renamed_sessions'][second]=='RENAMED_BETA'
        tui.send(b'\x1b[1;3C')
        tui.send(b'u')
        # Switch to first and close only it. Its saved history label remains.
        tui.send(b'\x1b[1;3D\x1b[1;3B\x1b[A\r')
        tui.wait(lambda: labels(tui,'CLI_NATIVE1111')==(True,True,True))
        tui.send(b'\x1bx')
        tui.wait(lambda: labels(tui,'CLI_NATIVE1111')==(True,False,False))
        assert alive(records(launches)[1]['pid'])
        # Reopen saved history with the same ID and name, no duplicate second child.
        tui.send(b'\x1b[F\r')
        tui.wait(lambda: len(records(launches))==3)
        assert records(launches)[2]['id']==first
        tui.wait(lambda: labels(tui,'CLI_NATIVE1111')==(True,True,True))
        tui.send(b'\x03')
        tui.wait(lambda: labels(tui,'CLI_NATIVE1111')==(True,False,False))
        assert alive(records(launches)[1]['pid'])
        tui.send(b'\x1bq'); tui.finish()
        print('PASS: new chat before history, rename in all panels/open-session rename, live history with override, switch/close/resume/native exit, other process survives')
    finally: tui.cleanup()
