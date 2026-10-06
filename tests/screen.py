
import unicodedata

def screen(tui):
    cells=[[' ']*150 for _ in range(40)]
    row=col=0
    data=tui.data.decode(errors='replace')
    index=0
    while index<len(data):
        c=data[index]
        if c=='\x1b':
            match=re.match(r'\x1b\[([0-?]*)[ -/]*([@-~])',data[index:])
            if match:
                params, final=match.groups()
                numbers=[int(n) if n.isdigit() else 0 for n in params.split(';')]
                n=numbers[0] or 1
                if final in 'Hf': row=max(0,min(39,n-1)); col=max(0,min(149,(numbers[1] if len(numbers)>1 else 1)-1))
                elif final=='A': row=max(0,row-n)
                elif final=='B': row=min(39,row+n)
                elif final=='C': col=min(149,col+n)
                elif final=='D': col=max(0,col-n)
                elif final=='G': col=max(0,min(149,n-1))
                elif final=='d': row=max(0,min(39,n-1))
                elif final=='J' and numbers[0] in (2,3): cells=[[' ']*150 for _ in range(40)]
                elif final=='K':
                    start,end=(0,150) if numbers[0]==2 else ((0,col+1) if numbers[0]==1 else (col,150))
                    cells[row][start:end]=[' ']*(end-start)
                index+=len(match.group()); continue
            index+=2; continue
        if c=='\r': col=0
        elif c=='\n': row=min(39,row+1)
        elif ord(c)>=32:
            if not unicodedata.combining(c):
                cells[row][col]=c
                col=min(149,col+(2 if unicodedata.east_asian_width(c) in 'WF' else 1))
        index+=1
    return [''.join(line) for line in cells]

def labels(tui, title):
    lines=screen(tui)
    return (any(title in line[:48] for line in lines[:33]),
            any(title in line[:48] for line in lines[33:39]),
            title in lines[0][48:])
