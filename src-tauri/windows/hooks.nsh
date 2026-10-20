; User data contract: the uninstaller keeps models and user content by default. The app's
; uninstall window (`harness-studio --uninstall --installer`) lets the user choose; the
; `harness cleanup` helper (fallback) removes only app-owned files (engines, environments, caches, logs,
; WebView2 data) and deletes data only when the user ticks the data checkbox. It refuses
; redirected roots and never follows junctions. No script recursively deletes APPDATA itself.
; Kaldırıcı kancası: uygulamanın "PATH'e Ekle" ile kullanıcı PATH'ine (HKCU\Environment\Path)
; yazdığı kurulum klasörü girdisini siler. Girdi, `harness.exe` sidecar'ının klasörüdür
; (= $INSTDIR); eşleşme büyük/küçük harfe ve sondaki ters eğik çizgiye duyarsızdır
; (crates/harness-core/src/integrations/cli_install.rs ile aynı kural). Diğer girdiler bayt bayt
; korunur; girdi yoksa hiçbir şey yazılmaz. NSIS dizgi sınırı (1024) yüzünden değer 1000
; karaktere ulaştıysa okuma kesilmiş olabilir: o durumda hiç dokunulmaz.

Function un.HSRemoveCliPath
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  Push $7
  Push $8

  ReadRegStr $0 HKCU "Environment" "Path"
  StrCmp $0 "" hs_done
  StrLen $3 $0
  IntCmp $3 1000 hs_done hs_len_ok hs_done
  hs_len_ok:

  ; Hedef: $INSTDIR, sondaki ters eğik çizgiler atılmış.
  StrCpy $2 $INSTDIR
  hs_trim_target:
    StrCpy $3 $2 1 -1
    StrCmp $3 "\" 0 hs_target_ready
    StrCpy $2 $2 -1
    Goto hs_trim_target
  hs_target_ready:
  StrCmp $2 "" hs_done

  StrCpy $1 ""      ; sonuç
  StrCpy $4 "0"     ; değişti mi
  StrCpy $5 "1"     ; sonuçta ilk girdi mi

  hs_next_entry:
    StrCpy $6 0
    hs_scan:
      StrCpy $7 $0 1 $6
      StrCmp $7 "" hs_last
      StrCmp $7 ";" hs_found
      IntOp $6 $6 + 1
      Goto hs_scan
    hs_found:
      StrCpy $8 $0 $6
      IntOp $6 $6 + 1
      StrCpy $0 $0 "" $6
      StrCpy $7 "more"
      Goto hs_have_entry
    hs_last:
      StrCpy $8 $0
      StrCpy $0 ""
      StrCpy $7 "last"
    hs_have_entry:

    ; $3 = girdi, sondaki ters eğik çizgiler atılmış (karşılaştırma için)
    StrCpy $3 $8
    hs_trim_entry:
      StrCpy $6 $3 1 -1
      StrCmp $6 "\" 0 hs_entry_ready
      StrCpy $3 $3 -1
      Goto hs_trim_entry
    hs_entry_ready:

    StrCmp $3 $2 0 hs_keep   ; StrCmp büyük/küçük harfe duyarsızdır
      StrCpy $4 "1"
      Goto hs_after_entry
    hs_keep:
      StrCmp $5 "1" 0 hs_append
        StrCpy $1 $8
        StrCpy $5 "0"
        Goto hs_after_entry
      hs_append:
        StrCpy $1 "$1;$8"
    hs_after_entry:
    StrCmp $7 "more" hs_next_entry

  StrCmp $4 "1" 0 hs_done
  WriteRegExpandStr HKCU "Environment" "Path" $1
  SendMessage 0xFFFF 0x001A 0 "STR:Environment" /TIMEOUT=5000

  hs_done:
  Pop $8
  Pop $7
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

!macro NSIS_HOOK_PREINSTALL
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Interactive uninstall: the app's own uninstall window asks what to remove (everything, or
  ; item by item), removes it, and exits with 0 (continue) or 2 (the user cancelled). Silent runs
  ; (including the one the window itself starts for the app files), updates and a window that
  ; cannot start fall back to the plain cleanup below: models and user content stay unless the
  ; "Delete the application data" checkbox is ticked.
  StrCpy $R7 "fallback"
  ; The window starts the silent uninstaller for the app files with HARNESS_UNINSTALL_DONE=1: the
  ; user already chose what to keep, so the plain cleanup must not delete anything more.
  ReadEnvStr $R6 "HARNESS_UNINSTALL_DONE"
  ${If} $UpdateMode <> 1
  ${AndIfNot} ${Silent}
  ${AndIf} ${FileExists} "$INSTDIR\harness-studio.exe"
    ExecWait '"$INSTDIR\harness-studio.exe" --uninstall --installer' $R7
    ${If} $R7 == "2"
      Quit
    ${EndIf}
  ${EndIf}
  ; Güncellemede (eski sürümü kaldırıp yenisini kuran akış) kullanıcının PATH girdisi kalmalı.
  ; Asıl yol: dosyalar silinmeden CLI girdiyi Rust tarafında siler (uzun PATH'te de çalışır;
  ; NSIS dizgileri 1024 karakterde kesilir, 10.10 testinde PATH 1534 karakterdi).
  StrCpy $R9 "1"
  ${If} $UpdateMode <> 1
  ${AndIf} ${FileExists} "$INSTDIR\harness.exe"
    nsExec::Exec '"$INSTDIR\harness.exe" remove-from-path'
    Pop $R9
  ${EndIf}
  ; Fallback: remove only app-owned files (engines, environments, caches, logs, WebView2 data).
  ; The helper validates the root by its identifier and never follows junctions. It is skipped
  ; when the data folder is already gone, so it cannot recreate it.
  ${If} $UpdateMode <> 1
  ${AndIf} $R7 != "0"
  ${AndIf} $R6 != "1"
  ${AndIf} ${FileExists} "$INSTDIR\harness.exe"
  ${AndIf} ${FileExists} "$APPDATA\com.harnessstudio.app\*.*"
    ${If} $DeleteAppDataCheckboxState = 1
      nsExec::Exec '"$INSTDIR\harness.exe" cleanup --yes --delete-data'
    ${Else}
      nsExec::Exec '"$INSTDIR\harness.exe" cleanup --yes'
    ${EndIf}
    Pop $R8
  ${EndIf}
  ; Otomatik başlatma kaydı.
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Yedek: CLI çalışmadıysa kısa PATH'ler için NSIS içi temizlik.
  ${If} $UpdateMode <> 1
  ${AndIf} $R9 != "0"
    Call un.HSRemoveCliPath
  ${EndIf}
!macroend
