import re, os
d = open(r'E:\disk-butler.exe','rb').read()
out = r'C:\Users\Administrator\WorkBuddy\2026-10-09-16-21-42\rev'

print("=== network capability check ===")
for p in [b'reqwest', b'hyper', b'ureq', b'TcpStream', b'UdpSocket', b'ConnectEx', b'WSAStartup',
          b'ws2_32', b'winhttp', b'WinHttpOpen', b'InternetOpen', b'HttpSendRequest',
          b'getaddrinfo', b'curl', b'socket', b'telemetry', b'analytics', b'sentry', b'umami']:
    print(f"   {p.decode():16s} {'@0x%x'%d.find(p) if d.find(p)>=0 else 'NOT FOUND':>12s}  cnt={d.count(p)}")

print("\n=== DLL import list (delay/normal) ===")
import struct
e=struct.unpack_from('<I',d,0x3c)[0]; coff=e+4
nsec=struct.unpack_from('<H',d,coff+2)[0]; optsize=struct.unpack_from('<H',d,coff+16)[0]; opt=coff+20
sh=opt+optsize; secs=[]
for i in range(nsec):
    o=sh+i*40; nm=d[o:o+8].rstrip(b'\0').decode('latin1')
    vs,va,rs,ra=struct.unpack_from('<IIII',d,o+8); secs.append((nm,va,vs,ra,rs))
def r2o(rva):
    for nm,va,vs,ra,rs in secs:
        if va<=rva<va+max(vs,rs): return ra+(rva-va)
dd=opt+112
imp_rva,_=struct.unpack_from('<II',d,dd+8*1)
o=r2o(imp_rva)
while True:
    ent=d[o:o+20]
    if len(ent)<20 or ent==b'\0'*20: break
    nr=struct.unpack_from('<I',ent,12)[0]
    if nr==0: break
    no=r2o(nr); nm=d[no:d.index(b'\0',no)].decode('latin1')
    print("   ", nm)
    o+=20

print("\n=== readable catalog regions -> files ===")
def dump_utf8_text(start, end, fname):
    seg = d[start:end]
    seg = re.sub(rb'[^\x09\x0a\x0d\x20-\x7e\xc2-\xf4][\x80-\xbf]*', b'\n', seg)
    txt = seg.decode('utf-8','replace')
    lines = [l.strip() for l in txt.split('\n') if len(l.strip())>=2]
    open(os.path.join(out,fname),'w',encoding='utf-8').write('\n'.join(lines))
    print(f"   {fname}: {len(lines)} lines from 0x{start:x}-0x{end:x}")

dump_utf8_text(0x7cc400, 0x7ce400, 'catalog_browsers_caches.txt')
dump_utf8_text(0x7d4c00, 0x7d5a00, 'catalog_service_pipe.txt')
dump_utf8_text(0x7d6000, 0x7db000, 'catalog_rules.txt')
dump_utf8_text(0x7db000, 0x7e0000, 'catalog_rules2.txt')
