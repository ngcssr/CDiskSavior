# -*- coding: utf-8 -*-
"""
实测：各种递归删除方法对 NTFS Junction 的行为
结论直接决定「删除引擎」必须用哪个原语 —— 用错会穿透 junction 删除目标盘上的真实数据。

安全性：全程在工作区 .tmp 下自建自删，不触碰任何用户数据。
"""
import ctypes, os, shutil, subprocess, sys
from ctypes import wintypes

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".tmp", "jtest"))
TARGET = os.path.join(ROOT, "real_data_on_G")      # 类比 G:\Yugongyipan\Local\Foo
LINK = os.path.join(ROOT, "appdata_junction")      # 类比 C:\Users\x\AppData\Local\Foo

FILE_ATTRIBUTE_REPARSE_POINT = 0x400
INVALID_FILE_ATTRIBUTES = 0xFFFFFFFF

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
k32.GetFileAttributesW.argtypes = [wintypes.LPCWSTR]
k32.GetFileAttributesW.restype = wintypes.DWORD
k32.RemoveDirectoryW.argtypes = [wintypes.LPCWSTR]
k32.RemoveDirectoryW.restype = wintypes.BOOL


def setup():
    if os.path.exists(ROOT):
        shutil.rmtree(ROOT, ignore_errors=True)
    os.makedirs(os.path.join(TARGET, "sub"), exist_ok=True)
    # 真实数据（可识别的内容）
    open(os.path.join(TARGET, "PRECIOUS.txt"), "w", encoding="utf-8").write("must survive")
    open(os.path.join(TARGET, "sub", "more.dat"), "w").write("x" * 100)
    # 建 junction: LINK -> TARGET
    r = subprocess.run(["cmd", "/C", "mklink", "/J", LINK, TARGET],
                       capture_output=True, encoding='gbk', errors='replace')
    print(f"  mklink 返回码={r.returncode}  {r.stdout.strip() or r.stderr.strip()}")


def target_intact():
    """目标数据是否仍完整"""
    return (os.path.exists(os.path.join(TARGET, "PRECIOUS.txt"))
            and os.path.exists(os.path.join(TARGET, "sub", "more.dat")))


def is_reparse(path):
    a = k32.GetFileAttributesW(path)
    return a != INVALID_FILE_ATTRIBUTES and bool(a & FILE_ATTRIBUTE_REPARSE_POINT)


def py_islink(path):
    """Python os.path.islink 是否把 junction 识别为链接"""
    return os.path.islink(path)


def py_isjunction(path):
    try:
        return os.path.isjunction(path)
    except AttributeError:
        return "N/A(py<3.12)"


def report(method, fn):
    setup()
    print(f"\n[{method}]")
    print(f"  islink={py_islink(LINK)}  isjunction={py_isjunction(LINK)}  reparse点={is_reparse(LINK)}")
    try:
        fn()
        err = None
    except Exception as e:
        err = f"{type(e).__name__}: {e}"
    print(f"  执行结果: {'成功' if err is None else err}")
    print(f"  链接是否还在: {os.path.exists(LINK) or os.path.lexists(LINK) if hasattr(os.path,'lexists') else os.path.exists(LINK)}")
    print(f"  目标数据是否完好: {'✔ 完好' if target_intact() else '✘ 已被删除(穿透!)'}"
          f"   目标目录是否还在: {os.path.exists(TARGET)}")


def m_rmtree():
    shutil.rmtree(LINK)


def m_rmdir():
    subprocess.run(["cmd", "/C", "rmdir", "/s", "/q", LINK], capture_output=True, encoding='gbk', errors='replace')


def m_remove_dir_all_emul():
    """模拟 Rust std::fs::remove_dir_all 的语义：
       先 symlink 判定 -> 是链接则只删链接；否则递归"""
    try:
        st = os.lstat(LINK)
        is_link = (st.st_mode & 0o170000) == 0o120000
    except OSError:
        is_link = False
    if is_link:
        os.rmdir(LINK)
    else:
        shutil.rmtree(LINK)


def m_removedirectoryw():
    """正确原语：RemoveDirectoryW —— 只删目录/链接本身，不递归"""
    ok = k32.RemoveDirectoryW(LINK)
    if not ok:
        raise OSError(ctypes.get_last_error(), "RemoveDirectoryW failed")


def main():
    print("=" * 70)
    print("NTFS Junction 递归删除行为实测")
    print("=" * 70)
    print(f"目标(真实数据): {TARGET}")
    print(f"链接(junction) : {LINK}")

    report("① Python shutil.rmtree(junction)", m_rmtree)
    report("② cmd rmdir /s /q junction   (disk-butler 曾用 shell 语义)", m_rmdir)
    report("③ 模拟 Rust remove_dir_all 语义(lstat 判链接)", m_remove_dir_all_emul)
    report("④ RemoveDirectoryW (正确原语)", m_removedirectoryw)

    # 清理
    if os.path.exists(ROOT):
        shutil.rmtree(ROOT, ignore_errors=True)
    print("\n" + "=" * 70)
    print("测试环境已清理")


if __name__ == "__main__":
    main()
