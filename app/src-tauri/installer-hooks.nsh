; ============================================================================
; CDisk Savior NSIS 安装器钩子（合并设计 §3.1 旧名迁移）
;
; PREINSTALL：清除旧 disk-butler 遗留的 LocalSystem 孤儿服务
; DiskButlerScanSvc——旧版卸载后服务无人管理，必须在新版安装前清理。
; 全部 best-effort：不存在/已删除时静默继续，不阻断安装。
; ============================================================================

!macro NSIS_HOOK_PREINSTALL
  ; 旧服务存在性探测（sc query 退出码 0 = 存在）
  nsExec::ExecToStack 'sc.exe query DiskButlerScanSvc'
  Pop $0
  ${If} $0 == 0
    ; 停止（可能正在运行；失败也继续删除）
    nsExec::ExecToStack 'sc.exe stop DiskButlerScanSvc'
    Pop $1
    ; 删除服务标记
    nsExec::ExecToStack 'sc.exe delete DiskButlerScanSvc'
    Pop $1
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; C1：安装 MFT 扫描服务（perMachine 已是管理员；OnDemand 注册，不启动常驻进程）
  ; 失败不阻断安装——主程序扫描会回退 jwalk 慢速遍历，设置页可一键修复
  IfFileExists "$INSTDIR\cdisk-savior-svc.exe" 0 done_svc_install
  ExecWait '"$INSTDIR\cdisk-savior-svc.exe" install' $0
  done_svc_install:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; C1：先卸载 MFT 扫描服务（停止+删除注册项），再走文件清理
  IfFileExists "$INSTDIR\cdisk-savior-svc.exe" 0 done_svc_uninstall
  ExecWait '"$INSTDIR\cdisk-savior-svc.exe" uninstall' $0
  done_svc_uninstall:

  ; Q8 决策：卸载不自动恢复迁移（目标盘可能缺失，自动恢复有数据丢失风险），
  ; 但检测到未恢复迁移记录时警告提示（迁移历史存在即视为有未恢复项——
  ; 恢复完成的记录会从历史中移除）。
  ; 局限：perMachine 卸载以管理员身份运行，读取的是该管理员账户的 APPDATA，
  ; 其他账户的迁移记录探测不到（已在设计文档 Q9 注明）。
  IfFileExists "$APPDATA\com.cdisksavior.app\migration-history.json" 0 done_uninstall_check
  FileOpen $0 "$APPDATA\com.cdisksavior.app\migration-history.json" r
  FileSeek $0 0 END $1
  FileClose $0
  IntCmp $1 8 done_uninstall_check done_uninstall_check
  MessageBox MB_ICONEXCLAMATION|MB_OK "检测到未恢复的空间腾挪（迁移）记录。$\r$\n$\r$\n卸载不会自动恢复已迁移到其他盘的目录，junction 链接将残留。$\r$\n建议先运行 CDisk Savior 恢复全部迁移后再卸载。"
  done_uninstall_check:
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
