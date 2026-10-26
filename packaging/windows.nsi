!include "MUI2.nsh"
!ifndef APP_VERSION
!define APP_VERSION "0.2.0"
!endif
!ifndef APP_BINARY
!define APP_BINARY "..\target\release\auvemir.exe"
!endif
Name "Auvemir"
OutFile "..\dist\Auvemir-${APP_VERSION}-windows-x86_64-Setup.exe"
InstallDir "$LOCALAPPDATA\Programs\Auvemir"
RequestExecutionLevel user
!define MUI_ICON "icons\auvemir.ico"
!define MUI_UNICON "icons\auvemir.ico"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
Section "Auvemir"
  SetOutPath "$INSTDIR"
  File "${APP_BINARY}"
  File /nonfatal "..\dist\Auvemir\*.dll"
  File /nonfatal "..\dist\Auvemir\THIRD-PARTY-MINGW.txt"
  File "..\LICENSE"
  File "..\README.md"
  File "..\THIRD_PARTY_NOTICES.md"
  CreateDirectory "$SMPROGRAMS\Auvemir"
  CreateShortcut "$SMPROGRAMS\Auvemir\Auvemir.lnk" "$INSTDIR\auvemir.exe"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Auvemir" "DisplayName" "Auvemir"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Auvemir" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Auvemir" "UninstallString" '$"$INSTDIR\Uninstall.exe$"'
SectionEnd
Section "Uninstall"
  Delete "$INSTDIR\auvemir.exe"
  Delete "$INSTDIR\libstdc++-6.dll"
  Delete "$INSTDIR\libgcc_s_seh-1.dll"
  Delete "$INSTDIR\libwinpthread-1.dll"
  Delete "$INSTDIR\THIRD-PARTY-MINGW.txt"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\THIRD_PARTY_NOTICES.md"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Auvemir\Auvemir.lnk"
  RMDir "$SMPROGRAMS\Auvemir"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Auvemir"
SectionEnd
