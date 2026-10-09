import re
d = open(r'E:\disk-butler-svc.exe','rb').read()

print("=== narrow strings 0x84C00-0x86000 ===")
seg = d[0x84C00:0x86000]
for m in re.finditer(rb'[\x20-\x7e]{4,}', seg):
    print(f"  0x{0x84C00+m.start():06x}  {m.group().decode('latin1')}")

print("\n=== SDDL / descriptor ===")
for m in re.finditer(rb'D:(A|;)[\x20-\x7e]{5,140}', d):
    print("   ", m.group().decode('latin1'))

print("\n=== service/pipe related keywords ===")
for p in [b'PIPE', b'pipe', b'cmd', b'scan', b'req', b'resp', b'json', b'path', b'root',
          b'drive', b'letter', b'progress', b'error', b'payload', b'OK', b'DENIED', b'denied']:
    i = d.find(p)
    print(f"   {p.decode():10s} cnt={d.count(p):4d} first={'0x%x'%i if i>=0 else '-'}")

print("\n=== all distinct cmd-like tokens ===")
for m in set(re.findall(rb'"(?:cmd|type|op|action)"\s*:\s*"[a-z_]+"', d)):
    print("   ", m.decode('latin1'))
