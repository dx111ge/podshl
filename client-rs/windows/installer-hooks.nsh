; Added to Tauri's NSIS installer (tauri.conf.json, bundle.windows.nsis).
;
; The uninstaller's "delete application data" box removes the folders named
; after the bundle identifier. This client keeps its state under
; %APPDATA%\podshl instead — the model settings, the pseudonym secret, the log
; head it last accepted, the vendor ledger — so without this the box would say
; it deleted the data and leave all of it behind.
;
; Not removed: an API key, which lives in Windows Credential Manager under
; "de.podshl.client" and is the person's to delete there.

;
; The helper that performs the changes needing administrator rights
; (src/bin/podshl-elevate.rs). It goes next to the client, where the client
; looks for it and nowhere else. `scripts/build/build_windows_installer.ps1`
; builds it and copies it beside this file. The folder is taken when this file
; is included, not inside the macro, which is expanded in Tauri's own script.
!define PODSHL_HOOKS_DIR "${__FILEDIR__}"
!macro NSIS_HOOK_POSTINSTALL
  SetOutPath "$INSTDIR"
  File "${PODSHL_HOOKS_DIR}\podshl-elevate.exe"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\podshl-elevate.exe"
  RMDir "$INSTDIR"
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    RMDir /r "$APPDATA\podshl"
  ${EndIf}
!macroend
