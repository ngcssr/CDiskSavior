import re, collections
d = open(r'E:\disk-butler.exe','rb').read()

print("=== Rust source paths (project) ===")
proj = collections.Counter()
for m in re.finditer(rb'[A-Za-z0-9_\\/\.:\-]{0,80}disk_butler[A-Za-z0-9_\\/\.\-]{0,80}', d):
    proj[m.group().decode('latin1')] += 1
for k,v in proj.most_common(60):
    print(f"  {v:3d}  {k}")

print("\n=== .rs file references (non-stdlib) ===")
rs = collections.Counter()
for m in re.finditer(rb'[A-Za-z0-9_\\/\.\-]{3,90}\.(?:rs|toml|json|ini|log|pdb)', d):
    s = m.group().decode('latin1')
    if '/rustc/' in s or 'library\\' in s or 'registry\\src' in s: continue
    rs[s] += 1
for k,v in rs.most_common(80):
    print(f"  {v:3d}  {k}")

print("\n=== src\\ paths ===")
for m in re.finditer(rb'src\\[A-Za-z0-9_\\\.\-]{2,70}\.rs', d):
    print("   ", m.group().decode('latin1'))
