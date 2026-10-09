import re, os
d = open(r'E:\disk-butler.exe','rb').read()
out = r'C:\Users\Administrator\WorkBuddy\2026-10-09-16-21-42\rev'

def dump(start, end, fname, minlen=2):
    seg = d[start:end]
    txt = seg.decode('utf-8','ignore')
    # normalized: split on control chars / long ascii noise
    lines = re.split(r'[\x00-\x08\x0b\x0c\x0e-\x1f]+', txt)
    keep = []
    for l in lines:
        l = l.strip()
        # keep if contains CJK or looks like a path/id
        if len(l) < minlen: continue
        if re.search(r'[\u4e00-\u9fff]', l) or re.search(r'[a-z0-9][a-z0-9_./\\\-]{3,}', l):
            keep.append(l)
    open(os.path.join(out,fname),'w',encoding='utf-8').write('\n'.join(keep))
    print(f"{fname}: {len(keep)} lines")

dump(0x7d4000, 0x7d6000, 'cat_service.txt')
dump(0x7d6000, 0x7d9000, 'cat_rules_a.txt')
dump(0x7d9000, 0x7dc000, 'cat_rules_b.txt')
dump(0x7dc000, 0x7e1000, 'cat_rules_c.txt')
dump(0x7e1000, 0x7e7000, 'cat_rules_d.txt')
dump(0x7cc400, 0x7ce400, 'cat_browser.txt')
