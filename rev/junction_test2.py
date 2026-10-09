# -*- coding: utf-8 -*-
"""补充实测：更多删除原语对 NTFS Junction 的行为。全程在 .tmp 下自建自删。"""
import os, shutil, subprocess, ctypes
from ctypes import wintypes

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".tmp", "jtest2"))
TARGET = os.path.join(ROOT, "REAL_G")
LINK = os.path.join(ROOT, "APP_JUNC")
k32 = ctypes.WinDLL("kernel32", use_last_error=True)
k32.GetFileAttributesW.argtypes = [wintypes.LPCWSTR]
k32.GetFileAttributesW.restype = wintypes.DWORD
k32.RemoveDirectoryW.argtypes = [wintypes.LPCWSTR]
k32.RemoveDirectoryW.restype = wintypes.BOOL


def setup():
    if os.path.exists(ROOT):
        shutil.rmtree(ROOT, ignore_errors=True)
    os.makedirs(os.path.join(TARGET, "sub"))
    open(os.path.join(TARGET, "PRECIOUS.txt"), "w").write("survive")
    open(os.path.join(TARGET, "sub", "m.dat"), "w").write("x" * 500)
    subprocess.run(["cmd", "/C", "mklink", "/J", LINK, TARGET],
                   capture_output=True, encoding="gbk", errors="replace")


def intact():
    return (os.path.exists(os.path.join(TARGET, "PRECIOUS.txt"))
            and os.path.exists(os.path.join(TARGET, "sub", "m.dat")))


def run(name, fn):
    setup()
    try:
        fn(); err = None
    except Exception as e:
        err = f"{type(e).__name__}: {str(e)[:70]}"
    print(f"\n[{name}]")
    print(f"  结果: {'OK' if err is None else err}")
    print(f"  链接残留: {os.path.exists(LINK)}   目标数据: {'✔ 完好' if intact() else '✘ 被删(穿透!)'}")


def m_del():
    subprocess.run(["cmd", "/C", "del", "/s", "/q", LINK],
                   capture_output=True, encoding="gbk", errors="replace")


def m_ps():
    ps = "powershell"
    subprocess.run([ps, "-NoProfile", "-Command",
                    f"Remove-Item -LiteralPath '{LINK}' -Recurse -Force -ErrorAction Stop"],
                   capture_output=True, encoding="gbk", errors="replace")


def m_robocopy():
    empty = os.path.join(ROOT, "empty")
    os.makedirs(empty, exist_ok=True)
    subprocess.run(["robocopy", empty, LINK, "/MIR", "/NFL", "/NDL", "/NJH", "/NJS"],
                   capture_output=True, encoding="gbk", errors="replace")


def m_removedirw():
    if not k32.RemoveDirectoryW(LINK):
        raise OSError(ctypes.get_last_error(), "RemoveDirectoryW failed")


if __name__ == "__main__":
    print("=" * 66)
    print("补充实测：更多删除原语对 junction 的行为")
    print("=" * 66)
    run("⑤ del /s /q junction   (cmd 经典陷阱)", m_del)
    run("⑥ 删除并强制递归（另一 shell 实现）", m_ps)
    run("⑦ robocopy 空目录 /MIR (镜像清空)", m_robocopy)
    run("⑧ RemoveDirectoryW (仅删链接，推荐原语)", m_removedirw)
    if os.path.exists(ROOT):
        shutil.rmtree(ROOT, ignore_errors=True)
    print("\n清理完成")
