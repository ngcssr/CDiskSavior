import re, collections
d = open(r'E:\disk-butler.exe','rb').read()

def show(pats, label, n=10, ctx=70):
    if isinstance(pats, bytes): pats=[pats]
    hits=[]
    for p in pats:
        for m in re.finditer(re.escape(p), d):
            hits.append((m.start(), p))
    print(f"--- {label}: {len(hits)} hits")
    for o,p in hits[:n]:
        s = d[max(0,o-ctx):o+ctx+len(p)]
        print(f"   0x{o:x} {s.decode('utf-8','replace')!r}")

print("=== embedded PE check (MZ headers after offset 0) ===")
cnt=0
for m in re.finditer(rb'MZ', d):
    o=m.start()
    if o<2: continue
    if o+0x40<=len(d) and d[o+0x3c:o+0x40]:
        import struct
        try:
            pe=struct.unpack_from('<I', d, o+0x3c)[0]
            if 0<pe<0x400 and d[o+pe:o+pe+4]==b'PE\x00\x00':
                print(f"   embedded PE @0x{o:x}"); cnt+=1
        except Exception: pass
print("   embedded PE count:", cnt)

print("\n=== progress/schema field names ===")
show([b'filesScanned', b'bytesScanned', b'percent', b'phase', b'positionsize'], 'progress', n=4)

print("\n=== pipe protocol / scan service ===")
show([b'DiskButlerScanSvc', b'disk-butler-scan', b'disk-butler-svc.exe', b'instal'], 'pipe', n=10, ctx=110)

print("\n=== cleanup / rollback ===")
show([b'rollback', b'Rollback', b'\xe5\x9b\x9e\xe6\x94\xb6\xe7\xab\x99', b'\xe5\x9b\x9e\xe6\xbb\x9a', b'\xe6\x81\xa2\xe5\xa4\x8d'], 'rollback', n=8, ctx=90)

print("\n=== path templates ===")
for p in [b'%LOCALAPPDATA%', b'%APPDATA%', b'%TEMP%', b'%SystemRoot%', b'%ProgramData%', b'%USERPROFILE%',
          b'AppData\\\\Local', b'node_modules', b'\\\\.npm', b'Cache', b'CrashDumps', b'D3DSCache',
          b'ShaderCache', b'GPUCache', b'Code Cache', b'WER', b'WinSxS', b'Temp']:
    i=d.find(p)
    print(f"   {p.decode('utf-8','replace'):20s} {'@0x%x'%i if i>=0 else 'NOT FOUND'}  cnt={d.count(p)}")

print("\n=== misc config ===")
show([b'tauri.conf', b'identifier', b'com.diskbutler', b'productName', b'version', b'Cargo.toml', b'cleanup-stats.json'], 'cfg', n=6, ctx=80)
