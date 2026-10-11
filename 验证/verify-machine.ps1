# ============================================================================
# CDisk Savior 实机自动化验证脚本（D 级）
#
# 用法：以管理员身份打开 PowerShell，执行：
#   powershell -ExecutionPolicy Bypass -File .\verify-machine.ps1
# 结果输出到同目录 verify-report.txt，把该文件交回分析。
#
# 覆盖：安装完整性 / MFT 服务注册 / 管道端到端秒扫 / 双实例互斥 /
#       longPathAware 嵌入 / 符号剥离 / 侧车二进制。
# 不覆盖（GUI/交互项）：见 manual-checklist.md。
# ============================================================================

$ErrorActionPreference = 'Continue'
$report = @()
$script:pass = 0; $script:fail = 0; $script:skip = 0

function Check($name, [bool]$ok, $detail = '') {
    if ($ok) { $script:pass++; $line = "[PASS] $name  $detail" }
    else     { $script:fail++; $line = "[FAIL] $name  $detail" }
    $script:report += $line; Write-Host $line
}
function Skip($name, $why) {
    $script:skip++; $line = "[SKIP] $name  $why"
    $script:report += $line; Write-Host $line
}

$installDir = "$env:ProgramFiles\CDisk Savior"
$exe = "$installDir\cdisk-savior.exe"
$svc = "$installDir\cdisk-savior-svc.exe"

Write-Host "===== CDisk Savior 实机验证 $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') ====="
$report += "===== CDisk Savior 实机验证 $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') ====="

# ---- 0. 管理员权限 ----
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
Check "管理员权限" $isAdmin
if (-not $isAdmin) { Write-Host "本脚本需要管理员权限，部分检查将不可靠。" -ForegroundColor Yellow }

# ---- 1. 安装完整性 ----
Check "主程序存在" (Test-Path $exe) $exe
Check "MFT 服务侧车存在" (Test-Path $svc) $svc
Check "无 PDB 调试符号旁挂" (-not (Test-Path "$installDir\*.pdb"))

# ---- 2. longPathAware 清单嵌入（R9/V20）----
if (Test-Path $exe) {
    $bytes = [System.IO.File]::ReadAllBytes($exe)
    $text = [System.Text.Encoding]::ASCII.GetString($bytes)
    Check "longPathAware 已嵌入主程序" ($text.Contains('longPathAware'))
    # strip=symbols 生效验证：release 不应残留 pdb 路径引用
    Check "符号已剥离（无 .pdb 引用）" (-not $text.Contains('.pdb'))
} else { Skip "清单/符号检查" "主程序未安装" }

# ---- 3. MFT 服务注册（C1）----
$svcObj = Get-Service -Name 'CDiskSaviorSvc' -ErrorAction SilentlyContinue
Check "CDiskSaviorSvc 服务已注册" ($null -ne $svcObj) $(if ($svcObj) { "Status=$($svcObj.Status) StartType=$($svcObj.StartType)" })

# ---- 4. 管道端到端 MFT 秒扫（服务按需启动）----
$pipeResult = $null
try {
    $pipe = New-Object System.IO.Pipes.NamedPipeClientStream('.', 'cdisk-savior-scan', [System.IO.Pipes.PipeDirection]::InOut)
    $pipe.Connect(15000)  # 15s 等待服务按需拉起
    $writer = New-Object System.IO.StreamWriter($pipe)
    $writer.AutoFlush = $true
    $reader = New-Object System.IO.StreamReader($pipe)
    $writer.WriteLine('{"cmd":"scan","root":"C:\\"}')
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $lastLine = $null; $done = $false
    while ($sw.Elapsed.TotalSeconds -lt 120) {
        if ($pipe.IsMessageAvailable -or $true) {
            $line = $reader.ReadLine()
            if ($null -eq $line) { break }
            $lastLine = $line
            if ($line -match '"type"\s*:\s*"done"') { $done = $true; break }
            if ($line -match '"type"\s*:\s*"error"') { break }
        }
    }
    $sw.Stop()
    if ($done -and $lastLine -match '"size"\s*:\s*(\d+)') {
        Check "MFT 管道秒扫端到端" $true ("耗时 {0:N1}s" -f $sw.Elapsed.TotalSeconds)
    } else {
        $tail = if ($lastLine) { $lastLine } else { '<空>' }
        Check "MFT 管道秒扫端到端" $false ("末行: " + $tail.Substring(0, [Math]::Min(120, $tail.Length)))
    }
    $pipe.Dispose()
} catch {
    Check "MFT 管道秒扫端到端" $false $_.Exception.Message
}

# ---- 5. 双实例互斥（§8 场景9）----
if (Test-Path $exe) {
    $p1 = Start-Process $exe -PassThru
    Start-Sleep -Seconds 4
    Start-Process $exe | Out-Null
    Start-Sleep -Seconds 3
    $procs = Get-Process -Name 'cdisk-savior' -ErrorAction SilentlyContinue
    Check "双实例互斥（第二实例被拒绝）" ($procs.Count -le 1) ("存活进程数: $($procs.Count)")
    $procs | Stop-Process -Force -ErrorAction SilentlyContinue
} else { Skip "双实例互斥" "主程序未安装" }

# ---- 6. 迁移历史自愈场景（V2 用例前半：原子写）----
# 损坏的快照文件应被自愈逻辑隔离而不是崩溃——该逻辑在单元测试已覆盖，
# 此处验证快照目录可写与文件结构。
$snapDir = "$env:LOCALAPPDATA\CDiskSavior"
if (Test-Path $snapDir) {
    $snaps = Get-ChildItem $snapDir -Filter 'snapshot-*.json' -ErrorAction SilentlyContinue
    $ok = $true
    foreach ($s in $snaps) {
        try { $null = Get-Content $s.FullName -Raw | ConvertFrom-Json } catch { $ok = $false }
    }
    Check "快照文件 JSON 结构完好" $ok ("快照数: $($snaps.Count)")
} else { Skip "快照文件检查" "尚未创建基线（GUI 操作后重跑可覆盖）" }

Write-Host ""
Write-Host "===== 结果：PASS=$pass FAIL=$fail SKIP=$skip ====="
$report += "===== 结果：PASS=$pass FAIL=$fail SKIP=$skip ====="
$report | Out-File -FilePath "$PSScriptRoot\verify-report.txt" -Encoding utf8
Write-Host "报告已写入 $PSScriptRoot\verify-report.txt" -ForegroundColor Cyan
