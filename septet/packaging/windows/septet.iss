; Septet installer (Inno Setup 6). Built in CI:
;   iscc /DAppVersion=0.1.0 /DSourceDir=<staging dir> /DOutputDir=<out> septet.iss
; Installs for the current user (no administrator rights needed).

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6C7E3B5A-2F1D-4E0B-9A47-5E7E2F3C9D11}
AppName=Septet
AppVersion={#AppVersion}
AppVerName=Septet {#AppVersion}
AppPublisher=louiswalder6
AppPublisherURL=https://gitlab.com/louiswalder6/septet
AppSupportURL=https://gitlab.com/louiswalder6/septet/-/issues
DefaultDirName={autopf}\Septet
DefaultGroupName=Septet
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile={#SourceDir}\LICENSE.md
OutputDir={#OutputDir}
OutputBaseFilename=Septet-{#AppVersion}-windows-x64-setup
SetupIconFile={#SourceDir}\septet.ico
UninstallDisplayIcon={app}\septet.exe
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\septet.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\LICENSE.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\NOTICE.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\THIRD-PARTY-LICENSES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Septet"; Filename: "{app}\septet.exe"
Name: "{autodesktop}\Septet"; Filename: "{app}\septet.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\septet.exe"; Description: "{cm:LaunchProgram,Septet}"; Flags: nowait postinstall skipifsilent
