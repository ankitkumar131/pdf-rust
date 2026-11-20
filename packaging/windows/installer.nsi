; NSIS installer for pdf-rust.
;
;   makensis /DAPP_VERSION=0.1.0 /DSOURCE_DIR=..\..\target\release \
;            /DPDFIUM_DLL=..\..\runtime\pdfium\pdfium.dll packaging\windows\installer.nsi
;
; The installer copies pdf-rust.exe, pdfium.dll, the licence and the README into
; %ProgramFiles%\pdf-rust, adds Start Menu and (optionally) desktop shortcuts,
; registers "Open with pdf-rust" for .pdf files and writes an uninstaller.

Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"

!ifndef APP_VERSION
  !define APP_VERSION "0.2.0"
!endif
!ifndef SOURCE_DIR
  !define SOURCE_DIR "..\..\target\release"
!endif
!ifndef PDFIUM_DLL
  !define PDFIUM_DLL "..\..\runtime\pdfium\pdfium.dll"
!endif

!define APP_NAME "pdf-rust"
!define APP_EXE "pdf-rust.exe"
!define APP_PUBLISHER "pdf-rust contributors"
!define APP_URL "https://github.com/ankitkumar131/pdf-rust"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"

Name "${APP_NAME} ${APP_VERSION}"
OutFile "..\..\dist\pdf-rust-${APP_VERSION}-setup.exe"
InstallDir "$PROGRAMFILES64\pdf-rust"
InstallDirRegKey HKLM "${UNINST_KEY}" "InstallLocation"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
BrandingText "${APP_NAME} ${APP_VERSION}"

!define MUI_ABORTWARNING
!define MUI_ICON "pdf-rust.ico"
!define MUI_UNICON "pdf-rust.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP_NAME}"
!define MUI_FINISHPAGE_LINK "Project page"
!define MUI_FINISHPAGE_LINK_LOCATION "${APP_URL}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\..\LICENSE"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "pdf-rust (required)" SecCore
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "${SOURCE_DIR}\${APP_EXE}"
  File "${PDFIUM_DLL}"
  File "..\..\LICENSE"
  File "..\..\README.md"
  File "..\..\CHANGELOG.md"
  File "pdf-rust.exe.manifest"

  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "${APP_PUBLISHER}"
  WriteRegStr HKLM "${UNINST_KEY}" "URLInfoAbout" "${APP_URL}"
  WriteRegStr HKLM "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKLM "${UNINST_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "Start Menu shortcut" SecStartMenu
  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortCut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"
  CreateShortCut "$SMPROGRAMS\${APP_NAME}\Uninstall ${APP_NAME}.lnk" "$INSTDIR\uninstall.exe"
SectionEnd

Section "Desktop shortcut" SecDesktop
  CreateShortCut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"
SectionEnd

Section "Open PDF files with pdf-rust" SecAssociate
  ; Registered under HKLM so the entry shows up for every user; the shell only
  ; uses it when the user picks it in "Open with".
  WriteRegStr HKLM "Software\Classes\pdf-rust.pdf\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'
  WriteRegStr HKLM "Software\Classes\pdf-rust.pdf\DefaultIcon" "" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKLM "Software\Classes\.pdf\OpenWithProgids" "pdf-rust.pdf" ""
  WriteRegStr HKLM "Software\Classes\Applications\${APP_EXE}\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'
  WriteRegStr HKLM "Software\Classes\Applications\${APP_EXE}\SupportedTypes" ".pdf" ""
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecCore} "The viewer and its PDFium renderer."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecStartMenu} "Add pdf-rust to the Start Menu."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Add a pdf-rust shortcut to the desktop."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAssociate} "Offer pdf-rust when opening .pdf files."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Function .onInit
  ; 64-bit Windows is required: the shipped PDFium is x64.
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "pdf-rust requires 64-bit Windows."
    Abort
  ${EndIf}
  SetRegView 64
FunctionEnd

Section "Uninstall"
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\pdfium.dll"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\CHANGELOG.md"
  Delete "$INSTDIR\pdf-rust.exe.manifest"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\Uninstall ${APP_NAME}.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"
  Delete "$DESKTOP\${APP_NAME}.lnk"
  SetRegView 64
  DeleteRegKey HKLM "${UNINST_KEY}"
  DeleteRegKey HKLM "Software\Classes\pdf-rust.pdf"
  DeleteRegKey HKLM "Software\Classes\Applications\${APP_EXE}"
  DeleteRegValue HKLM "Software\Classes\.pdf\OpenWithProgids" "pdf-rust.pdf"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd
