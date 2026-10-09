"""Named sets of chats, real clean notes, and three mouse-resizable dividers."""
from pathlib import Path
exec((Path(__file__).parent/'support.py').read_text())
exec((Path(__file__).parent/'screen.py').read_text())

def mouse(x,y,kind=0):
    return f'\x1b[<{32 if kind==1 else 0};{x};{y}{"m" if kind==2 else "M"}'.encode()

with tempfile.TemporaryDirectory(prefix='claude-saved-workspaces-') as directory:
    root=Path(directory)
    for name in ('bin','home','config','project'):
        (root/name).mkdir()
    config=root/'config/claude-code-tui/config.json'
    config.parent.mkdir()
    config.write_text(json.dumps({'projects':[str(root/'project')]}))
    launches=root/'launches'
    fake=root/'bin/claude'
    fake.write_text(r'''#!/usr/bin/python3
import os,sys,json,pathlib,tty
tty.setraw(0)
arg=next(a for a in sys.argv if a.startswith(('--session-id=','--resume=')))
id=arg.split('=',1)[1]
with open(os.environ['LAUNCHES'],'a') as f: f.write(json.dumps({'id':id,'arg':arg,'pid':os.getpid()})+'\n')
history=pathlib.Path(os.environ['HOME'])/'.claude/projects/synthetic'
history.mkdir(parents=True,exist_ok=True)
(history/(id+'.jsonl')).write_text(json.dumps({'sessionId':id,'cwd':os.getcwd(),'aiTitle':'Saved test'})+'\n')
print('WORKSPACE CLI READY',flush=True)
while os.read(0,4096): pass
''')
    fake.chmod(0o755)
    nvim=root/'bin/nvim'
    nvim.write_text('#!/bin/sh\nexec /usr/bin/nvim --clean -i NONE -n "$@"\n')
    nvim.chmod(0o755)
    env=dict(os.environ,HOME=str(root/'home'),XDG_CONFIG_HOME=str(root/'config'),XDG_DATA_HOME=str(root/'data'),PATH=str(root/'bin')+':/usr/bin:/bin',TERM='xterm-256color',LAUNCHES=str(launches))
    def cfg(): return json.loads(config.read_text())
    tui=Tui(env)
    def choose(text):
        tui.send((text+'\r').encode())
    def manager():
        tui.send('\x1bщ'.encode())
        tui.wait(lambda: any('Менеджер сессий:' in line for line in screen(tui)))
    def confirm():
        tui.send(b'\x1b[B\r')
    try:
        tui.send(b'n\r')
        tui.wait(lambda: len(records(launches))==1)
        first=records(launches)[0]['id']
        tui.send(b'\x1bn\r')
        tui.wait(lambda: len(records(launches))==2)
        second=records(launches)[1]['id']
        tui.wait(lambda: 'WORKSPACECLIREADY' in tui.text())
        tui.send(b'\x1bm')
        tui.wait(lambda: any('4 · ✎ Заметка' in line for line in screen(tui)))
        tui.send(b'GoDIRTY WORKSPACE NOTE\x1b')
        tui.wait(lambda: 'DIRTY WORKSPACE NOTE' in '\n'.join(screen(tui)))
        # Sidebar width, left vertical split, and right chat/note split.
        tui.send(mouse(48,10)+mouse(60,10,1)+mouse(60,10,2))
        tui.wait(lambda: cfg()['layout']['sidebar_percent']==39)
        tui.send(mouse(10,34)+mouse(10,25,1)+mouse(10,25,2))
        tui.wait(lambda: cfg()['layout']['projects_percent']==61)
        tui.send(mouse(80,22)+mouse(80,28,1)+mouse(80,28,2))
        tui.wait(lambda: cfg()['layout']['chat_percent']==69)
        for label in ('1 · Проекты','2 · Claude Code','3 · Открытые чаты','4 · ✎ Заметка'):
            assert any(label in line for line in screen(tui)), label
        manager(); choose('Сохранить как')
        tui.wait(lambda: any('Название сессии:' in line for line in screen(tui)))
        choose('Work')
        tui.wait(lambda: len(cfg().get('workspaces',[]))==1)
        saved=cfg()['workspaces'][0]
        assert [c['id'] for c in saved['chats']]==[first,second]
        assert saved['visible_notes']==[second] and saved['active_chat']==second
        assert saved['layout']['projects_percent']==61
        manager(); choose('Work'); choose('Переименовать')
        tui.send(b'\x7f'*4); choose('Renamed')
        tui.wait(lambda: cfg()['workspaces'][0]['name']=='Renamed')
        # Close waits for dirty editor; cancel via global panel key keeps chats alive.
        manager(); choose('Закрыть текущие'); confirm()
        tui.wait(lambda: 'Savechanges' in tui.text())
        assert all(alive(r['pid']) for r in records(launches))
        tui.send(b'c'); tui.pump(); tui.send(b'\x1b2')
        tui.pump()
        assert all(alive(r['pid']) for r in records(launches))
        manager(); choose('Закрыть текущие'); confirm()
        tui.wait(lambda: 'Savechanges' in tui.text())
        tui.send(b'y')
        tui.wait(lambda: all(not alive(r['pid']) for r in records(launches)))
        note=root/'home/knowledge-base/claude-code-tui/notes'/f'{second}.md'
        assert 'DIRTY WORKSPACE NOTE' in note.read_text()
        manager(); choose('Renamed'); choose('Открыть'); confirm()
        tui.wait(lambda: len(records(launches))==4)
        assert {r['id'] for r in records(launches)[2:]}=={first,second}
        assert all(r['arg'].startswith('--resume=') for r in records(launches)[2:])
        tui.wait(lambda: 'DIRTY WORKSPACE NOTE' in '\n'.join(screen(tui)))
        # Snapshot remains available in a fresh TUI process.
        tui.send(b'\x1bq'); tui.finish()
        tui=Tui(env)
        manager(); choose('Renamed'); choose('Открыть'); confirm()
        tui.wait(lambda: len(records(launches))==6)
        assert {r['id'] for r in records(launches)[4:]}=={first,second}
        manager(); choose('Сохранить и закрыть'); confirm()
        tui.wait(lambda: all(not alive(r['pid']) for r in records(launches)))
        manager(); choose('Renamed'); choose('Открыть'); confirm()
        tui.wait(lambda: len(records(launches))==8)
        manager(); choose('Renamed'); choose('Удалить'); confirm()
        tui.wait(lambda: cfg()['workspaces']==[])
        assert all(alive(r['pid']) for r in records(launches)[6:])
        assert note.exists() and len(list((root/'home/.claude/projects/synthetic').glob('*.jsonl')))==2
        tui.send(b'\x1b'); tui.pump(); tui.send(b'\x1bq'); tui.finish()
    finally:
        tui.cleanup()
    print('PASS: named workspace save/rename/close/cancel/restore/delete, dirty nvim safety, UUID resume, RU manager key, numbered panels and three mouse dividers')
