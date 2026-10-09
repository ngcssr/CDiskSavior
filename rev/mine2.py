import re, sys
d = open(r'E:\disk-butler.exe','rb').read()

def show(pats, label, n=8, ctx=60):
    if isinstance(pats, bytes): pats=[pats]
    hits=[]
    for p in pats:
        for m in re.finditer(re.escape(p), d):
            hits.append((m.start(), p))
    print(f"--- {label}: {len(hits)} hits")
    for o,p in hits[:n]:
        s = d[max(0,o-ctx):o+ctx+len(p)]
        print(f"   0x{o:x} {s.decode('latin1','replace')!r}")

print("=== URLs ===")
show([b'https://'], 'https', n=25)
show([b'http://'], 'http', n=20)
print("\n=== emails ===")
seen=set()
for m in re.finditer(rb'[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,6}', d):
    v=m.group().decode('latin1')
    if v not in seen:
        seen.add(v); print("   ", v)

print("\n=== process spawn / elevation ===")
show([b'ShellExecuteW', b'ShellExecuteExW', b'runas', b'IsUserAnAdmin', b'TokenElevation',
      b'CreateProcessW', b'cmd.exe', b'CreateProcessAsUser'], 'proc')
show([b'powershell'], 'ps')
show([b'cmd /', b'/c ', b'start ""'], 'cmdline')

print("\n=== delete / recycle ===")
show([b'SHFileOperation', b'IFileOperation', b'SHQueryRecycleBin', b'RemoveDirectoryW',
      b'DeleteFileW', b'MoveFileExW', b'NtSetInformationFile', b'FSCTL_', b'DeviceIoControl',
      b'GetDiskFreeSpace', b'K32GetPerformanceInfo'], 'delete')

print("\n=== registry / startup ===")
show([b'CurrentVersion\\Run', b'RegSetValue', b'RegOpenKey', b'RegCreateKey', b'RegDelete',
      b'StartupApproved', b'Software\\Microsoft'], 'reg')

print("\n=== named pipe / service ===")
show([b'disk-butler-scan', b'\\\\?\\pipe', b'\\\\.\\pipe', b'CreateNamedPipe', b'NtCreateNamedPipeFile',
      b'ConnectNamedPipe', b'WaitNamedPipe', b'DiskButlerScanSvc', b'disk-butler-svc'], 'pipe', n=12)

print("\n=== CSP / capabilities ===")
for m in re.finditer(rb'default-src[^\x00]{0,220}', d):
    print("   CSP:", m.group().decode('latin1','replace')[:240])
    break
show([b'opener', b'allow-open', b'commandScope'], 'plugin', n=6)
