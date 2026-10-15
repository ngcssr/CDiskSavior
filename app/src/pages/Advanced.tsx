import { useState, useCallback } from "react";
import { api_advanced } from "../api/commands";
import { ShieldCheck, RefreshCw, Zap, FileSearch } from "lucide-react";

type AnalyzeReport = {
  windowsTemp: { path: string; size: number };
  updateDownload: { path: string; size: number };
  totalSize: number;
};

function fmt(bytes: number): string {
  if (!bytes) return "0 B";
  const u = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${u[i]}`;
}

export default function Advanced() {
  const [analysis, setAnalysis] = useState<AnalyzeReport | null>(null);
  const [scanning, setScanning] = useState(false);
  const [cleaning, setCleaning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<string | null>(null);

  const analyze = useCallback(async () => {
    setScanning(true);
    setError(null);
    setResult(null);
    try {
      const r = await api_advanced.analyze_system_clean();
      setAnalysis(r);
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setScanning(false);
    }
  }, []);

  const deepCleanSystem = async () => {
    setCleaning(true);
    setError(null);
    try {
      await api_advanced.run_deep_clean_system();
      setResult("系统级清理完成（Windows Temp + 更新下载缓存）");
      await analyze();
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setCleaning(false);
    }
  };

  const [deepAnalysis, setDeepAnalysis] = useState<any>(null);
  const deepAnalyze = async () => {
    setScanning(true);
    try {
      const r = (await api_advanced.run_deep_analyze()) as any;
      setDeepAnalysis(r);
      setResult(`深度分析完成：${r?.items?.length ?? 0} 个可清理项，共 ${fmt(r?.totalSize ?? 0)}`);
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setScanning(false);
    }
  };
  const deepClean = async () => {
    setCleaning(true);
    setError(null);
    setResult(null);
    try {
      const r = (await api_advanced.run_deep_clean()) as any;
      setResult(`深度清理完成：清理 ${r?.items?.length ?? 0} 项，释放 ${fmt(r?.totalSize ?? 0)}`);
      setDeepAnalysis(null);
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setCleaning(false);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="border-b border-[var(--color-line)] px-6 py-4">
        <div className="flex items-center gap-2">
          <ShieldCheck size={20} className="text-[var(--color-primary)]" />
          <h1 className="text-xl font-bold">高级维护</h1>
        </div>
        <p className="mt-1 text-sm text-[var(--color-text-secondary)]">
          系统级深度清理（需管理员授权，UAC 取消则中止）
        </p>
      </div>

      <div className="flex-1 overflow-y-auto p-6">
        {error && (
          <div className="mb-4 rounded-xl border border-[var(--color-danger)]/30 bg-[var(--color-danger-bg-soft)] px-4 py-3 text-sm text-[var(--color-danger-text)]">
            {error}
          </div>
        )}
        {result && (
          <div className="mb-4 rounded-xl border border-[var(--color-safe)]/30 bg-[var(--color-safe)]/5 px-4 py-3 text-sm">
            ✓ {result}
          </div>
        )}

        {/* 系统清理分析 */}
        {analysis && (
          <section className="mb-6">
            <h2 className="mb-3 text-sm font-semibold text-[var(--color-text-secondary)]">系统空间分析</h2>
            <div className="grid grid-cols-2 gap-4">
              <div className="rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
                <div className="text-xs text-[var(--color-text-secondary)]">Windows 临时文件</div>
                <div className="mt-1 text-lg font-bold">{fmt(analysis.windowsTemp?.size ?? 0)}</div>
                <div className="mt-1 truncate font-mono text-xs text-[var(--color-text-secondary)]">{analysis.windowsTemp?.path}</div>
              </div>
              <div className="rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
                <div className="text-xs text-[var(--color-text-secondary)]">更新下载缓存</div>
                <div className="mt-1 text-lg font-bold">{fmt(analysis.updateDownload?.size ?? 0)}</div>
                <div className="mt-1 truncate font-mono text-xs text-[var(--color-text-secondary)]">{analysis.updateDownload?.path}</div>
              </div>
            </div>
            <div className="mt-3 rounded-xl bg-[var(--color-surface-hover)] px-4 py-2 text-sm">
              合计可释放：<strong>{fmt(analysis.totalSize ?? 0)}</strong>
            </div>
          </section>
        )}

        {/* 操作按钮 */}
        <section className="mb-6 flex flex-wrap gap-3">
          <button
            onClick={analyze}
            disabled={cleaning || scanning}
            className="flex items-center gap-2 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-2 text-sm disabled:opacity-50"
          >
            {scanning ? <RefreshCw size={16} className="animate-spin" /> : <FileSearch size={16} />}
            空间分析
          </button>
          <button
            onClick={deepCleanSystem}
            disabled={cleaning || scanning}
            className="flex items-center gap-2 rounded-lg bg-[var(--color-primary)] px-4 py-2 text-sm font-medium text-white disabled:opacity-50"
          >
            {cleaning ? <RefreshCw size={16} className="animate-spin" /> : <Zap size={16} />}
            {cleaning ? "清理中（等待 UAC）…" : "系统级深度清理"}
          </button>
          <button
            onClick={deepAnalyze}
            disabled={cleaning || scanning}
            className="flex items-center gap-2 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-2 text-sm disabled:opacity-50"
          >
            {scanning ? <RefreshCw size={16} className="animate-spin" /> : <FileSearch size={16} />}
            深度分析
          </button>
          {deepAnalysis && (
            <button
              onClick={deepClean}
              disabled={cleaning || scanning}
              className="flex items-center gap-2 rounded-lg border border-[var(--color-primary)]/40 bg-[var(--color-primary-soft)] px-4 py-2 text-sm font-medium text-[var(--color-primary)] disabled:opacity-50"
            >
              {cleaning ? <RefreshCw size={16} className="animate-spin" /> : <Zap size={16} />}
              {cleaning ? "清理中…" : `执行清理（${deepAnalysis.items?.length ?? 0} 项）`}
            </button>
          )}
        </section>

        {/* 说明 */}
        <section className="rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-4 text-sm">
          <h3 className="mb-2 font-semibold">操作说明</h3>
          <ul className="space-y-1.5 text-[var(--color-text-secondary)]">
            <li>· 系统级深度清理会弹出 UAC 管理员授权，取消则中止（不视为错误）</li>
            <li>· 清理范围：Windows\Temp + SoftwareDistribution\Download（更新缓存）</li>
            <li>· 清理后 Windows Update 可能需要重新下载部分更新</li>
            <li>· 深度分析复用 M3 清理引擎的安全护栏（R1 四层校验）</li>
          </ul>
        </section>
      </div>
    </div>
  );
}
