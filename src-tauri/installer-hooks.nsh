; wispzip NSIS installer hooks — Windows Explorer context-menu integration.
; Registered by the Tauri NSIS bundler when `windows.nsis.installerHooks`
; points at this file. Runs elevated (perMachine install), so HKCR is writable.

; Registry layout:
;   AllFilesystemObjects\shell\wispzip-compress       -> compress-to-ZIP item (files + folders)
;   SystemFileAssociations\<ext>\shell\wispzip-here   -> extract-to-current-folder item
;   SystemFileAssociations\<ext>\shell\wispzip-sub    -> extract into a new subfolder
; Commands launch the GUI with an intent argument; the app opens the
; matching confirm screen preloaded (single click to start).

!macro wispzip_register_compress
  WriteRegStr HKCR "AllFilesystemObjects\shell\wispzip-compress" "MUIVerb" "压缩为 ZIP"
  WriteRegStr HKCR "AllFilesystemObjects\shell\wispzip-compress" "Icon" "$INSTDIR\wispzip-app.exe,0"
  WriteRegStr HKCR "AllFilesystemObjects\shell\wispzip-compress\command" "" '"$INSTDIR\wispzip-app.exe" "c" "%1"'
!macroend

!macro wispzip_register_archive ext
  ; extract-to-current-folder: dest defaults to the archive's own directory
  WriteRegStr HKCR "SystemFileAssociations\${ext}\shell\wispzip-here" "MUIVerb" "解压到当前文件夹"
  WriteRegStr HKCR "SystemFileAssociations\${ext}\shell\wispzip-here" "Icon" "$INSTDIR\wispzip-app.exe,0"
  WriteRegStr HKCR "SystemFileAssociations\${ext}\shell\wispzip-here\command" "" '"$INSTDIR\wispzip-app.exe" "xhere" "%1"'
  ; extract-to-subfolder (named after the archive): dest defaults to a sibling subfolder
  WriteRegStr HKCR "SystemFileAssociations\${ext}\shell\wispzip-sub" "MUIVerb" "解压到 $(^Name)"
  WriteRegStr HKCR "SystemFileAssociations\${ext}\shell\wispzip-sub" "Icon" "$INSTDIR\wispzip-app.exe,0"
  WriteRegStr HKCR "SystemFileAssociations\${ext}\shell\wispzip-sub\command" "" '"$INSTDIR\wispzip-app.exe" "x" "%1"'
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro wispzip_register_compress
  !insertmacro wispzip_register_archive ".zip"
  !insertmacro wispzip_register_archive ".7z"
  !insertmacro wispzip_register_archive ".rar"
  !insertmacro wispzip_register_archive ".tar"
  !insertmacro wispzip_register_archive ".gz"
  !insertmacro wispzip_register_archive ".tgz"
  !insertmacro wispzip_register_archive ".bz2"
  !insertmacro wispzip_register_archive ".xz"
  !insertmacro wispzip_register_archive ".zst"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey HKCR "AllFilesystemObjects\shell\wispzip-compress"
  !insertmacro wispzip_unregister_archive ".zip"
  !insertmacro wispzip_unregister_archive ".7z"
  !insertmacro wispzip_unregister_archive ".rar"
  !insertmacro wispzip_unregister_archive ".tar"
  !insertmacro wispzip_unregister_archive ".gz"
  !insertmacro wispzip_unregister_archive ".tgz"
  !insertmacro wispzip_unregister_archive ".bz2"
  !insertmacro wispzip_unregister_archive ".xz"
  !insertmacro wispzip_unregister_archive ".zst"
!macroend

!macro wispzip_unregister_archive ext
  DeleteRegKey HKCR "SystemFileAssociations\${ext}\shell\wispzip-here"
  DeleteRegKey HKCR "SystemFileAssociations\${ext}\shell\wispzip-sub"
!macroend
