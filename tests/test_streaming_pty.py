from pathlib import Path
exec((Path(__file__).parent / 'support.py').read_text())
exec((Path(__file__).parent / 'screen.py').read_text())

with tempfile.TemporaryDirectory(prefix='claude-streaming-') as directory:
    root = Path(directory)
    for folder in ('home', 'config', 'bin', 'project'):
        (root / folder).mkdir()
    config = root / 'config/claude-code-tui/config.json'
    config.parent.mkdir()
    config.write_text(json.dumps({'projects': [str(root / 'project')]}))
    frames = root / 'frames'
    fake = root / 'bin/claude'
    fake.write_text('''#!/usr/bin/python3
import json, os, pathlib, time, tty
tty.setraw(0)
os.write(1, b'STREAM_READY')
while b's' not in os.read(0, 4096): pass
# Rewrite a synthetic history file while animating, exercising background refresh.
history = pathlib.Path(os.environ['HOME'])/'.claude/projects/synthetic/history.jsonl'
history.parent.mkdir(parents=True)
for i in range(100):
    if i % 30 == 0:
        with history.open('w') as f:
            for _ in range(4096):
                f.write(json.dumps({'type':'assistant','message':'x'*2048})+'\\n')
            f.write(json.dumps({'sessionId':'history','cwd':os.getcwd(),'customTitle':'HISTORY'})+'\\n')
    with open(os.environ['FRAMES'], 'a') as f:
        f.write(json.dumps({'frame':i,'time':time.monotonic()})+'\\n')
    os.write(1, ('\\x1b[HFRAME_%04d' % i).encode())
    time.sleep(0.06)
while True: os.read(0, 4096)
''')
    fake.chmod(0o755)
    env = dict(os.environ, HOME=str(root/'home'), XDG_CONFIG_HOME=str(root/'config'),
               PATH=str(root/'bin'), TERM='xterm-256color', FRAMES=str(frames))
    tui = Tui(env)
    try:
        tui.send(b'n\r')
        tui.wait(lambda: 'STREAM_READY' in tui.text())
        tui.send(b's')
        observed = {}
        deadline = time.monotonic() + 20
        while 99 not in observed and time.monotonic() < deadline:
            tui.pump()
            match = re.search(r'FRAME_(\d{4})', '\n'.join(line[49:] for line in screen(tui)))
            if match:
                observed.setdefault(int(match.group(1)), time.monotonic())
        assert 99 in observed, 'Stream did not reach the last frame'
        sent = {record['frame']:record['time'] for record in records(frames)}
        delays = sorted(observed[frame] - sent[frame] for frame in observed)
        assert len(observed) >= 80, f'Only {len(observed)}/100 animation frames rendered'
        assert delays[int(len(delays)*0.95)] < 0.25, f'95th percentile frame delay: {delays}'
        tui.send(b'\x1bq')
        tui.finish()
        print(f'PASS: {len(observed)}/100 CLI animation frames, p95 latency '
              f'{delays[int(len(delays)*0.95)]*1000:.0f} ms during history updates')
    finally:
        tui.cleanup()

# Keep active frames responsive while a hidden PTY continuously writes.
with tempfile.TemporaryDirectory(prefix='tui-flood-') as directory:
 root=Path(directory)
 for d in ('home','config','bin','project'): (root/d).mkdir()
 config=root/'config/claude-code-tui/config.json'; config.parent.mkdir(); config.write_text(json.dumps({'projects':[str(root/'project')]}))
 launches=root/'launches'; frames=root/'frames';fake=root/'bin/claude'
 fake.write_text('''#!/usr/bin/python3
import os,tty,time,json
import threading
tty.setraw(0)
with open(os.environ['LAUNCHES'],'a') as f:f.write(str(os.getpid())+'\\n')
os.write(1,b'READY')
while True:
 d=os.read(0,4096)
 if b'f' in d:
  def flood():
   while not os.path.exists(os.environ['TRIGGER']):time.sleep(.01)
   end=time.monotonic()+3
   while time.monotonic()<end:os.write(1,b'xxxxxxxx'*1024)
  threading.Thread(target=flood).start()
 if b's' in d:
  open(os.environ['TRIGGER'],'w').close()
  for i in range(60):
   with open(os.environ['FRAMES'],'a') as f:f.write(json.dumps({'frame':i,'time':time.monotonic()})+'\\n')
   os.write(1,('\\x1b[HFRAME_%04d'%i).encode());time.sleep(.05)
''');fake.chmod(0o755)
 env=dict(os.environ,HOME=str(root/'home'),XDG_CONFIG_HOME=str(root/'config'),PATH=str(root/'bin'),TERM='xterm-256color',LAUNCHES=str(launches),FRAMES=str(frames),TRIGGER=str(root/'trigger'))
 tui=Tui(env)
 try:
  tui.send(b'n\r');tui.wait(lambda:'READY' in tui.text())
  tui.send(b'f\x1bn\r');tui.wait(lambda:launches.exists() and len(launches.read_text().splitlines())==2)
  tui.send(b's');observed={};deadline=time.monotonic()+10
  while 59 not in observed and time.monotonic()<deadline:
   tui.pump();m=re.search(r'FRAME_(\d{4})','\n'.join(line[49:] for line in screen(tui)))
   if m:observed.setdefault(int(m[1]),time.monotonic())
  sent={r['frame']:r['time'] for r in records(frames)}
  delays=sorted(observed[f]-sent[f] for f in observed)
  assert len(observed) >= 45, f'Background flood blocked rendering: {len(observed)}/60 frames'
  assert delays[int(len(delays)*.95)] < .25, delays
  print(f'PASS: background flood, {len(observed)}/60 active frames, p95 {delays[int(len(delays)*.95)]*1000:.0f} ms')
  tui.send(b'\x1bq');tui.finish()
 finally:tui.cleanup()
