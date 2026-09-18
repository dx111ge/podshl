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
  ; Two things the client registers *outside* its own folder, which therefore
  ; outlive it unless they are named here. Neither is the person's data, so
  ; neither waits on the "delete application data" box — they are this
  ; program's marks on the system, and the program is going.
  ;
  ;   * the daily task `repairs install-hook` creates. Left behind, Windows
  ;     goes on running it once a day against an executable that is no longer
  ;     there. `remove-hook` before uninstalling would have taken it, and
  ;     nobody uninstalling a program thinks to do that first.
  ;   * the notification identity `de.podshl.client`, which is what makes a
  ;     toast say PODSHL rather than "Windows PowerShell". One key in the
  ;     person's own hive, which would otherwise sit in their notification
  ;     settings naming a program they removed.
  ;
  ; Not on an update: the new version wants both of them.
  ${If} $UpdateMode <> 1
    nsExec::Exec 'schtasks /Delete /F /TN "PODSHL\Repairs review"'
    Pop $0
    DeleteRegKey HKCU "Software\Classes\AppUserModelId\de.podshl.client"
  ${EndIf}
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    RMDir /r "$APPDATA\podshl"
  ${EndIf}
!macroend
