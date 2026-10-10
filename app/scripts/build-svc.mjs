// 构建扫描服务 sidecar：cargo release 构建 cdisk-savior-svc，
// 复制为 Tauri externalBin 约定名（<name>-<target-triple>.exe）
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const triple = "x86_64-pc-windows-msvc";

execFileSync(
  "cargo",
  ["build", "--release", "--features", "custom-protocol", "--bin", "cdisk-savior-svc"],
  // 必须从 src-tauri 目录执行：从 app/ 用 --manifest-path 会触发
  // schemars/indexmap 1.9 的 E0107（autocfg 探测随调用目录变化，原因未明）
  { stdio: "inherit", cwd: join(root, "src-tauri") }
);

const outDir = join(root, "src-tauri/binaries");
mkdirSync(outDir, { recursive: true });
copyFileSync(
  join(root, "src-tauri/target/release/cdisk-savior-svc.exe"),
  join(outDir, `cdisk-savior-svc-${triple}.exe`)
);
console.log("[build:svc] sidecar ready:", join(outDir, `cdisk-savior-svc-${triple}.exe`));
