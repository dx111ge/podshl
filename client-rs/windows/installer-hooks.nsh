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

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    RMDir /r "$APPDATA\podshl"
  ${EndIf}
!macroend
