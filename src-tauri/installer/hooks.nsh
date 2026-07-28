; Lucerna NSIS installer hooks — uninstall data-cleanup consent.
;
; The stock Tauri uninstaller removes only tracked binaries; the data root
; (instances, worlds, servers, mods — often gigabytes, possibly relocated via
; data-location.json) and Lucerna's OS-keyring credentials survive silently.
; Worse, Tauri's own "Delete the application data" checkbox wipes
; %APPDATA%\<id> wholesale, which erases data-location.json and orphans a
; relocated data root the user meant to keep.
;
; These hooks fix both, delegating all path/credential knowledge to the binary
; (`lucerna.exe --uninstall-cleanup`, see src/uninstall_cleanup.rs):
;   PREUNINSTALL  — ask (EN/RU) whether to also erase the named data, run the
;                   helper on consent. Never prompts in silent/passive/update
;                   runs, so scripted uninstalls and seamless updates are
;                   unaffected.
;   POSTUNINSTALL — if the app-data checkbox was ticked while game data was
;                   kept, restore the stashed data-location.json so a
;                   reinstall can still find the relocated data root.

Var LucernaCleanupChoice

!macro NSIS_HOOK_PREUNINSTALL
  StrCpy $LucernaCleanupChoice "0"
  ; Unconditional: POSTUNINSTALL reads $PLUGINSDIR on every non-update path,
  ; so it must be initialized (and the pointer stashed) even when the consent
  ; flow below is skipped. $PLUGINSDIR is temp and self-cleaning.
  InitPluginsDir
  CopyFiles /SILENT "$APPDATA\${BUNDLEID}\data-location.json" "$PLUGINSDIR\lucerna-data-location.json"
  ${If} $UpdateMode <> 1
  ${AndIf} $PassiveMode <> 1
  ${AndIfNot} ${Silent}
  ${AndIf} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    ; The template's own running-app check runs AFTER this hook — repeat it
    ; here so the helper never races a live launcher over the data root.
    !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

    ; Ask the binary what would remain: prints "SIZE|PATH", exit 0 = something
    ; exists, 2 = nothing to clean. String-compare the code: nsExec pushes
    ; "error" when the process cannot start, which numeric compares treat as 0.
    nsExec::ExecToStack '"$INSTDIR\${MAINBINARYNAME}.exe" --uninstall-cleanup --list'
    Pop $0 ; exit code
    Pop $1 ; "SIZE|PATH"
    ${If} $0 == "0"
      ; Split "$1" (= SIZE|PATH) at the first "|" into $R0 / $R1 by hand —
      ; core StrCpy/StrCmp only. The NSIS distribution Tauri bundles has no
      ; ${UnWordFind} (WordFunc's uninstaller variants are unavailable), so
      ; header-based splitting cannot be used here. "|" cannot appear in a
      ; Windows path, so the first hit is always the field separator.
      StrCpy $R0 ""
      StrCpy $R1 ""
      StrCpy $4 0
      lucerna_split_loop:
        StrCpy $5 $1 1 $4
        StrCmp $5 "" lucerna_split_done
        StrCmp $5 "|" lucerna_split_found
        IntOp $4 $4 + 1
        Goto lucerna_split_loop
      lucerna_split_found:
        StrCpy $R0 $1 $4
        IntOp $4 $4 + 1
        StrCpy $R1 $1 "" $4
      lucerna_split_done:
      ${If} $R1 != ""
        ${If} $LANGUAGE = 1049 ; Russian LCID — installer ships English + Russian
          StrCpy $2 "Будет удалено только само приложение Lucerna.$\r$\n$\r$\nИгровые данные — инстансы, миры, серверы, моды и настройки ($R0) — останутся здесь:$\r$\n$R1$\r$\n$\r$\nСохранённые данные входа в аккаунты также останутся в Диспетчере учётных данных Windows.$\r$\n$\r$\nУдалить и эти данные тоже? Это действие необратимо."
          StrCpy $3 "Часть данных удалить не удалось. Подробности — в журнале деинсталлятора."
        ${Else}
          StrCpy $2 "Only the Lucerna application itself will be removed.$\r$\n$\r$\nYour game data — instances, worlds, servers, mods and settings ($R0) — will remain at:$\r$\n$R1$\r$\n$\r$\nSaved account sign-ins will also remain in Windows Credential Manager.$\r$\n$\r$\nDelete this data as well? This cannot be undone."
          StrCpy $3 "Some data could not be removed. See the uninstaller log for details."
        ${EndIf}
        MessageBox MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2 "$2" /SD IDNO IDNO lucerna_cleanup_keep
          nsExec::ExecToLog '"$INSTDIR\${MAINBINARYNAME}.exe" --uninstall-cleanup'
          Pop $0
          ; Helper exit codes: 0 = clean full wipe (pointer restoration must
          ; NOT happen — it would dangle), 3 = success but data-location.json
          ; was deliberately kept (configured root unreachable), anything else
          ; = partial failure. On 3 and on failures the pointer stays
          ; restore-eligible ("2"), so surviving data remains discoverable.
          ${If} $0 == "0"
            StrCpy $LucernaCleanupChoice "1"
          ${Else}
            StrCpy $LucernaCleanupChoice "2"
            ${If} $0 != "3"
              MessageBox MB_OK|MB_ICONEXCLAMATION "$3" /SD IDOK
            ${EndIf}
          ${EndIf}
        lucerna_cleanup_keep:
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; The app-data checkbox deleted %APPDATA%\<id> (including the redirect file)
  ; while game data survived — either the user kept it ("0") or the wipe was
  ; partial / deliberately kept the pointer ("2"). Restore the pointer so the
  ; data root is still discoverable on reinstall. Only a clean full wipe ("1")
  ; must NOT restore (the pointer would dangle). When no redirect existed
  ; (default location), the stash is absent and this is a no-op.
  ${If} $UpdateMode <> 1
  ${AndIf} $DeleteAppDataCheckboxState = 1
  ${AndIf} $LucernaCleanupChoice != "1"
  ${AndIf} ${FileExists} "$PLUGINSDIR\lucerna-data-location.json"
    CreateDirectory "$APPDATA\${BUNDLEID}"
    CopyFiles /SILENT "$PLUGINSDIR\lucerna-data-location.json" "$APPDATA\${BUNDLEID}\data-location.json"
  ${EndIf}
!macroend
