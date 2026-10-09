"""Workspace features against synthetic history/hooks and isolated real nvim."""
from pathlib import Path
exec((Path(__file__).parent / 'support.py').read_text())
exec((Path(__file__).parent / 'screen.py').read_text())


def mouse(x, y, release=False, drag=False):
    return f'\x1b[<{32 if drag else 0};{x};{y}{"m" if release else "M"}'.encode()

with tempfile.TemporaryDirectory(prefix='claude-workspace-') as directory:
    root = Path(directory)
    for name in ('bin', 'home', 'config', 'project'):
        (root/name).mkdir()
    config = root/'config/claude-code-tui/config.json'
    config.parent.mkdir()
    config.write_text(json.dumps({'projects': [str(root/'project')]}))
    launches, inputs, editor_log = root/'launches', root/'inputs', root/'editors'
    fake = root/'bin/claude'
    fake.write_text(r'''#!/usr/bin/python3
import json, os, pathlib, select, shlex, subprocess, sys, tty
tty.setraw(0)
id = next(a.split('=',1)[1] for a in sys.argv if a.startswith(('--session-id=', '--resume=')))
settings = json.loads(sys.argv[sys.argv.index('--settings')+1])
command = shlex.split(settings['hooks']['Stop'][0]['hooks'][0]['command'])
assert set(settings)=={'hooks'}
assert set(settings['hooks'])=={'SessionStart','UserPromptSubmit','PreToolUse','PermissionRequest','PostToolUse','PostToolUseFailure','Notification','Stop','StopFailure'}
def emit(event):
    payload = {'session_id':id, 'hook_event_name':event, 'prompt_id':'p1', 'prompt':'DO NOT STORE PROMPT'}
    subprocess.run(command, input=json.dumps(payload).encode(), check=True)
emit('SessionStart')
log = pathlib.Path(os.environ['LAUNCHES'])
items = log.read_text().splitlines() if log.exists() else []
title = 'Alpha' if not items else 'Beta'
with log.open('a') as file: file.write(json.dumps({'id':id,'title':title,'pid':os.getpid()})+'\n')
history = pathlib.Path(os.environ['HOME'])/'.claude/projects/synthetic'
history.mkdir(parents=True,exist_ok=True)
(history/(id+'.jsonl')).write_text(json.dumps({'sessionId':id,'cwd':os.getcwd(),'aiTitle':title})+'\n'+json.dumps({'type':'assistant','message':{'content':[{'type':'text','text':'Unique workspace needle\nSecond response line'},{'type':'tool_use','input':{'secret':'DO NOT INDEX TOOL'}}]}})+'\n')
print('\x1b[2J\x1b[HCHAT FRAGMENT',flush=True)
marker = pathlib.Path(os.environ['TRIGGERS'])/(id+'.stop')
while True:
    if marker.exists():
        marker.unlink(); emit('UserPromptSubmit'); emit('Stop')
    if select.select([0],[],[],0.02)[0]:
        data=os.read(0,1024)
        if not data: break
        with open(os.environ['INPUTS'],'ab') as file: file.write(data)
''')
    fake.chmod(0o755)
    wrapper = root/'bin/nvim'
    wrapper.write_text('''#!/usr/bin/python3
import json, os, sys
with open(os.environ['EDITORS'],'a') as log: log.write(json.dumps({'file':sys.argv[-1], 'pid':os.getpid()})+'\\n')
os.execv('/usr/bin/nvim',['nvim','--clean','-i','NONE','-n',*sys.argv[1:]])
''')
    wrapper.chmod(0o755)
    triggers = root/'triggers'
    triggers.mkdir()
    notification_log=root/'notifications'
    notifier=root/'bin/notify-send'
    notifier.write_text('#!/usr/bin/python3\nimport os, pathlib, sys\npathlib.Path(os.environ["NOTIFICATIONS"]).write_text("|".join(sys.argv[1:]))\n')
    notifier.chmod(0o755)
    clipboard = root/'bin/wl-copy'
    clipboard.write_text('#!/usr/bin/python3\nimport sys\nsys.stdin.buffer.read()\n')
    clipboard.chmod(0o755)
    env = dict(os.environ, HOME=str(root/'home'), XDG_CONFIG_HOME=str(root/'config'),
               XDG_DATA_HOME=str(root/'data'), PATH=str(root/'bin'), TERM='xterm-256color',
               LAUNCHES=str(launches), INPUTS=str(inputs), EDITORS=str(editor_log), TRIGGERS=str(triggers), NOTIFICATIONS=str(notification_log))
    tui = Tui(env)
    try:
        tui.send(b'n\r')
        tui.wait(lambda: len(records(launches))==1)
        first=records(launches)[0]['id']
        tui.wait(lambda: any('ждёт ввода' in line for line in screen(tui)))
        # All global actions work while the CLI is active, also in Russian.
        tui.send('\x1bр'.encode())
        tui.wait(lambda: any('Справка по клавишам' in line for line in screen(tui)))
        tui.send(b'\x1b')
        tui.send(b'\x1bk')
        tui.wait(lambda: any('Команды:' in line for line in screen(tui)))
        tui.send('новый\r'.encode())
        tui.wait(lambda: any('Новый чат' in line for line in screen(tui)))
        tui.send(b'\r')
        tui.wait(lambda: len(records(launches))==2)
        second=records(launches)[1]['id']
        (triggers/(first+'.stop')).touch()
        tui.wait(lambda: any('◆' in line and 'готово' in line for line in screen(tui)))
        assert not notification_log.exists(), 'Notifications must be opt-in'
        tui.send(b'\x1bt')
        (triggers/(first+'.stop')).touch()
        tui.wait(lambda: notification_log.exists())
        assert 'DO NOT STORE PROMPT' not in notification_log.read_text()
        assert json.loads(config.read_text())['desktop_notifications']
        tui.send(b'\x1bj')
        tui.wait(lambda: 'Alpha' in screen(tui)[0] and 'готово' in screen(tui)[0])
        assert not any('◆' in line for line in screen(tui))
        assert len(records(launches))==2
        tui.send(b'\x1bs')
        tui.send(b'Beta\r')
        tui.wait(lambda: 'Beta' in screen(tui)[0])
        assert len(records(launches))==2, 'Switcher restarted a live CLI'
        tui.send(b'\x1bz')
        tui.send(b'\x1bh')
        tui.send(b'\x1b')
        assert str(root/'project') in json.loads(config.read_text())['collapsed_projects']
        # Fuzzy popup must consume query input; body search excludes tool payloads.
        tui.send('\x1bа'.encode())
        tui.send(b'Unique')
        tui.wait(lambda: any('Unique workspace needle' in line for line in screen(tui)))
        tui.send(b'\r')
        tui.wait(lambda: editor_log.exists() and 'Uniqueworkspaceneedle' in tui.text())
        tui.send(b':q\r')
        tui.send(b'\x1bfDO NOT INDEX TOOL')
        tui.wait(lambda: any('0 результатов' in line for line in screen(tui)))
        tui.send(b'\x1b')
        # Resize sidebar via border and persist it; then hide/maximize and restore.
        tui.send(mouse(48,10)+mouse(60,10,drag=True)+mouse(60,10,release=True))
        tui.wait(lambda: json.loads(config.read_text()).get('layout',{}).get('sidebar_percent')==39)
        tui.send(b'\x1bu')
        tui.wait(lambda: json.loads(config.read_text())['layout']['sidebar_hidden'])
        tui.send(b'\x1bu')
        tui.send(b'\x1bd')
        tui.wait(lambda: screen(tui)[0].startswith('┌2 · Claude Code') and 'Проекты' not in screen(tui)[0])
        tui.send(b'\x1bd')
        tui.wait(lambda: 'Проекты' in screen(tui)[0])
        # Create an unsaved nvim buffer, then append selection without overwriting it.
        tui.send(b'\x1bm')
        tui.wait(lambda: len(records(editor_log))>=2 and 'Notes' in tui.text())
        tui.send(b'GoUNSAVED BUFFER\x1b')
        note=root/'home/knowledge-base/claude-code-tui/notes'/f'{second}.md'
        assert 'UNSAVED BUFFER' not in note.read_text()
        editor_count=len(records(editor_log))
        tui.send(b'\x1bm\x1bm')
        tui.wait(lambda: 'UNSAVED BUFFER' in '\n'.join(screen(tui)))
        assert len(records(editor_log))==editor_count
        assert 'UNSAVED BUFFER' not in note.read_text()
        tui.send(b'\x1b[1;3A')
        tui.send(mouse(61,2)+mouse(73,2,drag=True)+mouse(73,2,release=True))
        tui.send(b'\x1bj')
        tui.wait(lambda: 'Alpha' in screen(tui)[0])
        tui.send(b'\x1be')
        tui.wait(lambda: 'Сначалавыделите' in tui.text())
        assert not (root/'home/knowledge-base/claude-code-tui/notes'/f'{first}.md').exists(), 'Selection leaked into another chat note'
        tui.send(b'\x1bj')
        tui.wait(lambda: 'Beta' in screen(tui)[0])
        tui.send(mouse(61,2)+mouse(73,2,drag=True)+mouse(73,2,release=True))
        tui.send(b'\x1be')
        tui.send(b':w\r')
        note=root/'home/knowledge-base/claude-code-tui/notes'/f'{second}.md'
        tui.wait(lambda: note.exists() and 'UNSAVED BUFFER' in note.read_text() and 'CHAT FRAGMENT' in note.read_text())
        tui.send(b'\x1bl')
        tui.wait(lambda: any('База знаний' in line for line in screen(tui)))
        tui.send(b'\r')
        assert len(records(launches))==2
        assert not inputs.exists(), 'TUI shortcut/query/editor input leaked into Claude'
        tui.send(b'GoEXIT DIRTY\x1b')
        tui.send(b'\x1bd')
        tui.send(b'\x1bq')
        tui.wait(lambda: 'Savechanges' in tui.text() and any('Заметка' in line for line in screen(tui)))
        tui.send(b'y')
        tui.finish()
        assert 'EXIT DIRTY' in note.read_text()
    finally:
        tui.cleanup()
    print('PASS: hooks/unread, RU help/search, command palette, switch/previous without restart, collapse, fulltext/tool exclusion, persisted drag/hide/maximize, catalog and selection preserving dirty nvim')
