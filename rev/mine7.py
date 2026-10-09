import struct, os, hashlib
d = open(r'E:\disk-butler.exe','rb').read()
out = r'C:\Users\Administrator\WorkBuddy\2026-10-09-16-21-42\rev\icons'
os.makedirs(out, exist_ok=True)
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
            data=d[r2o(drva):r2o(drva)+dsz]
            if path[0]==3:
                fn=os.path.join(out,f"icon_{data[16:20].hex()}_{path[1]}.png")
                open(fn,'wb').write(data)
                print(f"  RT_ICON id={path[1]} {dsz}B -> {os.path.basename(fn)}")
walk(0,[])
print("\nsha256(exe) =", hashlib.sha256(d).hexdigest())
print("md5(exe)    =", hashlib.md5(d).hexdigest())
print("size        =", len(d))
