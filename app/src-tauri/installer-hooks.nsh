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
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
