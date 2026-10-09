import struct, re, collections, os
d = open(r'E:\disk-butler-svc.exe','rb').read()
out = r'C:\Users\Administrator\WorkBuddy\2026-10-09-16-21-42\rev'

def rva2off(rva, secs):
    for va,vs,ra,rs in secs:
        if va<=rva<va+max(vs,rs): return ra+(rva-va)
e=struct.unpack_from('<I',d,0x3c)[0]; coff=e+4
nsec=struct.unpack_from('<H',d,coff+2)[0]; optsize=struct.unpack_from('<H',d,coff+16)[0]; opt=coff+20
optmagic=struct.unpack_from('<H',d,opt)[0]
sh=opt+optsize; secs=[]
for i in range(nsec):
    o=sh+i*40; nm=d[o:o+8].rstrip(b'\0').decode('latin1')
    vs,va,rs,ra=struct.unpack_from('<IIII',d,o+8); secs.append((va,vs,ra,rs))
dd=opt+(112 if optmagic==0x20b else 96)
def r2o(rva): return rva2off(rva, secs)

print("=== imported DLLs ===")
imp_rva,_=struct.unpack_from('<II',d,dd+8*1)
o=r2o(imp_rva)
while True:
    ent=d[o:o+20]
    if len(ent)<20 or ent==b'\0'*20: break
    nr=struct.unpack_from('<I',ent,12)[0]
    if nr==0: break
    no=r2o(nr); print("   ", d[no:d.index(b'\0',no)].decode('latin1'))
    o+=20

print("\n=== version info ===")
rsrc_rva,_=struct.unpack_from('<II',d,dd+8*2)
base=r2o(rsrc_rva)
def walk(off, path):
    n_named,n_id=struct.unpack_from('<HH',d,base+off+12)
    for i in range(n_named+n_id):
        eo=base+off+16+i*8
        name,offv=struct.unpack_from('<II',d,eo)
        if len(path)<2: walk(offv & 0x7fffffff, path+[name])
        else:
            do=base+(offv&0x7fffffff); drva,dsz=struct.unpack_from('<II',d,do)
            if path==[16,1]:
                txt=d[r2o(drva):r2o(drva)+dsz].decode('utf-16le','replace')
                for m in re.finditer(r'[\x20-\x7e]{3,}', txt): print("   ", m.group())
            if path==[24,1]:
                print("   MANIFEST:", d[r2o(drva):r2o(drva)+dsz].decode('utf-8','replace')[:500])
walk(0,[])

print("\n=== key strings ===")
for p in [b'DiskButlerScanSvc', b'disk-butler-scan', b'install', b'uninstall', b'repair',
          b'whitelist', b'\xe7\x99\xbd\xe5\x90\x8d\xe5\x8d\x95', b'MFT', b'\\\\.\\', b'Volume', b'FSCTL_', b'descriptor',
          b'CreateService', b'OpenSCManager', b'StartService', b'SeDebugPrivilege', b'SeBackupPrivilege',
          b'AdjustTokenPrivileges', b'Impersonate', b'GetNamedSecurityInfo']:
    i=d.find(p)
    print(f"   {p!r:28s} {'@0x%x'%i if i>=0 else 'NOT FOUND'}  cnt={d.count(p)}")

print("\n=== svc module tree (.rs / src) ===")
for m in sorted(set(re.findall(rb'[A-Za-z0-9_\\/\-]{0,60}src\\[A-Za-z0-9_\\\.\-]{2,60}\.rs', d))):
    s=m.decode('latin1')
    if 'registry' in s or 'rustc' in s: continue
    print("   ", s)

print("\n=== readable text region -> file ===")
seg=d
# dump utf-8 readable
def dump(s,e,fn):
    t=seg[s:e].decode('utf-8','ignore')
    lines=[l.strip() for l in re.split(r'[\x00-\x08\x0b\x0c\x0e-\x1f]+',t) if len(l.strip())>=2]
    lines=[l for l in lines if re.search(r'[\u4e00-\u9fff]',l) or re.search(r'[a-zA-Z][a-zA-Z0-9_./\\\-]{4,}',l)]
    open(os.path.join(out,fn),'w',encoding='utf-8').write('\n'.join(lines))
    print(f"   {fn}: {len(lines)} lines")
dump(0x400, len(d), 'svc_strings.txt')
